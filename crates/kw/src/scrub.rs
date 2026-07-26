//! Redacts known secret values from a child process's output
//! (ARCHITECTURE.md §10.3).
//!
//! This is the surface no amount of reference-rewriting closes: every file on
//! disk can be clean and a library still prints its config at startup, an HTTP
//! client still logs a header, a stack trace still carries a connection string.
//! All of it lands in the agent's context after the files were already safe.
//!
//! **Best-effort, and labelled as such.** It matches each value and a few common
//! encodings of it. A library that hashes a secret, or splits it across two log
//! lines, defeats this — and pretending otherwise would be worse than the honest
//! limit. The guarantee is brokering, where the child never holds the value at
//! all; scrubbing is what protects the credentials that cannot be brokered.

use std::io::{Read, Write};

/// The shortest run of a secret that is redacted on its own.
///
/// A program that "safely" logs a key prints the front of it — `sk-f2e295c9…` —
/// and a scrubber that only matches the whole value lets that straight through.
/// That is not a corner case; truncating before logging is the single most
/// common way a careless library reveals a credential, and it happened here on
/// the first real use.
///
/// Twelve characters. Secrets are random, so unrelated output sharing a
/// twelve-byte prefix with one is a coincidence worth roughly 2^36 — the floor
/// is not really about collisions. It is about low-entropy values, and
/// `Rule::expand` already drops anything under eight bytes before it gets here.
///
/// Lower is better up to that point: every character below the threshold is a
/// character of a real key that reaches the transcript.
const MIN_PREFIX: usize = 12;

/// One value to redact and the text to put in its place.
pub struct Rule {
    pub needle: Vec<u8>,
    pub replacement: Vec<u8>,
    /// Whether a leading run of `needle` counts as a match on its own.
    ///
    /// True for the value as issued. False for the encoded forms: a partial
    /// base64 or percent-encoded run is not something a program prints on
    /// purpose, and matching it would only add false positives.
    pub match_prefix: bool,
}

impl Rule {
    /// Build the rules for one secret: the value itself plus the encodings it is
    /// most likely to have been through before being printed.
    pub fn expand(value: &str, reference: &str) -> Vec<Rule> {
        let mut out = vec![Rule {
            needle: value.as_bytes().to_vec(),
            replacement: reference.as_bytes().to_vec(),
            match_prefix: true,
        }];

        // JSON-escaped: a value inside a logged request body.
        let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
        if escaped != value {
            out.push(Rule {
                needle: escaped.into_bytes(),
                replacement: reference.as_bytes().to_vec(),
                match_prefix: false,
            });
        }

        // URL-encoded: a value that travelled in a query string or form body.
        let encoded = percent_encode(value);
        if encoded != value {
            out.push(Rule {
                needle: encoded.into_bytes(),
                replacement: reference.as_bytes().to_vec(),
                match_prefix: false,
            });
        }

        // Base64: `Authorization: Basic`, and anything that logs a token blob.
        let b64 = base64(value.as_bytes());
        out.push(Rule {
            needle: b64.into_bytes(),
            replacement: reference.as_bytes().to_vec(),
            match_prefix: false,
        });

        // Values shorter than eight bytes are dropped: they collide with ordinary
        // words and would redact unrelated output, which is its own kind of
        // damage.
        out.retain(|r| r.needle.len() >= 8);
        out
    }
}

/// Copies `from` to `to`, redacting as it goes.
///
/// The buffer is carried across reads because a value split over two `write()`
/// calls by the child would otherwise pass through untouched — the failure mode
/// that makes a naive per-chunk scrubber worse than none, since it looks like it
/// is working.
pub fn pipe<R: Read, W: Write>(mut from: R, mut to: W, rules: &[Rule]) -> std::io::Result<()> {
    let mut pending: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 8192];

    loop {
        let n = from.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        pending.extend_from_slice(chunk.get(..n).unwrap_or_default());
        // `redact` decides for itself how much it can safely consume: anything
        // that might still be the start of a value is left for the next read.
        let (cleaned, consumed) = redact(&pending, rules, false);
        to.write_all(&cleaned)?;
        to.flush()?;
        pending = pending.get(consumed..).unwrap_or_default().to_vec();
    }

    let (cleaned, _) = redact(&pending, rules, true);
    to.write_all(&cleaned)?;
    to.flush()
}

/// Redact what is certain, and report how much of `haystack` was consumed.
///
/// When `at_end` is false, a run that reaches the end of the buffer is *not*
/// judged: it may be the first half of a value the child is still writing. Those
/// bytes stay unconsumed and come back with the next read. Deciding early is the
/// bug this signature exists to prevent — a partial match would be replaced, and
/// the rest of the value would then arrive and be printed in the clear.
fn redact(haystack: &[u8], rules: &[Rule], at_end: bool) -> (Vec<u8>, usize) {
    let mut out = Vec::with_capacity(haystack.len());
    let mut i = 0;
    'outer: while i < haystack.len() {
        let Some(tail) = haystack.get(i..) else { break };

        // A whole value wins over any partial reading of another.
        for rule in rules {
            if tail.starts_with(&rule.needle) {
                out.extend_from_slice(&rule.replacement);
                i += rule.needle.len();
                continue 'outer;
            }
        }

        // Might this be a value still arriving?
        if !at_end
            && rules
                .iter()
                .any(|r| common_prefix(tail, &r.needle) == tail.len())
        {
            return (out, i);
        }

        // A truncated value: the front of a key that a program printed to be
        // "safe". The longest run wins.
        for rule in rules {
            if rule.match_prefix && rule.needle.len() > MIN_PREFIX {
                let run = common_prefix(tail, &rule.needle);
                if run >= MIN_PREFIX {
                    out.extend_from_slice(&rule.replacement);
                    i += run;
                    continue 'outer;
                }
            }
        }

        if let Some(byte) = tail.first() {
            out.push(*byte);
        }
        i += 1;
    }
    (out, i)
}

