//! `kw render` — resolve references into a real file (ARCHITECTURE.md §8.1).
//!
//! The escape hatch of last resort, and it is worth being blunt about why it
//! exists. `kw exec` covers everything Keyward launches; this covers the things it
//! cannot — an IDE-started dev server, a Docker Compose `env_file:`, a `launchd`
//! job. Those receive the literal string `keyward://anthropic` and fail inside
//! somebody else's error message ("invalid API key"), which is the one failure mode
//! guaranteed to send a user back to pasting keys into files.
//!
//! So this command writes plaintext to disk, on purpose, once. Everything here is
//! about making that the smallest possible event:
//!
//! - It refuses any path git would take. A rendered file that gets committed is
//!   strictly worse than never having used Keyward, because the user believes they
//!   are protected.
//! - Mode 0600, set at creation rather than after, so the file is never briefly
//!   world-readable.
//! - It says what it did. A security tool that quietly downgrades is not one.

use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::process::Command;

use keyward_core::find_all;
use serde_json::{Value, json};
use zeroize::Zeroize;

use crate::client::Client;

/// What git thinks of the output path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cover {
    /// `.gitignore` covers it. The only case that may be written.
    Ignored,
    /// Git would happily take this file.
    Tracked,
    /// No repository, or no git. Unknown is not permission.
    Unknown,
}

pub fn run(args: &[String]) -> i32 {
    let mut template: Option<&str> = None;
    let mut out: Option<&str> = None;
    let mut wants_out = false;
    for arg in args {
        match arg.as_str() {
            "-o" | "--out" => wants_out = true,
            "-h" | "--help" => {
                println!("usage: kw render <template> -o <out>");
                return 0;
            }
            other if wants_out => {
                out = Some(other);
                wants_out = false;
            }
            other if other.starts_with('-') => {
                eprintln!("kw: unknown option `{other}`\n\nusage: kw render <template> -o <out>");
                return 2;
            }
            other => template = Some(other),
        }
    }

    let (Some(template), Some(out)) = (template, out) else {
        eprintln!("kw: usage: kw render <template> -o <out>");
        return 2;
    };

    match render(Path::new(template), Path::new(out)) {
        Ok(count) => {
            println!(
                "Wrote {out} with {count} resolved value(s), mode 0600.\n\
                 This is the one place Keyward puts plaintext on disk. Delete it when \
                 the tool that needed it is done, and prefer `kw exec -- <command>` \
                 anywhere you can launch the process yourself."
            );
            0
        }
        Err(e) => {
            eprintln!("kw: {e}");
            e.exit_code()
        }
    }
}

/// Errors worth their own text, because each one has a different next step.
pub enum Error {
    NotIgnored { path: String, cover: Cover },
    Client(crate::client::Error),
    Io(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::NotIgnored { path, cover } => {
                let why = match cover {
                    Cover::Tracked => ".gitignore does not cover it",
                    _ => "there is no .gitignore here that covers it",
                };
                write!(
                    f,
                    "refusing to write `{path}` — {why}.\n    \
                     This file gets real secret values written into it, so it has to be \
                     one git will never take. Add `{path}` to .gitignore and run this again."
                )
            }
            Error::Client(e) => write!(f, "{e}"),
            Error::Io(m) => write!(f, "{m}"),
        }
    }
}

impl Error {
    fn exit_code(&self) -> i32 {
        match self {
            Error::Client(e) => e.exit_code(),
            _ => 1,
        }
    }
}

/// Resolve every reference in `template` and write the result to `out`.
///
/// The gitignore check runs before the daemon is contacted, so a refusal costs no
/// approval prompt and — more importantly — never asks the daemon for plaintext it
/// would then have nowhere safe to put.
pub fn render(template: &Path, out: &Path) -> Result<usize, Error> {
    let cover = cover_of(out);
    guard(out, cover)?;

    let text = std::fs::read_to_string(template)
        .map_err(|e| Error::Io(format!("could not read `{}`: {e}", template.display())))?;

    let found = find_all(&text);
    if found.is_empty() {
        return Err(Error::Io(format!(
            "`{}` has no keyward:// references in it — nothing to render",
            template.display()
        )));
    }
    let mut names: Vec<String> = found
        .iter()
        .map(|f| f.reference.name().to_owned())
        .collect();
    names.sort();
    names.dedup();

    let project = std::env::current_dir()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()));
    let result = Client::connect()
        .map_err(Error::Client)?
        .call(
            "secret.hand",
            json!({"names": names, "actor": "kw render", "project": project}),
        )
        .map_err(Error::Client)?;

    let values = result
        .get("values")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();

    // Splice back to front so every span stays valid as earlier ones are replaced.
    let mut rendered = text.clone();
    let mut resolved = 0usize;
    for hit in found.iter().rev() {
        let Some(value) = values.get(hit.reference.name()).and_then(Value::as_str) else {
            continue;
        };
        if rendered.get(hit.start..hit.end).is_some() {
            rendered.replace_range(hit.start..hit.end, value);
            resolved += 1;
        }
    }

    let write = write_0600(out, &rendered);
    rendered.zeroize();
    write?;
    Ok(resolved)
}

