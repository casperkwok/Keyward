//! `keywardd` — the only process that holds plaintext.
//!
//! Speaks the ARCHITECTURE.md §7 protocol over a Unix socket. Compared with
//! `keywardd-stub` it does three things for real: it stores values in the OS
//! keychain, it persists metadata to `vault.json`, and it appends to the usage
//! log that powers the "who used it" screen.
//!
//! Every connection is attested (§7) and every entry in the usage log carries the
//! identity the kernel reported, not the one the caller typed.

mod approval;
mod broker;
mod peer;
mod store;

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use std::{fs, thread};

use keyward_core::{Approval, Caller, Decision, Delivery, Reference, Secret, Tier, Use, evaluate};
use serde_json::{Value, json};

use crate::approval::{Approvals, Outcome};
use crate::broker::{Broker, Opened, Served};
use crate::store::{SecretStore, StoreError};

fn support_dir() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    Some(PathBuf::from(home).join("Library/Application Support/Keyward"))
}

fn main() -> std::io::Result<()> {
    let Some(dir) = support_dir() else {
        eprintln!("HOME is not set; cannot locate the vault");
        std::process::exit(1);
    };

    let store = match SecretStore::open(dir.clone()) {
        Ok(s) => s,
        Err(e) => {
            // Refuse to start rather than presenting an empty vault over a
            // metadata file that exists but could not be parsed.
            eprintln!("cannot open the vault: {e}");
            std::process::exit(1);
        }
    };
    let store = Arc::new(Mutex::new(store));

    // The broker records a use for every request it forwards, so the "who used
    // it" screen fills in from real traffic rather than from launches.
    let logging = Arc::clone(&store);
    let broker = match Broker::start(move |served: Served| {
        if let Ok(store) = logging.lock() {
            store.record(&Use {
                at: now_iso8601(),
                secret: served.secret,
                actor: served.actor,
                project: served.project,
                caller: served.caller,
                tier: Tier::Broker,
                allowed: true,
            });
        }
    }) {
        Ok(b) => b,
        Err(e) => {
            // Refuse to start rather than silently downgrading every brokered
            // secret to being handed over.
            eprintln!("cannot bind the broker: {e}");
            std::process::exit(1);
        }
    };

    let socket = dir.join("keywardd.sock");
    let _ = fs::remove_file(&socket);
    let listener = UnixListener::bind(&socket)?;
    restrict(&socket);

    println!("keywardd listening on {}", socket.display());
    println!("broker on http://127.0.0.1:{}", broker.port());
    println!("{} secrets", store_len(&store));

    let approvals = Approvals::new();

    for stream in listener.incoming() {
        match stream {
            Ok(s) => {
                let store = Arc::clone(&store);
                let broker = broker.clone();
                let approvals = approvals.clone();
                thread::spawn(move || {
                    if let Err(e) = serve(s, store, broker, approvals) {
                        eprintln!("connection ended: {e}");
                    }
                });
            }
            Err(e) => eprintln!("accept failed: {e}"),
        }
    }
    Ok(())
}

fn store_len(store: &Arc<Mutex<SecretStore>>) -> usize {
    store
        .lock()
        .map(|s| s.vault().secrets.len())
        .unwrap_or_default()
}

fn restrict(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
}

fn serve(
    stream: UnixStream,
    store: Arc<Mutex<SecretStore>>,
    broker: Broker,
    approvals: Approvals,
) -> std::io::Result<()> {
    // Attested once, on accept. A pid is stable for the life of a connection, and
    // resolving it per request would be two syscalls spent re-learning the same
    // answer — while opening a window where the peer's pid could be recycled
    // between the check and the request it was meant to authorise.
    let caller = peer::attest(&stream);

    let mut out = stream.try_clone()?;
    let reader = BufReader::new(stream);

    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let response = match serde_json::from_str::<Value>(&line) {
            Ok(request) => handle(&request, &store, &broker, &approvals, &caller),
            Err(e) => fail(Value::Null, "bad_request", &e.to_string()),
        };
        writeln!(out, "{response}")?;
        out.flush()?;
    }
    Ok(())
}

