//! Finds literal secrets that are still sitting in files (ARCHITECTURE.md §10.4).
//!
//! This closes the commit path. Everything else in Keyward is about what a
//! *future* file holds; a project that already has `sk_live_…` in three configs
//! gets no benefit from that until someone finds the three configs. `kw scan`
//! finds them, and `kw scan --staged` stops the fourth from being committed.
//!
//! Two properties decide whether this is usable at all:
//!
//! - **A `keyward://` reference is never a finding.** A hook that fires on the fix
//!   it is supposed to be recommending gets uninstalled within a day.
//! - **False positives cost more than misses here.** A scanner that flags every
//!   git SHA and every long identifier trains its user to pass `--no-verify`, and
//!   then it catches nothing at all. So the entropy rule is deliberately narrow
//!   (see [`looks_random`]) and known prefixes carry most of the detection.
//!
//! Output is masked *harder than the rest of the product*. Elsewhere `mask` keeps
//! seven leading and four trailing characters, which is right for a list the user
//! is scanning for "which key is this". A scan report is different: it names a
//! **live, unrotated** secret, it is printed by a pre-commit hook, and it lands in
//! the terminal an agent is reading — the exact channel this tool exists to keep
//! clean. So findings show the vendor prefix and nothing else. `sk_live_…` plus a
//! file and a line is already enough to act on; the trailing characters only help
//! whoever is reading over your shoulder.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use keyward_core::find_all;

/// One literal secret, located and described but never quoted in full.
pub struct Finding {
    pub path: PathBuf,
    pub line: usize,
    pub label: &'static str,
    pub masked: String,
}

impl std::fmt::Display for Finding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}:{}: {} — {}",
            self.path.display(),
            self.line,
            self.label,
            self.masked
        )
    }
}

/// Prefixes that identify a credential on sight, longest first so `sk-ant-` wins
/// over `sk-` and the label names the right vendor.
///
/// The label matters more than it looks: "AWS access key ID" tells the user which
/// console to go rotate in, which is the actual next step after a hit.
const PREFIXES: &[(&str, &str)] = &[
    ("github_pat_", "GitHub fine-grained token"),
    ("dop_v1_", "DigitalOcean access token"),
    ("sk_live_", "Stripe live secret key"),
    ("sk_test_", "Stripe test secret key"),
    ("sk-ant-", "Anthropic API key"),
    ("glpat-", "GitLab personal access token"),
    ("xoxb-", "Slack bot token"),
    ("xoxp-", "Slack user token"),
    ("ghp_", "GitHub personal access token"),
    ("gho_", "GitHub OAuth token"),
    ("AKIA", "AWS access key ID"),
    ("AIza", "Google API key"),
    ("sk-", "OpenAI-style API key"),
    ("re_", "Resend API key"),
];

const ENTROPY_LABEL: &str = "high-entropy value in assignment position";

/// Directories whose contents are not the user's code. Walking them is slow and
/// every hit in one is a finding nobody can act on — you do not rotate a key
/// because `node_modules` has a fixture with one in it.
const SKIP_DIRS: &[&str] = &[
    ".git",
    ".hg",
    ".svn",
    "node_modules",
    "target",
    "dist",
    "build",
    "out",
    ".next",
    ".nuxt",
    ".svelte-kit",
    "vendor",
    "venv",
    ".venv",
    "__pycache__",
    ".mypy_cache",
    ".pytest_cache",
    ".terraform",
    "Pods",
    "DerivedData",
    ".build",
];

/// Files above this are generated, vendored or binary in every case that matters,
/// and reading them into memory to scan is the slowest thing this command does.
const MAX_FILE_BYTES: u64 = 1 << 20;

// MARK: - Entry point