/// The refusal. Kept separate from the git call so it is decidable in a test
/// without a repository, and so there is exactly one place that says yes.
fn guard(out: &Path, cover: Cover) -> Result<(), Error> {
    if cover == Cover::Ignored {
        return Ok(());
    }
    Err(Error::NotIgnored {
        path: out.display().to_string(),
        cover,
    })
}

/// Ask git, rather than parsing `.gitignore` here.
///
/// Precedence across nested ignore files, negation patterns, `core.excludesFile`
/// and `.git/info/exclude` is a real specification, and a second implementation of
/// it that disagrees with the first would be wrong in the direction that matters —
/// it would say "ignored" about a file git is about to commit.
fn cover_of(out: &Path) -> Cover {
    // `check-ignore` answers about a path, not a file, so this works before the
    // file exists — which is the normal case here.
    // Git's own diagnostics go nowhere: "is outside repository" is true but it is
    // not the sentence the user needs, and printing both makes ours look like noise.
    let Ok(status) = Command::new("git")
        .arg("check-ignore")
        .arg("--quiet")
        .arg(out)
        .stderr(std::process::Stdio::null())
        .status()
    else {
        return Cover::Unknown;
    };
    match status.code() {
        Some(0) => Cover::Ignored,
        Some(1) => Cover::Tracked,
        // 128 is "not a git repository" or a bad path. Either way this is not a
        // yes, and a file Keyward cannot reason about does not get plaintext.
        _ => Cover::Unknown,
    }
}

fn write_0600(out: &Path, contents: &str) -> Result<(), Error> {
    if let Some(parent) = out.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)
            .map_err(|e| Error::Io(format!("could not create `{}`: {e}", parent.display())))?;
    }
    // `.mode()` at creation, and `truncate` rather than removing and recreating: a
    // window where the file exists with default permissions is a window where any
    // process on the machine can read the value.
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(out)
        .map_err(|e| Error::Io(format!("could not write `{}`: {e}", out.display())))?;
    // `.mode()` only applies when the file is created, so an existing file would
    // keep whatever mode it already had. Re-render into a file someone once made
    // world-readable and it stays world-readable — with a real value in it now.
    let perms = <std::fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o600);
    std::fs::set_permissions(out, perms)
        .map_err(|e| Error::Io(format!("could not set 0600 on `{}`: {e}", out.display())))?;
    file.write_all(contents.as_bytes())
        .map_err(|e| Error::Io(format!("could not write `{}`: {e}", out.display())))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_ignored_path_may_be_written() {
        assert!(guard(Path::new(".env.local"), Cover::Ignored).is_ok());
        assert!(guard(Path::new(".env"), Cover::Tracked).is_err());
        // Not knowing is not permission. Outside a repository there is nothing
        // stopping the file being committed later by a repository created around it.
        assert!(guard(Path::new(".env"), Cover::Unknown).is_err());
    }

    #[test]
    fn the_refusal_says_what_to_do() {
        let Err(e) = guard(Path::new("config.toml"), Cover::Tracked) else {
            unreachable!("should refuse")
        };
        let message = e.to_string();
        assert!(message.contains(".gitignore"), "{message}");
        assert!(message.contains("config.toml"), "{message}");
    }

    #[test]
    fn refuses_before_touching_the_daemon_or_the_disk() {
        // The ordering is the point: a refusal must not have already asked for
        // plaintext, and must leave nothing behind.
        let dir = std::env::temp_dir().join(format!("kw-render-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let template = dir.join("config.tmpl");
        let out = dir.join("config.toml");
        let _ = std::fs::write(&template, "key = \"keyward://stripe\"\n");

        // Whatever the surrounding checkout looks like, a temp directory is not a
        // path any .gitignore in this repository covers.
        assert!(matches!(cover_of(&out), Cover::Tracked | Cover::Unknown));
        assert!(render(&template, &out).is_err());
        assert!(!out.exists(), "a refused render must write nothing");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