fn ok(id: Value, result: Value) -> Value {
    json!({"id": id, "ok": true, "result": result})
}

fn fail(id: Value, code: &str, message: &str) -> Value {
    json!({"id": id, "ok": false, "error": {"code": code, "message": message}})
}

fn handle(
    request: &Value,
    store: &Arc<Mutex<SecretStore>>,
    broker: &Broker,
    approvals: &Approvals,
    caller: &Caller,
) -> Value {
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let method = request.get("method").and_then(Value::as_str).unwrap_or("");
    let params = request.get("params");

    // These three are handled before the vault is locked, and must stay that way.
    // `secret.hand` can park for a minute waiting for a human, and the human's
    // answer arrives on another connection as `approval.resolve` — which would
    // then be waiting for the lock held by the request it is trying to release.
    // Locking here would deadlock the daemon on its own approval prompt.
    match method {
        "secret.hand" => return hand(id, params, store, approvals, caller),
        "approval.pending" => {
            let pending: Vec<Value> = approvals
                .pending()
                .into_iter()
                .map(|p| {
                    json!({
                        "id": p.id,
                        "secret": p.secret,
                        "caller": p.caller,
                        "actor": p.actor,
                        "pid": p.pid,
                        "purpose": p.purpose,
                        "project": p.project,
                        "expires_in": p.expires_in,
                    })
                })
                .collect();
            return ok(id, json!({ "pending": pending }));
        }
        "approval.resolve" => {
            let (Some(prompt), Some(decision)) =
                (str_param(params, "id"), str_param(params, "decision"))
            else {
                return fail(id, "bad_request", "`id` and `decision` are required");
            };
            let Some(outcome) = Outcome::parse(&decision) else {
                return fail(
                    id,
                    "bad_request",
                    "`decision` must be `allow_once`, `allow_caller` or `deny`",
                );
            };
            return ok(
                id,
                json!({ "resolved": approvals.resolve(&prompt, outcome) }),
            );
        }
        _ => {}
    }

    let Ok(mut store) = store.lock() else {
        return fail(
            id,
            "io_error",
            "the vault lock was poisoned; restart Keyward",
        );
    };

    match method {
        "daemon.status" => ok(
            id,
            json!({
                "version": env!("CARGO_PKG_VERSION"),
                "stub": false,
                "secrets": store.vault().secrets.len(),
                "broker_port": broker.port(),
                "broker_sessions": broker.open_sessions()
            }),
        ),

        // Asked for by an app that has just been updated and found an older
        // daemon still answering the socket (see `Updater.swift`). Without it the
        // new app adopts the old daemon — the one inside the bundle the updater
        // just replaced — and the process holding the user's secrets silently
        // stays on the previous version.
        //
        // Not an authorisation hole: any process running as this user can already
        // send this one a signal. What it adds is a way to stop *cleanly*, which a
        // signal is not.
        "daemon.quit" => {
            // Taken under the lock, so no vault write is in flight at this
            // moment. A write that starts during the delay below still completes
            // or never begins — every write is a temp file plus a rename.
            drop(store);
            thread::spawn(|| {
                thread::sleep(std::time::Duration::from_millis(150));
                if let Some(dir) = support_dir() {
                    let _ = fs::remove_file(dir.join("keywardd.sock"));
                }
                std::process::exit(0);
            });
            return ok(id, json!({ "quitting": true }));
        }

        "vault.list" => {
            let recent = store.uses(None, 500);
            let live = broker.sessions();
            let secrets: Vec<Value> = store
                .vault()
                .secrets
                .values()
                .map(|s| {
                    let mut row = summarise(s, &recent);
                    if let Some(session) = live.iter().find(|l| l.secret == s.name)
                        && let Some(object) = row.as_object_mut()
                    {
                        object.insert(
                            "live".into(),
                            json!({
                                "session": session.token,
                                "requests": session.requests,
                                "opened_at": session.opened_at,
                                "actor": session.actor,
                            }),
                        );
                    }
                    row
                })
                .collect();
            ok(id, json!({ "secrets": secrets }))
        }

        "uses.list" => {
            let name = params.and_then(|p| p.get("name")).and_then(Value::as_str);
            let limit = params
                .and_then(|p| p.get("limit"))
                .and_then(Value::as_u64)
                .unwrap_or(50) as usize;
            ok(id, json!({ "uses": store.uses(name, limit) }))
        }

        "vault.add" => {
            let (Some(name), Some(value)) = (str_param(params, "name"), str_param(params, "value"))
            else {
                return fail(id, "bad_request", "`name` and `value` are required");
            };
            // Validate through the same grammar the CLI and `.env` files use, so a
            // name that cannot be referenced can never enter the vault.
            if let Err(e) = Reference::new(name.clone()) {
                return fail(id, "bad_request", &e.to_string());
            }
            let display = str_param(params, "display").unwrap_or_else(|| name.clone());
            let delivery = delivery_param(params);
            // Report the mode back. The caller cannot infer it from `vault.list`:
            // `status` is `unused` until the secret has been used, which hides the
            // delivery behind an unrelated fact — and a CLI that then says
            // "forwarded" about a handed secret is exactly the misunderstanding
            // DESIGN.md §4 makes Keyward responsible for preventing.
            let kind = if delivery.is_brokered() { "brokered" } else { "handed" };
            match store.add(name, display, delivery, value) {
                Ok(()) => ok(id, json!({"ok": true, "delivery": kind})),
                Err(e) => fail(id, e.code(), &e.to_string()),
            }
        }

        "vault.rotate" => {
            let (Some(name), Some(value)) = (str_param(params, "name"), str_param(params, "value"))
            else {
                return fail(id, "bad_request", "`name` and `value` are required");
            };
            match store.rotate(&name, value) {
                Ok(()) => ok(id, json!({"ok": true})),
                Err(e) => fail(id, e.code(), &e.to_string()),
            }
        }

        "vault.set_display" => {
            let (Some(name), Some(display)) =
                (str_param(params, "name"), str_param(params, "display"))
            else {
                return fail(id, "bad_request", "`name` and `display` are required");
            };
            match store.set_display(&name, display) {
                Ok(()) => ok(id, json!({"ok": true})),
                Err(e) => fail(id, e.code(), &e.to_string()),
            }
        }

        // The switch behind the approval prompt (§7). Its only caller is a human
        // in the settings pane, which is why it is a write to the vault and not a
        // parameter on `secret.hand`.
        "vault.set_approval" => {
            let (Some(name), Some(approval)) =
                (str_param(params, "name"), str_param(params, "approval"))
            else {
                return fail(id, "bad_request", "`name` and `approval` are required");
            };
            let approval = match approval.as_str() {
                "ask" => Approval::Ask,
                "never" => Approval::Never,
                _ => return fail(id, "bad_request", "`approval` must be `ask` or `never`"),
            };
            match store.set_approval(&name, approval) {
                Ok(()) => ok(id, json!({"ok": true})),
                Err(e) => fail(id, e.code(), &e.to_string()),
            }
        }

        "vault.remove" => match str_param(params, "name") {
            Some(name) => match store.remove(&name) {
                Ok(()) => ok(id, json!({"ok": true})),
                Err(e) => fail(id, e.code(), &e.to_string()),
            },
            None => fail(id, "bad_request", "`name` is required"),
        },

        // Development affordance: record a use so the "who used it" screen has
        // something real to show before the broker exists.
        "uses.record" => {
            let Some(name) = str_param(params, "name") else {
                return fail(id, "bad_request", "`name` is required");
            };
            if store.vault().get(&name).is_none() {
                return fail(id, "not_found", &StoreError::NotFound(name).to_string());
            }
            let record = Use {
                at: now_iso8601(),
                secret: name,
                actor: str_param(params, "actor").unwrap_or_else(|| caller.actor()),
                project: str_param(params, "project"),
                caller: Some(caller.describe()),
                tier: keyward_core::Tier::Broker,
                allowed: true,
            };
            store.record(&record);
            ok(id, json!({"ok": true}))
        }

        // The strongest tier: the child gets a loopback URL and a session token,
        // and the value stays in this process (ARCHITECTURE.md §6.1).
        "broker.open" => {
            let Some(name) = str_param(params, "name") else {
                return fail(id, "bad_request", "`name` is required");
            };
            let Some(secret) = store.vault().get(&name) else {
                return fail(id, "not_found", &format!("no secret named `{name}`"));
            };
            // Always answer with a variable name, conventional or derived, so no
            // caller is left holding a session token with nowhere to send it.
            let base_url_env = secret.delivery.base_url_variable(&name);
            let Delivery::Brokered { upstream, .. } = secret.delivery.clone() else {
                return fail(
                    id,
                    "denied",
                    &format!("`{name}` is not an HTTP credential, so it cannot be forwarded"),
                );
            };
            let value = match store.reveal(&name) {
                Ok(v) => v,
                Err(e) => return fail(id, e.code(), &e.to_string()),
            };
            let ttl_secs = params
                .and_then(|p| p.get("ttl_secs"))
                .and_then(Value::as_u64)
                .unwrap_or(broker::DEFAULT_TTL_SECS);
            let opened = Opened {
                ttl_secs,
                owner: Some(caller.actor()),
                owner_label: Some(caller.describe()),
                project: str_param(params, "project"),
            };
            let Some(session) = broker.open(&name, &upstream, value, opened) else {
                return fail(id, "io_error", "the broker refused a new session");
            };
            ok(
                id,
                json!({
                    "session": session.token,
                    "base_url": format!("http://127.0.0.1:{}", broker.port()),
                    "token": session.token,
                    "base_url_env": base_url_env,
                    // Reported so `kw` and the GUI can say when protection lapses
                    // rather than letting a session die under a running child
                    // with no explanation.
                    "expires_at": session.expires_at,
                    "ttl_secs": session.expires_at.saturating_sub(session.opened_at),
                }),
            )
        }

        "broker.sessions" => {
            let sessions: Vec<Value> = broker
                .sessions()
                .iter()
                .map(|s| {
                    json!({
                        "session": s.token,
                        "secret": s.secret,
                        "opened_at": s.opened_at,
                        "expires_at": s.expires_at,
                        "requests": s.requests,
                        "actor": s.actor,
                    })
                })
                .collect();
            ok(id, json!({ "sessions": sessions }))
        }

        "broker.close" => match str_param(params, "session") {
            Some(token) => ok(id, json!({ "closed": broker.close(&token) })),
            None => fail(id, "bad_request", "`session` is required"),
        },

        "scrub.values" => fail(
            id,
            "not_implemented",
            "not needed until brokering is optional",
        ),

        "" => fail(id, "bad_request", "missing `method`"),
        _ => fail(id, "not_found", "unknown method"),
    }
}