/// How many leading bytes the two share.
fn common_prefix(a: &[u8], b: &[u8]) -> usize {
    a.iter().zip(b).take_while(|(x, y)| x == y).count()
}

fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn base64(input: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for group in input.chunks(3) {
        let b = [
            group.first().copied().unwrap_or(0),
            group.get(1).copied().unwrap_or(0),
            group.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        let idx = [(n >> 18) & 63, (n >> 12) & 63, (n >> 6) & 63, n & 63];
        for (position, value) in idx.iter().enumerate() {
            match ALPHABET.get(*value as usize) {
                Some(c) if position <= group.len() => out.push(char::from(*c)),
                _ => out.push('='),
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules() -> Vec<Rule> {
        Rule::expand("sk_live_51H8xQ2eZvKYlo2C9", "keyward://stripe")
    }

    #[test]
    fn redacts_a_value_in_the_middle_of_a_line() {
        let (out, _) = redact(b"[stripe] key=sk_live_51H8xQ2eZvKYlo2C9 ready", &rules(), true);
        assert_eq!(
            String::from_utf8_lossy(&out),
            "[stripe] key=keyward://stripe ready"
        );
    }

    #[test]
    fn redacts_across_a_chunk_boundary() {
        // The failure this whole design exists to avoid: a naive per-write
        // scrubber passes this through and looks like it is working.
        let mut sink = Vec::new();
        let input = b"prefix sk_live_51H8xQ2eZvKYlo2C9 suffix".to_vec();
        pipe(Chunked::new(input, 7), &mut sink, &rules()).unwrap_or_default();
        assert_eq!(
            String::from_utf8_lossy(&sink),
            "prefix keyward://stripe suffix"
        );
    }

    #[test]
    fn redacts_url_encoded_and_base64_forms() {
        let encoded = percent_encode("sk_live_51H8xQ2eZvKYlo2C9");
        let (out, _) = redact(format!("?token={encoded}").as_bytes(), &rules(), true);
        assert!(!String::from_utf8_lossy(&out).contains("51H8x"));

        let b64 = base64(b"sk_live_51H8xQ2eZvKYlo2C9");
        let (out, _) = redact(format!("Basic {b64}").as_bytes(), &rules(), true);
        assert!(!String::from_utf8_lossy(&out).contains(&b64));
    }

    #[test]
    fn leaves_unrelated_output_alone() {
        let (out, _) = redact(b"listening on http://localhost:3000", &rules(), true);
        assert_eq!(
            String::from_utf8_lossy(&out),
            "listening on http://localhost:3000"
        );
    }

    #[test]
    fn refuses_to_match_on_short_values() {
        // "abc" would appear in unrelated output constantly. Redacting it would
        // damage the logs it was meant to protect.
        assert!(Rule::expand("abc", "keyward://x").is_empty());
    }

    /// A reader that hands out `size` bytes at a time, to force the boundary case.
    struct Chunked {
        data: Vec<u8>,
        size: usize,
        at: usize,
    }

    impl Chunked {
        fn new(data: Vec<u8>, size: usize) -> Self {
            Self { data, size, at: 0 }
        }
    }

    impl Read for Chunked {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let end = (self.at + self.size).min(self.data.len());
            let n = end - self.at;
            if n == 0 {
                return Ok(0);
            }
            let Some(src) = self.data.get(self.at..end) else {
                return Ok(0);
            };
            let Some(dst) = buf.get_mut(..n) else {
                return Ok(0);
            };
            dst.copy_from_slice(src);
            self.at = end;
            Ok(n)
        }
    }

    #[test]
    fn a_truncated_value_is_redacted_too() {
        // What a program prints when it thinks it is being careful, and what
        // Codex printed on the first real use of Keyward: the front of the key.
        let (out, _) = redact(b"Key: sk_live_51H8xQ2eZ...", &rules(), true);
        assert_eq!(
            String::from_utf8_lossy(&out),
            "Key: keyward://stripe...",
            "the front of a key is still the key"
        );
    }

    #[test]
    fn a_short_run_is_left_alone() {
        // Below the threshold this is ordinary output, and redacting it would
        // corrupt text that has nothing to do with the secret.
        let (out, _) = redact(b"prefix sk_live here", &rules(), true);
        assert_eq!(String::from_utf8_lossy(&out), "prefix sk_live here");
    }

    #[test]
    fn a_partial_run_at_a_chunk_boundary_is_not_judged_early() {
        // The dangerous shape: half a value arrives, gets replaced as a
        // "truncated" one, and the other half is then printed in the clear.
        // Seventeen characters of `sk_live_51H8xQ2eZvKYlo2C9`, ending exactly at
        // the buffer's edge: long enough to look like a truncated value, but the
        // rest of it may be one `write()` away.
        let (out, consumed) = redact(b"x sk_live_51H8xQ2eZ", &rules(), false);
        assert_eq!(String::from_utf8_lossy(&out), "x ");
        assert_eq!(consumed, 2, "the value must wait for the rest of itself");

        // At the end of the stream there is nothing more coming, so the same
        // bytes are judged — and redacted.
        let (out, _) = redact(b"x sk_live_51H8xQ2eZ", &rules(), true);
        assert_eq!(String::from_utf8_lossy(&out), "x keyward://stripe");
    }
}