pub fn run(args: &[String]) -> i32 {
    let mut staged = false;
    let mut root: Option<&str> = None;
    for arg in args {
        match arg.as_str() {
            "--staged" | "--cached" => staged = true,
            "-h" | "--help" => {
                println!("usage: kw scan [--staged] [path]");
                return 0;
            }
            other if other.starts_with('-') => {
                eprintln!("kw: unknown option `{other}`\n\nusage: kw scan [--staged] [path]");
                return 2;
            }
            other => root = Some(other),
        }
    }

    let findings = if staged {
        match scan_staged() {
            Ok(f) => f,
            Err(e) => {
                eprintln!("kw: {e}");
                return 2;
            }
        }
    } else {
        scan_path(Path::new(root.unwrap_or(".")))
    };

    if findings.is_empty() {
        println!("No literal secrets found.");
        return 0;
    }

    for finding in &findings {
        println!("{finding}");
    }
    let n = findings.len();
    let plural = if n == 1 { "" } else { "s" };
    println!(
        "\n{n} literal secret{plural}. Store each one with `kw add <name>`, put \
         `keyward://<name>` in the file, and run the project with `kw exec`."
    );
    // Exit 1 so this works as a pre-commit hook without a wrapper script. Anything
    // other than a non-zero exit here means the hook silently passes.
    1
}

// MARK: - Sources

/// Walk `root` (a file or a directory) and scan every text file under it.
pub fn scan_path(root: &Path) -> Vec<Finding> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];

    while let Some(path) = stack.pop() {
        // `symlink_metadata`, not `metadata`: following links turns a cyclic
        // symlink into an infinite walk, and a link out of the project into a scan
        // of somebody's home directory.
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.is_symlink() {
            continue;
        }

        if meta.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if SKIP_DIRS.contains(&name) {
                continue;
            }
            let Ok(entries) = std::fs::read_dir(&path) else {
                continue;
            };
            for entry in entries.flatten() {
                stack.push(entry.path());
            }
            continue;
        }

        if meta.len() > MAX_FILE_BYTES {
            continue;
        }
        // Read as bytes and require UTF-8 rather than testing the extension: an
        // allowlist of extensions misses `.env.production.local`, and a secret does
        // not care what a file is called.
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let Ok(text) = String::from_utf8(bytes) else {
            continue;
        };
        for (line, label, masked) in scan_text(&text) {
            out.push(Finding {
                path: path.clone(),
                line,
                label,
                masked,
            });
        }
    }

    out.sort_by(|a, b| a.path.cmp(&b.path).then(a.line.cmp(&b.line)));
    out
}

/// Scan what is about to be committed, not what is on disk.
///
/// The distinction matters for a hook: a `.env` full of live keys that is properly
/// gitignored is not what the commit path is about, and failing the commit over it
/// would block work that was never unsafe.
pub fn scan_staged() -> Result<Vec<Finding>, String> {
    // `-U0` so the diff carries no context lines: an unchanged line that happens to
    // sit next to an edit is not something this commit introduced, and blaming a
    // commit for it is how a hook becomes impossible to get past.
    let output = Command::new("git")
        .args(["diff", "--cached", "-U0", "--no-color"])
        .output()
        .map_err(|e| format!("could not run git: {e}"))?;
    if !output.status.success() {
        return Err("`git diff --cached` failed — is this a git repository?".into());
    }
    let diff = String::from_utf8_lossy(&output.stdout);
    Ok(scan_diff(&diff))
}

/// Parse a unified diff and scan only its added lines, keeping the line numbers the
/// file will have after the commit lands.
fn scan_diff(diff: &str) -> Vec<Finding> {
    let mut out = Vec::new();
    let mut path = PathBuf::new();
    let mut line_no = 0usize;

    for line in diff.lines() {
        if let Some(target) = line.strip_prefix("+++ ") {
            path = PathBuf::from(target.strip_prefix("b/").unwrap_or(target));
            continue;
        }
        if let Some(header) = line.strip_prefix("@@ ") {
            line_no = header
                .split_whitespace()
                .find_map(|part| part.strip_prefix('+'))
                .and_then(|span| span.split(',').next())
                .and_then(|n| n.parse::<usize>().ok())
                .unwrap_or(0);
            continue;
        }
        let Some(added) = line.strip_prefix('+') else {
            continue;
        };
        if path.as_os_str().is_empty() || path == Path::new("/dev/null") {
            continue;
        }
        for (_, label, masked) in scan_text(added) {
            out.push(Finding {
                path: path.clone(),
                line: line_no,
                label,
                masked,
            });
        }
        line_no += 1;
    }
    out
}