/// `secret.hand` — the method that moves plaintext out of this process, and so the
/// one that runs the policy check for real (ARCHITECTURE.md §4, §6.2).
///
/// Split out of [`handle`] because it is the only request that can wait on a
/// person, and the vault lock must be released across that wait.
fn hand(
    id: Value,
    params: Option<&Value>,
    store: &Arc<Mutex<SecretStore>>,
    approvals: &Approvals,
    caller: &Caller,
) -> Value {
    let names: Vec<String> = params
        .and_then(|p| p.get("names"))
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    if names.is_empty() {
        return fail(id, "bad_request", "`names` is required");
    }
    // `actor` is what the caller says it is running; `caller` is what the kernel
    // says it is. The log keeps both, and the fallback is the attested one,
    // because an absent claim should not become "unknown" when we know.
    let purpose = str_param(params, "purpose").or_else(|| str_param(params, "actor"));
    let actor = str_param(params, "actor").unwrap_or_else(|| caller.actor());
    let project = str_param(params, "project");
    let attested = caller.describe();

    // Phase one: decide, holding the lock only long enough to read policy.
    let decisions = {
        let Ok(store) = store.lock() else {
            return fail(
                id,
                "io_error",
                "the vault lock was poisoned; restart Keyward",
            );
        };
        let mut decisions = Vec::with_capacity(names.len());
        for name in &names {
            let Some(secret) = store.vault().get(name) else {
                return fail(id, "not_found", &format!("no secret named `{name}`"));
            };
            match evaluate(Tier::Injection, secret.tier(), secret.approval) {
                Decision::Allow => decisions.push((name.clone(), false)),
                // A brokered secret can never be handed over. This is the
                // product's headline guarantee, and this branch is where it is
                // actually enforced rather than merely described.
                Decision::Denied(d) => return fail(id, "denied", &format!("`{name}`: {d}")),
                Decision::NeedsApproval => decisions.push((name.clone(), true)),
            }
        }
        decisions
    };

    // Phase two: ask, with no lock held. Every other connection — including the
    // GUI's `approval.resolve` — keeps working while this thread sleeps.
    for (name, needs_approval) in &decisions {
        if !needs_approval {
            continue;
        }
        let outcome = approvals.ask(name, caller, purpose.as_deref(), project.as_deref());
        if outcome.allowed() {
            continue;
        }
        // A refusal is a use. It is the entry that tells a user something asked
        // for their production database at 3am and did not get it, which is the
        // one the "who used it" screen exists for.
        if let Ok(store) = store.lock() {
            store.record(&Use {
                at: now_iso8601(),
                secret: name.clone(),
                actor: actor.clone(),
                project: project.clone(),
                caller: Some(attested.clone()),
                tier: Tier::Injection,
                allowed: false,
            });
        }
        let code = outcome.code().unwrap_or("approval_denied");
        return fail(id, code, &outcome.message(name));
    }

    // Phase three: disclose.
    let mut values = serde_json::Map::new();
    let Ok(store) = store.lock() else {
        return fail(
            id,
            "io_error",
            "the vault lock was poisoned; restart Keyward",
        );
    };
    for (name, _) in &decisions {
        match store.reveal(name) {
            Ok(value) => {
                values.insert(name.clone(), Value::String(value));
            }
            Err(e) => return fail(id, e.code(), &e.to_string()),
        }
    }

    // Record only after every value resolved, so a partial failure does not leave
    // the log claiming a use that never happened.
    for (name, _) in &decisions {
        store.record(&Use {
            at: now_iso8601(),
            secret: name.clone(),
            actor: actor.clone(),
            project: project.clone(),
            caller: Some(attested.clone()),
            tier: Tier::Injection,
            allowed: true,
        });
    }
    ok(id, json!({ "values": Value::Object(values) }))
}

