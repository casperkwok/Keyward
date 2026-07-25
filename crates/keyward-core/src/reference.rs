//! The `keyward://` reference grammar (ARCHITECTURE.md §8).
//!
//! A reference names a secret without being one. It is what gets committed to git,
//! written into `.env`, and handed to a coding agent, so the parser is deliberately
//! strict: anything not exactly well-formed fails loudly rather than being coerced.
//! A reference that silently means something other than what it looks like is the
//! one failure mode this format exists to prevent.
//!
//! ```text
//! ref    ::= scheme "://" name
//! scheme ::= "keyward" | "kw"
//! name   ::= [a-z0-9]([a-z0-9-]{0,62}[a-z0-9])?
//! ```
//!
//! One secret is one name and one value, so a reference is one segment. An earlier
//! draft had `keyward://profile/field` to group several fields under a provider;
//! that modelled a product this is no longer.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Canonical scheme. [`Reference`] always renders with this one, even when parsed
/// from the `kw://` short form — one spelling on disk keeps diffs stable.
pub const SCHEME: &str = "keyward";
/// Accepted short form. Convenient to type, never emitted.
pub const SCHEME_SHORT: &str = "kw";

const MAX_NAME: usize = 64;

/// A parsed `keyward://name` reference.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Reference(String);

impl Reference {
    /// Build from an already-known name, validating it.
    pub fn new(name: impl Into<String>) -> Result<Self, ParseRefError> {
        let name = name.into();
        validate(&name)?;
        Ok(Self(name))
    }

    /// The secret's slug, e.g. `stripe`.
    pub fn name(&self) -> &str {
        &self.0
    }

    /// Derive a slug from a human-entered display name: `Stripe 测试` → `stripe`.
    ///
    /// Returns `None` when nothing usable survives — a name of only CJK or emoji
    /// has no ASCII slug, and inventing one (transliteration, a hash) would produce
    /// a reference the user cannot predict or recognise in their own `.env`. The
    /// caller asks them to type one instead.
    pub fn slugify(display: &str) -> Option<Self> {
        let mut out = String::new();
        let mut prev_dash = false;
        for ch in display.chars() {
            if ch.is_ascii_alphanumeric() {
                out.push(ch.to_ascii_lowercase());
                prev_dash = false;
            } else if !out.is_empty() && !prev_dash {
                out.push('-');
                prev_dash = true;
            }
        }
        while out.ends_with('-') {
            out.pop();
        }
        out.truncate(MAX_NAME);
        while out.ends_with('-') {
            out.pop();
        }
        Self::new(out).ok()
    }
}

impl fmt::Display for Reference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{SCHEME}://{}", self.0)
    }
}

impl std::str::FromStr for Reference {
    type Err = ParseRefError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // No trimming. A reference with surrounding whitespace came from something
        // that mangled it, and guessing is how a parser starts accepting input its
        // callers did not mean.
        let name = s
            .strip_prefix(SCHEME)
            .or_else(|| s.strip_prefix(SCHEME_SHORT))
            .and_then(|r| r.strip_prefix("://"))
            .ok_or(ParseRefError::BadScheme)?;
        validate(name)?;
        Ok(Self(name.to_owned()))
    }
}

// Serialised as the canonical string, so `vault.json` and the IPC protocol carry
// `"keyward://stripe"` rather than a wrapper object.
impl Serialize for Reference {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Reference {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

fn validate(s: &str) -> Result<(), ParseRefError> {
    // `@` and `/` get their own errors: both are things a person may reasonably
    // try, and "invalid name" would leave them guessing which character was wrong.
    if s.contains('@') {
        return Err(ParseRefError::VersionUnsupported);
    }
    if s.contains('/') {
        return Err(ParseRefError::PathUnsupported);
    }
    if s.is_empty() || s.len() > MAX_NAME {
        return Err(ParseRefError::BadName);
    }
    let chars_ok = s
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    // Leading or trailing hyphens make names that are hard to read in a list and
    // easy to mistype; reject rather than normalise.
    let edges_ok = !s.starts_with('-') && !s.ends_with('-');
    if chars_ok && edges_ok {
        Ok(())
    } else {
        Err(ParseRefError::BadName)
    }
}

/// Why a string is not a reference.
///
/// Kept specific: these reach users through `kw exec` when a `.env` line is
/// malformed, and "invalid reference" on line 14 of a `.env` is not actionable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ParseRefError {
    #[error("not a Keyward reference (expected `keyward://name`)")]
    BadScheme,
    #[error(
        "invalid name: expected 1-64 characters of a-z, 0-9 or '-', not starting or ending with '-'"
    )]
    BadName,
    #[error("version pinning is not supported yet; use `keyward://name`")]
    VersionUnsupported,
    #[error("a reference is a single name: `keyward://stripe`, not `keyward://stripe/key`")]
    PathUnsupported,
}

/// A reference found inside a larger body of text, with its byte span.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub reference: Reference,
    /// Byte range of the matched text, so a caller can splice a resolved value in
    /// without searching again.
    pub start: usize,
    pub end: usize,
}