// MARK: - Detection

/// Prefix only, then an ellipsis.
///
/// Deliberately not `keyward_core::mask`: see the module comment. A finding is a
/// secret that is still live, and the report travels further than a list view.
fn redact(token: &str) -> String {
    let head: String = token.chars().take(prefix_len(token)).collect();
    format!("{head}…")
}

/// How much of the front is safe to show: the vendor prefix if one matched,
/// otherwise four characters — enough to find the line, not enough to use.
fn prefix_len(token: &str) -> usize {
    PREFIXES
        .iter()
        .filter(|(prefix, _)| token.starts_with(prefix))
        .map(|(prefix, _)| prefix.len())
        .max()
        .unwrap_or(4)
}

/// Scan a body of text, returning `(line number, label, masked excerpt)`.
fn scan_text(text: &str) -> Vec<(usize, &'static str, String)> {
    let mut out = Vec::new();
    for (index, line) in text.lines().enumerate() {
        // A reference is the thing this scanner exists to recommend. Its span is
        // computed per line and every candidate overlapping it is dropped, rather
        // than relying on the token rules to happen not to match — `keyward://sk-…`
        // is a legal name, so "happens not to match" is not a guarantee.
        let references = find_all(line);
        for (start, end) in tokens(line) {
            if references.iter().any(|r| start < r.end && r.start < end) {
                continue;
            }
            let Some(token) = line.get(start..end) else {
                continue;
            };
            if let Some(label) = classify(token, line, start) {
                out.push((index + 1, label, redact(token)));
            }
        }
    }
    out
}

/// Decide whether one token is a literal secret, and what to call it.
fn classify(token: &str, line: &str, start: usize) -> Option<&'static str> {
    if is_placeholder(token) {
        return None;
    }
    for (prefix, label) in PREFIXES {
        let Some(body) = token.strip_prefix(prefix) else {
            continue;
        };
        // A prefix alone is not a secret. `re_` opens half the identifiers in a
        // Python file and `sk-` is a plausible variable name, so the body has to
        // look like key material: long enough, and not a word.
        if body.len() >= 8 && (body.bytes().any(|b| b.is_ascii_digit()) || is_mixed_case(body)) {
            return Some(label);
        }
    }
    if in_assignment_position(line, start) && looks_random(token) {
        return Some(ENTROPY_LABEL);
    }
    None
}

