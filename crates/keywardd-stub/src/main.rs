//! A fake `keywardd`.
//!
//! Speaks the ARCHITECTURE.md §7 protocol — newline-delimited JSON over a Unix
//! socket at the real daemon's path — and answers from canned data. It holds no
//! secrets, touches no keychain, and opens no network sockets.
//!
//! It exists so the macOS and Windows apps can be built against the real wire
//! format before the real daemon does. That is the whole point of §2.1's decision
//! to make the cross-language contract a line of JSON rather than an FFI boundary:
//! the thing on the other end can be two hundred lines of anything.
//!
//! ```console
//! $ cargo run -p keywardd-stub
//! keywardd-stub listening on /Users/you/Library/Application Support/Keyward/keywardd.sock
//! ```

// A stub's failure modes are "it stopped" — panicking with a legible message is
// the right behaviour here, unlike in the real daemon where a panic on a request
// path denies service to every other session.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use std::{fs, thread};

use serde_json::{Value, json};

fn socket_path() -> PathBuf {
    let home = std::env::var("HOME").expect("HOME must be set");
    PathBuf::from(home)
        .join("Library/Application Support/Keyward")
        .join("keywardd.sock")
}

fn main() -> std::io::Result<()> {
    let path = socket_path();
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    // A socket file left behind by a previous run would make bind() fail with
    // EADDRINUSE even though nothing is listening.
    let _ = fs::remove_file(&path);

    let listener = UnixListener::bind(&path)?;
    println!("keywardd-stub listening on {}", path.display());
    println!("stop with ^C; the socket is removed on the next start");

    for stream in listener.incoming() {
        match stream {
            Ok(s) => {
                thread::spawn(move || {
                    if let Err(e) = serve(s) {
                        eprintln!("connection ended: {e}");
                    }
                });
            }
            Err(e) => eprintln!("accept failed: {e}"),
        }
    }
    Ok(())
}

fn serve(stream: UnixStream) -> std::io::Result<()> {
    let mut out = stream.try_clone()?;
    let reader = BufReader::new(stream);

    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let response = match serde_json::from_str::<Value>(&line) {
            Ok(req) => handle(&req),
            Err(e) => json!({"id": Value::Null, "ok": false,
                             "error": {"code": "bad_request", "message": e.to_string()}}),
        };
        writeln!(out, "{response}")?;
        out.flush()?;
    }
    Ok(())
}

fn handle(req: &Value) -> Value {
    let id = req.get("id").cloned().unwrap_or(Value::Null);
    let method = req.get("method").and_then(Value::as_str).unwrap_or("");

    let result = match method {
        "daemon.status" => Ok(json!({
            "version": env!("CARGO_PKG_VERSION"),
            "stub": true,
            "secrets": 5
        })),
        "vault.list" => Ok(json!({ "secrets": secrets() })),
        "uses.list" => Ok(json!({ "uses": uses(req) })),
        // Writes are accepted and discarded — enough for the add screen to close
        // and the list to look right, which is all the UI needs to be built.
        "vault.add" | "vault.rotate" | "vault.remove" | "vault.set_approval" => {
            Ok(json!({ "ok": true }))
        }
        // Deliberately not implemented. These move plaintext, and a stub that
        // pretends to broker would let a bug in the app go unnoticed until the
        // real daemon replaced it.
        "secret.hand" | "broker.open" | "scrub.values" => Err((
            "not_implemented",
            "the stub daemon does not move secret values",
        )),
        "" => Err(("bad_request", "missing `method`")),
        _ => Err(("not_found", "unknown method")),
    };

    match result {
        Ok(r) => json!({"id": id, "ok": true, "result": r}),
        Err((code, message)) => {
            json!({"id": id, "ok": false, "error": {"code": code, "message": message}})
        }
    }
}

/// An RFC 3339 timestamp `minutes_ago` before now.
///
/// Canned data has to be relative to the clock, not frozen. Fixed timestamps made
/// every row in the app read "in 6 hr." — a future tense that looks like a bug in
/// the formatter rather than in the fixture.
fn ago(minutes_ago: i64) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    iso8601(now - minutes_ago * 60)
}

/// Seconds since the epoch to `YYYY-MM-DDTHH:MM:SSZ`.
///
/// Hand-rolled rather than pulling in a date crate: this is a development stub,
/// and Howard Hinnant's civil-from-days is twelve lines.
fn iso8601(epoch: i64) -> String {
    let days = epoch.div_euclid(86_400);
    let secs = epoch.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        y,
        m,
        d,
        secs / 3600,
        (secs % 3600) / 60,
        secs % 60
    )
}

/// The list from the design mock, so the app and the mock can be compared directly.
fn secrets() -> Value {
    json!([
        {
            "name": "stripe", "display": "Stripe", "ref": "keyward://stripe",
            "masked": "sk_live_…4f2a", "status": "protected", "letter": "S",
            "tint": "#5B51E8", "logo": "StripeLogo",
            "last_use": {"at": ago(2), "actor": "codex", "project": "my-shop"}
        },
        {
            "name": "openai", "display": "OpenAI", "ref": "keyward://openai",
            "masked": "sk-proj-…9c1b", "status": "protected", "letter": "O",
            "tint": "#0E9C74", "logo": "OpenAILogo",
            "last_use": {"at": ago(12), "actor": "claude", "project": "my-shop"}
        },
        {
            "name": "pg-prod", "display": "生产数据库", "ref": "keyward://pg-prod",
            "masked": "postgres://…", "status": "shared", "letter": "P",
            "tint": "#2F6491", "logo": "PostgresLogo",
            "last_use": {"at": ago(64), "actor": "psql", "project": "my-shop"}
        },
        {
            "name": "resend", "display": "Resend", "ref": "keyward://resend",
            "masked": "re_…7a2e", "status": "unused", "letter": "R",
            "tint": "#DC4C2B", "logo": "ResendLogo",
            "last_use": Value::Null
        },
        {
            "name": "github", "display": "GitHub 令牌", "ref": "keyward://github",
            "masked": "ghp_…d13c", "status": "protected", "letter": "G",
            "tint": "#7C51E8", "logo": "GitHubLogo",
            "last_use": {"at": ago(1_300), "actor": "gh", "project": "landing-page"}
        }
    ])
}

fn uses(req: &Value) -> Value {
    let name = req
        .get("params")
        .and_then(|p| p.get("name"))
        .and_then(Value::as_str)
        .unwrap_or("stripe");
    // Spread across the fortnight so the activity strip has something to show.
    let entries: [(i64, &str, &str); 9] = [
        (2, "codex", "broker"),
        (14, "codex", "broker"),
        (95, "claude", "broker"),
        (320, "npm run dev", "broker"),
        (1_300, "stripe-mcp", "broker"),
        (2_900, "codex", "broker"),
        (4_400, "psql", "injection"),
        (7_100, "codex", "broker"),
        (11_800, "claude", "broker"),
    ];
    Value::Array(
        entries
            .iter()
            .map(|(mins, actor, tier)| {
                json!({"at": ago(*mins), "secret": name, "actor": actor,
                       "project": "my-shop", "tier": tier, "allowed": true})
            })
            .collect(),
    )
}
