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

/// One value to redact and the text to put in its place.
pub struct Rule {
    pub needle: Vec<u8>,
    pub replacement: Vec<u8>,
}

impl Rule {
    /// Build the rules for one secret: the value itself plus the encodings it is
    /// most likely to have been through before being printed.
    pub fn expand(value: &str, reference: &str) -> Vec<Rule> {
        let mut out = vec![Rule {
            needle: value.as_bytes().to_vec(),
            replacement: reference.as_bytes().to_vec(),
        }];

        // JSON-escaped: a value inside a logged request body.
        let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
        if escaped != value {
            out.push(Rule {
                needle: escaped.into_bytes(),
                replacement: reference.as_bytes().to_vec(),
            });
        }

        // URL-encoded: a value that travelled in a query string or form body.
        let encoded = percent_encode(value);
        if encoded != value {
            out.push(Rule {
                needle: encoded.into_bytes(),
                replacement: reference.as_bytes().to_vec(),
            });
        }

        // Base64: `Authorization: Basic`, and anything that logs a token blob.
        let b64 = base64(value.as_bytes());
        out.push(Rule {
            needle: b64.into_bytes(),
            replacement: reference.as_bytes().to_vec(),
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
    let longest = rules.iter().map(|r| r.needle.len()).max().unwrap_or(0);
    let mut pending: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 8192];

    loop {
        let n = from.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        pending.extend_from_slice(chunk.get(..n).unwrap_or_default());
        let cleaned = redact(&pending, rules);

        // Hold back the last `longest - 1` bytes: a needle could still be
        // completing across the boundary.
        let keep = longest.saturating_sub(1).min(cleaned.len());
        let flush_to = cleaned.len() - keep;
        to.write_all(cleaned.get(..flush_to).unwrap_or_default())?;
        to.flush()?;
        pending = cleaned.get(flush_to..).unwrap_or_default().to_vec();
    }

    let cleaned = redact(&pending, rules);
    to.write_all(&cleaned)?;
    to.flush()
}

fn redact(haystack: &[u8], rules: &[Rule]) -> Vec<u8> {
    let mut out = Vec::with_capacity(haystack.len());
    let mut i = 0;
    'outer: while i < haystack.len() {
        let Some(tail) = haystack.get(i..) else { break };
        for rule in rules {
            if tail.starts_with(&rule.needle) {
                out.extend_from_slice(&rule.replacement);
                i += rule.needle.len();
                continue 'outer;
            }
        }
        if let Some(byte) = tail.first() {
            out.push(*byte);
        }
        i += 1;
    }
    out
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
        let out = redact(b"[stripe] key=sk_live_51H8xQ2eZvKYlo2C9 ready", &rules());
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
        let out = redact(format!("?token={encoded}").as_bytes(), &rules());
        assert!(!String::from_utf8_lossy(&out).contains("51H8x"));

        let b64 = base64(b"sk_live_51H8xQ2eZvKYlo2C9");
        let out = redact(format!("Basic {b64}").as_bytes(), &rules());
        assert!(!String::from_utf8_lossy(&out).contains(&b64));
    }

    #[test]
    fn leaves_unrelated_output_alone() {
        let out = redact(b"listening on http://localhost:3000", &rules());
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
}