fn str_param(params: Option<&Value>, key: &str) -> Option<String> {
    params
        .and_then(|p| p.get(key))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

/// Upstream and base-URL variable for services we know how to stand in front of.
///
/// This table is why the product's strongest mode is reachable at all. Without it
/// `vault.add` had no `delivery` to work from and fell through to `Handed`, so
/// every secret a user created was handed over — the broker existed and nothing
/// could ever use it. That is the failure DESIGN.md §4 is written against: a
/// secret ends up in the weaker mode while the stronger one was available, and
/// nobody chose it.
///
/// `base_url_env` is only filled in where the SDK genuinely reads that variable.
/// Inventing one would hand the child a token its client never uses, which fails
/// against the real host in a way the user cannot diagnose — worse than admitting
/// we do not know, which is what `kw exec` prints when this is `None`.
fn service_for(name: &str) -> Option<(&'static str, Option<&'static str>)> {
    const SERVICES: &[(&str, &str, Option<&str>)] = &[
        ("openai", "https://api.openai.com", Some("OPENAI_BASE_URL")),
        ("anthropic", "https://api.anthropic.com", Some("ANTHROPIC_BASE_URL")),
        ("deepseek", "https://api.deepseek.com", None),
        ("moonshot", "https://api.moonshot.cn", None),
        ("kimi", "https://api.moonshot.cn", None),
        ("groq", "https://api.groq.com", None),
        ("stripe", "https://api.stripe.com", None),
        ("resend", "https://api.resend.com", None),
        ("github", "https://api.github.com", None),
        ("gitlab", "https://gitlab.com", None),
        ("cloudflare", "https://api.cloudflare.com", None),
        ("sentry", "https://sentry.io", None),
        ("posthog", "https://app.posthog.com", None),
        ("linear", "https://api.linear.app", None),
        ("notion", "https://api.notion.com", None),
        ("shopify", "https://api.shopify.com", None),
        ("algolia", "https://algolia.net", None),
        ("cloudinary", "https://api.cloudinary.com", None),
        ("replicate", "https://api.replicate.com", None),
        ("huggingface", "https://api-inference.huggingface.co", Some("HF_ENDPOINT")),
    ];
    // Longest match, so `openai-prod` finds `openai` and a name that happens to
    // start with a shorter entry does not win over a more specific one.
    SERVICES
        .iter()
        .filter(|(key, _, _)| name == *key || name.starts_with(&format!("{key}-")))
        .max_by_key(|(key, _, _)| key.len())
        .map(|(_, upstream, var)| (*upstream, *var))
}

fn delivery_param(params: Option<&Value>) -> Delivery {
    let upstream = params
        .and_then(|p| p.get("delivery"))
        .and_then(|d| d.get("upstream"))
        .and_then(Value::as_str);
    let base_url_env = params
        .and_then(|p| p.get("delivery"))
        .and_then(|d| d.get("base_url_env"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    if let Some(u) = upstream
        && !u.is_empty()
    {
        // An explicit upstream always wins: the user knows their service better
        // than the table does.
        return Delivery::Brokered {
            upstream: u.to_owned(),
            base_url_env,
        };
    }
    if let Some(name) = params.and_then(|p| p.get("name")).and_then(Value::as_str)
        && let Some((upstream, var)) = service_for(name)
    {
        return Delivery::Brokered {
            upstream: upstream.to_owned(),
            base_url_env: base_url_env.or_else(|| var.map(str::to_owned)),
        };
    }
    // Handing over is the safe default for an unrecognised credential: brokering
    // something that is not an HTTP bearer token fails at request time, and a
    // failure the user cannot diagnose is worse than a weaker mode they were told
    // about.
    Delivery::Handed
}

/// Build the list row for one secret: everything `vault.list` promises, including
/// the last use, so the main screen needs exactly one round trip (§7).
fn summarise(secret: &Secret, recent: &[Use]) -> Value {
    let last = recent.iter().find(|u| u.secret == secret.name);
    let letter = secret
        .display
        .chars()
        .find(|c| c.is_alphanumeric())
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_else(|| "•".into());

    // Delivery has to reach the client, not just the daemon. A brokered secret and
    // a handed one are used differently — the first needs the caller pointed at the
    // broker — and a consumer that cannot tell them apart gives instructions that
    // silently do not work.
    let delivery = if secret.delivery.is_brokered() {
        "brokered"
    } else {
        "handed"
    };
    let base_url_env = secret.delivery.base_url_variable(&secret.name);
    // The host the broker forwards to. Not a secret, and without it a caller
    // cannot tell what the loopback address actually reaches — an agent that
    // cannot tell declines to use it, which is what happened.
    let upstream = match &secret.delivery {
        Delivery::Brokered { upstream, .. } => Some(upstream.clone()),
        Delivery::Handed => None,
    };

    json!({
        "name": secret.name,
        "display": secret.display,
        "ref": format!("keyward://{}", secret.name),
        "masked": secret.masked,
        "delivery": delivery,
        "base_url_env": base_url_env,
        "upstream": upstream,
        "status": if last.is_none() { "unused" } else { secret.tier().status_key() },
        "letter": letter,
        "tint": tint_for(&secret.name),
        "logo": logo_for(&secret.name),
        "last_use": last.map(|u| json!({
            "at": u.at, "actor": u.actor, "project": u.project
        })),
    })
}

/// Bundled brand marks, keyed by the slug a user is likely to choose.
///
/// A miss is not a failure — the row falls back to a monogram, which is the
/// common case, since a secret can be for anything.
fn logo_for(name: &str) -> Option<&'static str> {
    const MARKS: &[(&str, &str)] = &[
        ("stripe", "StripeLogo"),
        ("openai", "OpenAILogo"),
        ("anthropic", "AnthropicLogo"),
        ("github", "GitHubLogo"),
        ("gitlab", "GitLabLogo"),
        ("resend", "ResendLogo"),
        ("supabase", "SupabaseLogo"),
        ("vercel", "VercelLogo"),
        ("netlify", "NetlifyLogo"),
        ("cloudflare", "CloudflareLogo"),
        ("railway", "RailwayLogo"),
        ("render", "RenderLogo"),
        ("sentry", "SentryLogo"),
        ("posthog", "PostHogLogo"),
        ("datadog", "DatadogLogo"),
        ("auth0", "Auth0Logo"),
        ("clerk", "ClerkLogo"),
        ("firebase", "FirebaseLogo"),
        ("cloudinary", "CloudinaryLogo"),
        ("algolia", "AlgoliaLogo"),
        ("meilisearch", "MeilisearchLogo"),
        ("mongodb", "MongoDBLogo"),
        ("mongo", "MongoDBLogo"),
        ("redis", "RedisLogo"),
        ("mysql", "MySQLLogo"),
        ("postgres", "PostgresLogo"),
        ("postgresql", "PostgresLogo"),
        ("pg", "PostgresLogo"),
        ("docker", "DockerLogo"),
        ("npm", "NpmLogo"),
        ("notion", "NotionLogo"),
        ("linear", "LinearLogo"),
        ("discord", "DiscordLogo"),
        ("telegram", "TelegramLogo"),
        ("shopify", "ShopifyLogo"),
        ("huggingface", "HuggingFaceLogo"),
        ("replicate", "ReplicateLogo"),
        ("deepseek", "DeepSeekLogo"),
        ("qwen", "QwenLogo"),
        ("moonshot", "MoonshotLogo"),
        ("kimi", "MoonshotLogo"),
        ("minimax", "MiniMaxLogo"),
        ("zhipu", "ZhipuLogo"),
        ("glm", "ZhipuLogo"),
        ("ollama", "OllamaLogo"),
        ("modelscope", "ModelScopeLogo"),
    ];
    // Longest match wins, so `pg-prod` finds `pg` while `postgres-prod` finds the
    // more specific `postgres` rather than stopping at `pg`.
    MARKS
        .iter()
        .filter(|(key, _)| name == *key || name.starts_with(&format!("{key}-")))
        .max_by_key(|(key, _)| key.len())
        .map(|(_, asset)| *asset)
}

/// Deterministic monogram tint, so a secret keeps the same colour across restarts
/// and across the two desktop apps.
fn tint_for(name: &str) -> String {
    const PALETTE: [&str; 8] = [
        "#5B51E8", "#0E9C74", "#2F6491", "#DC4C2B", "#7C51E8", "#B8622E", "#2F7D8F", "#8A4B7A",
    ];
    let sum: usize = name.bytes().map(usize::from).sum();
    PALETTE
        .get(sum % PALETTE.len())
        .copied()
        .unwrap_or("#5B51E8")
        .to_owned()
}

/// Seconds since the epoch to `YYYY-MM-DDTHH:MM:SSZ` (Howard Hinnant's
/// civil-from-days). Hand-rolled to keep a date crate out of the daemon.
fn now_iso8601() -> String {
    let epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
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
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        secs / 3600,
        (secs % 3600) / 60,
        secs % 60
    )
}