/// Find every reference in arbitrary text — a `.env`, an `mcp.json`, a staged diff.
///
/// Matches are bounded by identifier characters on the left, so `xkw://a` is not a
/// match. Only well-formed references are returned; a malformed near-miss is
/// skipped rather than reported, because callers are scanning haystacks where
/// "looks a bit like a reference" is noise.
pub fn find_all(text: &str) -> Vec<Found> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();

    for (i, _) in text.match_indices("://") {
        let Some(head) = text.get(..i) else { continue };
        let start = head
            .rfind(|c: char| !c.is_ascii_alphabetic())
            .map_or(0, |p| p + 1);
        let Some(scheme) = text.get(start..i) else {
            continue;
        };
        if scheme != SCHEME && scheme != SCHEME_SHORT {
            continue;
        }
        // Reject `x-kw://…`: an identifier character immediately before the scheme
        // means this is part of a longer token, not a reference.
        if let Some(prev) = start.checked_sub(1).and_then(|p| bytes.get(p))
            && (prev.is_ascii_alphanumeric() || *prev == b'_' || *prev == b'-')
        {
            continue;
        }

        let Some(after) = text.get(i + 3..) else {
            continue;
        };
        let len = after
            .find(|c: char| !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'))
            .unwrap_or(after.len());
        let end = i + 3 + len;

        let Some(candidate) = text.get(start..end) else {
            continue;
        };
        if let Ok(reference) = candidate.parse::<Reference>() {
            out.push(Found {
                reference,
                start,
                end,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(s: &str) -> Reference {
        match s.parse::<Reference>() {
            Ok(v) => v,
            Err(e) => unreachable!("`{s}` should parse: {e}"),
        }
    }

    #[test]
    fn parses_canonical_form() {
        assert_eq!(r("keyward://stripe").name(), "stripe");
    }

    #[test]
    fn short_scheme_is_accepted_but_never_emitted() {
        assert_eq!(r("kw://stripe").to_string(), "keyward://stripe");
    }

    #[test]
    fn rejects_malformed() {
        let cases = [
            ("http://stripe", ParseRefError::BadScheme),
            ("keyward:/stripe", ParseRefError::BadScheme),
            ("notkeyward://stripe", ParseRefError::BadScheme),
            ("keyward://", ParseRefError::BadName),
            ("keyward://Stripe", ParseRefError::BadName),
            ("keyward://-lead", ParseRefError::BadName),
            ("keyward://trail-", ParseRefError::BadName),
            ("keyward://has_underscore", ParseRefError::BadName),
        ];
        for (input, want) in cases {
            assert_eq!(input.parse::<Reference>(), Err(want), "input: {input}");
        }
    }

    #[test]
    fn old_two_segment_form_gets_its_own_error() {
        // People will try this — it was the format in an earlier draft, and it is
        // what every similar tool uses. The message has to say what to do instead.
        assert_eq!(
            "keyward://stripe/api_key".parse::<Reference>(),
            Err(ParseRefError::PathUnsupported)
        );
        assert_eq!(
            "keyward://stripe@4".parse::<Reference>(),
            Err(ParseRefError::VersionUnsupported)
        );
    }

    #[test]
    fn does_not_trim_whitespace() {
        assert!(" keyward://stripe".parse::<Reference>().is_err());
        assert!("keyward://stripe ".parse::<Reference>().is_err());
    }

    #[test]
    fn slugify_handles_what_people_actually_type() {
        let cases = [
            ("Stripe", "stripe"),
            ("OpenAI", "openai"),
            ("Production DB", "production-db"),
            ("  Stripe (test)  ", "stripe-test"),
            ("GitHub Token!!", "github-token"),
            ("Stripe 测试", "stripe"),
        ];
        for (input, want) in cases {
            match Reference::slugify(input) {
                Some(s) => assert_eq!(s.name(), want, "input: {input}"),
                None => unreachable!("`{input}` should slugify"),
            }
        }
    }

    #[test]
    fn slugify_refuses_rather_than_inventing() {
        // A name with no ASCII has no predictable slug. Transliterating or hashing
        // would give the user a reference they cannot recognise in their own .env,
        // so the UI asks them to type one instead.
        assert_eq!(Reference::slugify("生产数据库"), None);
        assert_eq!(Reference::slugify("🔑"), None);
        assert_eq!(Reference::slugify(""), None);
    }

    #[test]
    fn serde_round_trips_as_a_plain_string() {
        let re = r("keyward://stripe");
        let json = match serde_json::to_string(&re) {
            Ok(j) => j,
            Err(e) => unreachable!("serialize: {e}"),
        };
        assert_eq!(json, "\"keyward://stripe\"");
        match serde_json::from_str::<Reference>(&json) {
            Ok(back) => assert_eq!(back, re),
            Err(e) => unreachable!("deserialize: {e}"),
        }
    }

    #[test]
    fn scans_a_dotenv_file() {
        let env = "\
STRIPE_SECRET_KEY=keyward://stripe
OPENAI_API_KEY=kw://openai
DATABASE_URL=keyward://pg-prod
PUBLIC_API_BASE=https://api.example.com
";
        let refs: Vec<String> = find_all(env)
            .iter()
            .map(|f| f.reference.to_string())
            .collect();
        assert_eq!(
            refs,
            ["keyward://stripe", "keyward://openai", "keyward://pg-prod"]
        );
    }

    #[test]
    fn scan_spans_are_exact() {
        // `kw render` splices resolved values into these ranges.
        let env = "KEY=keyward://stripe\n";
        let found = find_all(env);
        let Some(f) = found.first() else {
            unreachable!("expected a match")
        };
        assert_eq!(env.get(f.start..f.end), Some("keyward://stripe"));
    }

    #[test]
    fn scan_ignores_near_misses() {
        let text = "xkw://a\nmy-keyward://a\nkeyward://BAD\nhttps://example.com/kw\n";
        assert_eq!(find_all(text), vec![]);
    }

    #[test]
    fn scan_stops_at_quotes_and_delimiters() {
        let text = r#"A="keyward://stripe", B='kw://openai';"#;
        let refs: Vec<String> = find_all(text)
            .iter()
            .map(|f| f.reference.to_string())
            .collect();
        assert_eq!(refs, ["keyward://stripe", "keyward://openai"]);
    }
}