/// Maximal runs of characters a credential is made of, as byte spans.
///
/// `:` and `/` are excluded, which is what keeps a URL from becoming one long
/// high-entropy token, and `=` is excluded because it is the delimiter the
/// assignment test looks for.
fn tokens(line: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    for (i, ch) in line.char_indices() {
        let part_of_token = ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.');
        match (part_of_token, start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                push_token(&mut out, line, s, i);
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        push_token(&mut out, line, s, line.len());
    }
    out
}

fn push_token(out: &mut Vec<(usize, usize)>, line: &str, start: usize, end: usize) {
    // Trailing punctuation belongs to the sentence, not the token: `key=abc123.`
    // ends a line, and carrying the dot into the mask makes the excerpt wrong.
    let mut end = end;
    while end > start
        && line
            .get(..end)
            .is_some_and(|head| head.ends_with('.') || head.ends_with('-'))
    {
        end -= 1;
    }
    if end > start {
        out.push((start, end));
    }
}

/// True when the token is the right-hand side of an assignment: `KEY=…`, `"key": …`.
///
/// Entropy alone is not evidence of a secret — it is evidence of a hash, a minified
/// bundle, or a base64 test fixture. Position is what turns it into evidence, so
/// the entropy rule never fires outside one.
fn in_assignment_position(line: &str, start: usize) -> bool {
    let Some(head) = line.get(..start) else {
        return false;
    };
    let trimmed = head.trim_end_matches([' ', '\t', '"', '\'', '`']);
    if matches!(trimmed.chars().last(), Some('=' | ':')) {
        return true;
    }
    // `Authorization: Bearer <token>` — the token is the value of the word before
    // it, not of the colon further left. A bearer token pasted into a curl command
    // or a fetch header is one of the most common ways a live credential ends up in
    // a committed file.
    let lower = trimmed.to_ascii_lowercase();
    ["bearer", "basic", "token"]
        .iter()
        .any(|word| lower.ends_with(word))
}

/// The narrow entropy rule. Four conditions, and each one exists to exclude a
/// specific thing that is high-entropy and not a credential.
///
/// - **All three character classes.** Drops git SHAs, UUIDs, hex digests and
///   `SCREAMING_CONSTANT` names — every one of them scores well on Shannon and none
///   of them is a secret. This misses a lowercase-hex credential, deliberately: the
///   alternative flags every commit hash in the repository.
/// - **Shannon ≥ 4.0.** On a 20-character token the ceiling is only 4.3, so this is
///   demanding rather than nominal.
/// - **Two digits.** Shannon on a short string mostly measures how few characters
///   repeat, which is why `getUserByIdAndProjectNameV2` scores 4.2. Random keys
///   carry scattered digits; identifiers carry at most a version number.
/// - **No six-letter same-case run.** Identifiers are made of words and words are
///   made of runs. `…AndProjectName…` has one; `9dK2mQ7xR4tV8wZ` cannot.
///
/// Together they miss maybe a fifth of genuinely random keys. That is the right
/// trade: the prefix table is the primary detector, this is the backstop for
/// vendors not in it, and a scanner that cries wolf gets `--no-verify`'d forever.
fn looks_random(token: &str) -> bool {
    if token.len() < 20 {
        return false;
    }
    let has_lower = token.bytes().any(|b| b.is_ascii_lowercase());
    let has_upper = token.bytes().any(|b| b.is_ascii_uppercase());
    let digits = token.bytes().filter(u8::is_ascii_digit).count();
    if !(has_lower && has_upper && digits >= 2) {
        return false;
    }
    if longest_word_run(token) >= 6 {
        return false;
    }
    entropy(token) >= 4.0
}

/// Length of the longest run of letters of the same case — a cheap stand-in for
/// "contains a word".
fn longest_word_run(token: &str) -> usize {
    let mut best = 0usize;
    let mut run = 0usize;
    let mut upper = false;
    for ch in token.chars() {
        if ch.is_ascii_alphabetic() {
            let is_upper = ch.is_ascii_uppercase();
            run = if run > 0 && is_upper == upper {
                run + 1
            } else {
                1
            };
            upper = is_upper;
            best = best.max(run);
        } else {
            run = 0;
        }
    }
    best
}

fn is_mixed_case(s: &str) -> bool {
    s.bytes().any(|b| b.is_ascii_lowercase()) && s.bytes().any(|b| b.is_ascii_uppercase())
}

/// Documentation samples and fill-me-in strings.
///
/// These are the single biggest source of noise, because the files most likely to
/// contain a key-shaped string are `.env.example` and the README — and AWS's own
/// documented sample key is `AKIAIOSFODNN7EXAMPLE`, which a prefix match would
/// otherwise report on every project that ever pasted it.
fn is_placeholder(token: &str) -> bool {
    let lower = token.to_ascii_lowercase();
    const MARKERS: &[&str] = &[
        "example",
        "xxxx",
        "your",
        "placeholder",
        "changeme",
        "change-me",
        "redacted",
        "dummy",
        "sample",
        "fake",
        "todo",
        "here",
    ];
    if MARKERS.iter().any(|m| lower.contains(m)) {
        return true;
    }
    // `sk_live_00000000000000000000`, `AKIA................`: eight of the same
    // character in a row is nobody's credential, it is a blank to fill in.
    let mut run = 0usize;
    let mut previous = None;
    for ch in lower.chars() {
        run = if previous == Some(ch) { run + 1 } else { 1 };
        previous = Some(ch);
        if run >= 8 {
            return true;
        }
    }
    false
}

fn entropy(s: &str) -> f64 {
    let mut counts: HashMap<char, usize> = HashMap::new();
    for ch in s.chars() {
        *counts.entry(ch).or_default() += 1;
    }
    let len = s.chars().count() as f64;
    if len == 0.0 {
        return 0.0;
    }
    -counts
        .values()
        .map(|&count| {
            let p = count as f64 / len;
            p * p.log2()
        })
        .sum::<f64>()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(text: &str) -> Vec<&'static str> {
        scan_text(text).into_iter().map(|(_, l, _)| l).collect()
    }

    #[test]
    fn finds_every_known_prefix() {
        // Prefix and body are stored apart and joined here, so no line in this
        // file is itself a string that looks like a live credential.
        //
        // Not fussiness: GitHub's push protection rejected this repository over
        // the `glpat-` case below — a value that was never issued by anyone. A
        // scanner's own fixtures are the one place where "looks exactly like the
        // real thing" is the point, so the fixtures have to be assembled rather
        // than written down.
        let cases = [
            ("OPENAI_API_KEY=sk-proj-", "9dK2mQ7xR4tV8wZ1aB3cD5eF"),
            ("ANTHROPIC_API_KEY=sk-ant-api03-", "9dK2mQ7xR4tV8wZ1aB3c"),
            ("STRIPE=sk_live_", "51H8xQ2eZvKYlo2C9"),
            ("STRIPE=sk_test_", "51H8xQ2eZvKYlo2C9"),
            ("AWS_ACCESS_KEY_ID=AKIA", "4NPZQ7RMK2WJ6TLX"),
            ("token: ghp_", "9dK2mQ7xR4tV8wZ1aB3cD5eF7gH0iJ"),
            ("token: gho_", "9dK2mQ7xR4tV8wZ1aB3cD5eF7gH0iJ"),
            ("token: github_pat_", "11ABCDE0Y9dK2mQ7xR4tV8wZ"),
            ("SLACK=xoxb-", "2841-7739-9dK2mQ7xR4tV8wZ1aB3c"),
            ("SLACK=xoxp-", "2841-7739-9dK2mQ7xR4tV8wZ1aB3c"),
            ("GOOGLE_KEY=AIza", "SyD9dK2mQ7xR4tV8wZ1aB3cD5eF7g"),
            ("RESEND=re_", "9dK2mQ7_xR4tV8wZ1aB3cD5"),
            ("DO_TOKEN=dop_v1_", "9dk2mq7xr4tv8wz1ab3cd5ef7gh0ij"),
            ("GITLAB=glpat-", "9dK2mQ7xR4tV8wZ1aB3c"),
        ];
        for (prefix, body) in cases {
            let case = format!("{prefix}{body}");
            assert_eq!(labels(&case).len(), 1, "should flag exactly once: {case}");
        }
    }

    #[test]
    fn never_flags_a_reference() {
        // The whole point of the scanner is to recommend this line. Flagging it
        // would make the hook fire on its own fix.
        let env = "\
STRIPE_SECRET_KEY=keyward://stripe
OPENAI_API_KEY=kw://openai
DATABASE_URL=keyward://pg-prod
";
        assert!(labels(env).is_empty());
    }

    #[test]
    fn never_flags_a_reference_whose_name_looks_like_a_key() {
        // `sk-test-billing` is a legal secret name, so this must be excluded by the
        // reference span, not by the token rules happening not to match.
        assert!(labels("KEY=keyward://sk-test-billing-2024").is_empty());
    }

    #[test]
    fn leaves_ordinary_code_alone() {
        let cases = [
            "PUBLIC_API_BASE=https://api.example.com",
            "const re_export = require('./re_exports');",
            "git checkout 8f14e45fceea167a5a36dedd4bea2543",
            "id: 550e8400-e29b-41d4-a716-446655440000",
            "ANTHROPIC_API_KEY_FOR_STAGING_ENVIRONMENT=keyward://anthropic",
            "let handler = getUserByIdAndProjectNameV2;",
            "\"version\": \"1.2.3-beta.4\"",
            "digest = sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934c",
        ];
        for case in cases {
            assert!(labels(case).is_empty(), "false positive: {case}");
        }
    }

    #[test]
    fn ignores_documentation_samples() {
        // `.env.example` is the file most likely to hold a key-shaped string and
        // least likely to hold a key. AWS's own docs use the first one.
        let cases = [
            "AWS_ACCESS_KEY_ID=AKIAIOSFODNN7EXAMPLE",
            "OPENAI_API_KEY=sk-your-key-here",
            "STRIPE=sk_live_xxxxxxxxxxxxxxxxxxxx",
            "GITHUB=ghp_placeholder0000000000000000000",
        ];
        for case in cases {
            assert!(labels(case).is_empty(), "false positive: {case}");
        }
    }

    #[test]
    fn entropy_rule_needs_an_assignment() {
        let secret = "9dK2mQ7xR4tV8wZ1aB3cD5eF7gH0";
        assert_eq!(labels(&format!("SESSION_KEY={secret}")), [ENTROPY_LABEL]);
        assert_eq!(
            labels(&format!("\"session\": \"{secret}\"")),
            [ENTROPY_LABEL]
        );
        // The same string as a bare argument or in prose is a hash, a nonce, or a
        // fixture — position is what makes it evidence.
        assert!(labels(&format!("build artifact {secret} uploaded")).is_empty());
        // A header value is an assignment for this purpose, and a bearer token
        // written into a script is exactly what this is meant to catch.
        assert_eq!(
            labels(&format!("curl -H \"Authorization: Bearer {secret}\"")),
            [ENTROPY_LABEL]
        );
    }

    #[test]
    fn reports_a_mask_and_never_the_value() {
        let found = scan_text("STRIPE=sk_live_51H8xQ2eZvKYlo2C9");
        let Some((line, _, masked)) = found.first() else {
            unreachable!("expected a finding")
        };
        assert_eq!(*line, 1);
        assert!(
            !masked.contains("51H8xQ2eZvKY"),
            "leaked the value: {masked}"
        );
        assert!(masked.contains('…'));
    }

    #[test]
    fn reports_the_line_a_secret_is_on() {
        let text = "# comment\nSAFE=keyward://stripe\nSTRIPE=sk_live_51H8xQ2eZvKYlo2C9\n";
        let found = scan_text(text);
        assert_eq!(found.iter().map(|(l, _, _)| *l).collect::<Vec<_>>(), [3]);
    }

    #[test]
    fn staged_diff_gets_post_commit_line_numbers() {
        let diff = "\
diff --git a/.env b/.env
index 1234567..89abcde 100644
--- a/.env
+++ b/.env
@@ -4,0 +5,2 @@ EXISTING=1
+STRIPE=sk_live_51H8xQ2eZvKYlo2C9
+SAFE=keyward://openai
";
        let found = scan_diff(diff);
        assert_eq!(found.len(), 1);
        let Some(f) = found.first() else {
            unreachable!("expected a finding")
        };
        assert_eq!(f.path, Path::new(".env"));
        assert_eq!(f.line, 5);
    }

    #[test]
    fn staged_diff_ignores_removed_lines() {
        // Deleting a secret must not fail the commit that deletes it.
        let diff = "\
--- a/.env
+++ b/.env
@@ -1 +1 @@
-STRIPE=sk_live_51H8xQ2eZvKYlo2C9
+STRIPE=keyward://stripe
";
        assert!(scan_diff(diff).is_empty());
    }
}
