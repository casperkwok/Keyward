//! `kw` — the Keyward CLI.
//!
//! Two commands are safe for a coding agent to run and cannot return a secret
//! value under any policy: `kw list` and `kw ref`. Everything that moves
//! plaintext goes through the daemon, which decides (ARCHITECTURE.md §9).
//!
//! No argument anywhere accepts a secret value. Values come from a TTY prompt or
//! stdin, because argv is world-readable on Linux and lands in shell history
//! everywhere.

mod client;
mod mcp;
mod render;
mod scan;
mod scrub;

use std::io::{IsTerminal, Read, Write};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use keyward_core::{Reference, derived_base_url_env, find_all};
use serde_json::{Value, json};
use zeroize::Zeroize;

use crate::client::{Client, Error};

const USAGE: &str = "\
kw — Keyward

  kw list [--json]          what exists. Names and masks only, never a value.
  kw ref <name>             print keyward://<name>
  kw status                 is the daemon up, and how many secrets

  kw add <name> [--upstream URL] [--base-url-env VAR]
                            prompts for the value on a TTY. Known services are
                            forwarded automatically; --upstream forwards anything
  kw rotate <name>          replace a value
  kw rm <name>              delete a secret

  kw pin <name>             a long-lived local token for an app you launch
                            yourself — put it in that app's config instead of
                            the key. `kw pin <name> --revoke` kills it.

  kw exec [-f .env] -- CMD  resolve keyward:// refs in .env, run CMD
  kw render TMPL -o OUT     resolve refs into a file — last resort, see below

  kw scan [--staged] [path] find literal secrets. Exits 1 on a hit, so it works
                            as a pre-commit hook.
  kw mcp [--http [PORT]]    MCP server: names and references, no values. On stdio
                            by default; --http serves the same tools at
                            http://127.0.0.1:8787/mcp

`kw render` is the only command that writes a secret to disk. It refuses any path
.gitignore does not cover, and the file it writes is mode 0600.

Values are never taken as arguments — argv is world-readable on Linux and
lands in shell history everywhere.
";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = run(&args);
    std::process::exit(code);
}

fn run(args: &[String]) -> i32 {
    let Some(command) = args.first().map(String::as_str) else {
        print!("{USAGE}");
        return 0;
    };

    let result = match command {
        "list" => cmd_list(args.contains(&"--json".to_string())),
        "ref" => cmd_ref(args.get(1)),
        "status" => cmd_status(),
        "add" => cmd_add(args),
        "rotate" => cmd_rotate(args.get(1)),
        "rm" | "remove" => cmd_remove(args.get(1)),
        "pin" => cmd_pin(args),
        "exec" => return cmd_exec(args.get(1..).unwrap_or_default()),
        "scan" => return scan::run(args.get(1..).unwrap_or_default()),
        "render" => return render::run(args.get(1..).unwrap_or_default()),
        // Speaks JSON-RPC on stdout for as long as it runs, so nothing else here
        // may print to it.
        "mcp" => {
            // `--http [PORT]` serves the same tools over loopback HTTP, so the
            // line a user pastes into their agent is a URL rather than a path
            // into the app bundle.
            return match args.iter().position(|a| a == "--http") {
                Some(i) => {
                    let port = args
                        .get(i + 1)
                        .and_then(|p| p.parse::<u16>().ok())
                        .unwrap_or(mcp::DEFAULT_HTTP_PORT);
                    mcp::run_http(port)
                }
                None => mcp::run(),
            };
        }
        "-h" | "--help" | "help" => {
            print!("{USAGE}");
            return 0;
        }
        other => {
            eprintln!("kw: unknown command `{other}`\n");
            print!("{USAGE}");
            return 2;
        }
    };

    match result {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("kw: {e}");
            e.exit_code()
        }
    }
}

// MARK: - Read-only commands

fn cmd_list(as_json: bool) -> Result<(), Error> {
    let result = Client::connect()?.call("vault.list", Value::Null)?;
    let secrets = result
        .get("secrets")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    if as_json {
        let out =
            serde_json::to_string_pretty(&secrets).map_err(|e| Error::Malformed(e.to_string()))?;
        println!("{out}");
        return Ok(());
    }

    if secrets.is_empty() {
        println!("No secrets yet. `kw add <name>` to store one.");
        return Ok(());
    }

    let width = secrets
        .iter()
        .filter_map(|s| s.get("name").and_then(Value::as_str))
        .map(str::len)
        .max()
        .unwrap_or(4)
        .max(4);

    for secret in &secrets {
        let name = secret.get("name").and_then(Value::as_str).unwrap_or("?");
        let masked = secret
            .get("masked")
            .and_then(Value::as_str)
            .unwrap_or("———");
        let status = secret.get("status").and_then(Value::as_str).unwrap_or("");
        let used = secret
            .get("last_use")
            .and_then(|u| {
                let actor = u.get("actor").and_then(Value::as_str)?;
                Some(match u.get("project").and_then(Value::as_str) {
                    Some(project) => format!("{actor} in {project}"),
                    None => actor.to_owned(),
                })
            })
            .unwrap_or_else(|| "never used".into());
        println!("{name:<width$}  {masked:<16}  {status:<9}  {used}");
    }
    Ok(())
}

fn cmd_ref(name: Option<&String>) -> Result<(), Error> {
    let Some(name) = name else {
        return Err(Error::Malformed("usage: kw ref <name>".into()));
    };
    // Validate locally so a typo fails here rather than printing a reference that
    // resolves to nothing.
    let reference = Reference::new(name.clone()).map_err(|e| Error::Malformed(e.to_string()))?;
    let mut client = Client::connect()?;
    let result = client.call("vault.list", Value::Null)?;
    let exists = result
        .get("secrets")
        .and_then(Value::as_array)
        .is_some_and(|list| {
            list.iter()
                .any(|s| s.get("name").and_then(Value::as_str) == Some(name.as_str()))
        });
    if !exists {
        return Err(Error::Refused {
            code: "not_found".into(),
            message: format!("no secret named `{name}`"),
        });
    }
    println!("{reference}");
    Ok(())
}

fn cmd_status() -> Result<(), Error> {
    let result = Client::connect()?.call("daemon.status", Value::Null)?;
    let version = result
        .get("version")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let count = result.get("secrets").and_then(Value::as_u64).unwrap_or(0);
    let stub = result.get("stub").and_then(Value::as_bool) == Some(true);
    println!("keywardd {version}{}", if stub { " (stub)" } else { "" });
    println!("{count} secrets");
    println!("socket {}", client::socket_path().display());
    Ok(())
}

/// A token for an app Keyward did not start.
///
/// Everything else here assumes the process using a key is one Keyward launched.
/// Codex, an IDE, a desktop client — the user starts those, and they read a
/// credential from a config file long before `kw exec` could inject anything.
/// The honest options were to put the real key in that file or to leave the case
/// unsolved; this is the third one.
fn cmd_pin(args: &[String]) -> Result<(), Error> {
    let Some(name) = args.get(1) else {
        return Err(Error::Malformed("usage: kw pin <name> [--revoke]".into()));
    };
    let mut client = Client::connect()?;

    if args.contains(&"--revoke".to_string()) {
        client.call("broker.unpin", json!({ "name": name }))?;
        println!("Revoked. Anything still using that token stops working now.");
        return Ok(());
    }

    let result = client.call("broker.pin", json!({ "name": name }))?;
    let token = result.get("token").and_then(Value::as_str).unwrap_or("");
    let base = result.get("base_url").and_then(Value::as_str).unwrap_or("");
    let upstream = result.get("upstream").and_then(Value::as_str).unwrap_or("");

    println!("{token}");
    println!();
    println!("  Put that where the app wants its API key, and point its base URL at");
    println!("  {base}");
    println!("  Requests go on to {upstream} with the real credential attached.");
    println!();
    println!("  It works only on this machine and only for `{name}`.");
    println!("  `kw pin {name} --revoke` kills it without touching the key itself.");
    Ok(())
}

// MARK: - Writes

fn cmd_add(args: &[String]) -> Result<(), Error> {
    let Some(name) = args.get(1) else {
        return Err(Error::Malformed("usage: kw add <name>".into()));
    };
    Reference::new(name.clone()).map_err(|e| Error::Malformed(e.to_string()))?;

    let upstream = flag(args, "--upstream");
    let base_url_env = flag(args, "--base-url-env");

    let mut value = read_secret(&format!("Value for {name}: "))?;
    let mut params = json!({"name": name, "display": name, "value": value});
    if let Some(upstream) = &upstream
        && let Some(object) = params.as_object_mut()
    {
        let mut delivery = json!({"kind": "brokered", "upstream": upstream});
        if let Some(var) = &base_url_env
            && let Some(d) = delivery.as_object_mut()
        {
            d.insert("base_url_env".into(), Value::String(var.clone()));
        }
        object.insert("delivery".into(), delivery);
    }
    let outcome = Client::connect()?.call("vault.add", params);
    value.zeroize();
    let result = outcome?;

    // Report which mode it landed in. The user did not choose it (DESIGN.md §4),
    // so they have to be told — and the daemon reports it directly, because
    // `vault.list`'s `status` says `unused` until first use and would have this
    // line confidently describing the wrong mode.
    match result.get("delivery").and_then(Value::as_str) {
        Some("brokered") => println!(
            "Stored, and forwarded by Keyward — no program will receive the value."
        ),
        _ => println!(
            "Stored, and handed to programs you launch — Keyward cannot forward this one.\nPass --upstream <url> if it is an HTTP API."
        ),
    }
    println!("Use keyward://{name} in your config.");
    Ok(())
}

/// `--flag value` lookup. Deliberately not a parser: three flags across the whole
/// CLI does not justify a dependency, and a hand-rolled one that silently accepts
/// `--flag=value` in some commands and not others is worse than neither.
fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn cmd_rotate(name: Option<&String>) -> Result<(), Error> {
    let Some(name) = name else {
        return Err(Error::Malformed("usage: kw rotate <name>".into()));
    };
    let mut value = read_secret(&format!("New value for {name}: "))?;
    let outcome = Client::connect()?.call("vault.rotate", json!({"name": name, "value": value}));
    value.zeroize();
    outcome?;
    println!("Replaced. keyward://{name} now resolves to the new value.");
    Ok(())
}

fn cmd_remove(name: Option<&String>) -> Result<(), Error> {
    let Some(name) = name else {
        return Err(Error::Malformed("usage: kw rm <name>".into()));
    };
    Client::connect()?.call("vault.remove", json!({ "name": name }))?;
    println!("Deleted `{name}`. Anything still referencing it will stop working.");
    Ok(())
}

/// Read a value from a TTY prompt, or from stdin when piped.
fn read_secret(prompt: &str) -> Result<String, Error> {
    if std::io::stdin().is_terminal() {
        eprint!("{prompt}");
        let _ = std::io::stderr().flush();
    }
    let mut buffer = String::new();
    std::io::stdin()
        .read_to_string(&mut buffer)
        .map_err(|e| Error::Io(e.to_string()))?;
    let value = buffer.trim_end_matches(['\n', '\r']).to_owned();
    buffer.zeroize();
    if value.is_empty() {
        return Err(Error::Malformed("no value given".into()));
    }
    Ok(value)
}

// MARK: - approval

/// Answer approval prompts from the terminal while a `secret.hand` call is in
/// flight.
///
/// The daemon blocks that call until a human answers or sixty seconds pass. Until
/// the GUI grows a prompt, nothing answers — so `kw exec` simply hung, produced no
/// output, and eventually failed. A wait with no explanation is indistinguishable
/// from a hang, and the user's next move is ^C.
///
/// Runs on its own connection because the one carrying `secret.hand` is busy
/// waiting for the very answer this thread exists to give.
fn watch_for_approval(stop: Arc<AtomicBool>) {
    let mine = std::process::id();
    let mut announced = false;

    while !stop.load(Ordering::Relaxed) {
        std::thread::sleep(std::time::Duration::from_millis(250));
        let Ok(mut client) = Client::connect() else { continue };
        let Ok(result) = client.call("approval.pending", Value::Null) else {
            continue;
        };
        let Some(pending) = result.get("pending").and_then(Value::as_array) else {
            continue;
        };

        for entry in pending {
            // Only ours — another run's prompt is not this run's to report.
            if entry.get("pid").and_then(Value::as_u64) != Some(u64::from(mine)) {
                continue;
            }
            let secret = entry.get("secret").and_then(Value::as_str).unwrap_or("?");

            // Announce and wait — never offer to answer here.
            //
            // This used to read a decision from stdin when stdin was a terminal.
            // The guard looked sufficient and was not: agent runners routinely
            // allocate a PTY, and in that case `is_terminal()` is true and the
            // keystroke comes from the agent. The prompt exists precisely to stop
            // the agent from taking a secret on its own say-so, so an agent able to
            // answer it turns the whole mechanism into decoration.
            //
            // The answer belongs in the GUI, where the thing typing cannot reach.
            // Nothing is lost: a person who never runs commands themselves — which
            // is the case this product is built for — was never going to see this
            // prompt anyway.
            if !announced {
                eprintln!("  waiting for you to approve `{secret}` in Keyward (60s)…");
                announced = true;
            }
        }
    }
}

// MARK: - exec

fn cmd_exec(args: &[String]) -> i32 {
    let mut env_path = String::from(".env");
    let mut rest: &[String] = args;
    if rest.first().map(String::as_str) == Some("-f")
        || rest.first().map(String::as_str) == Some("--file")
    {
        match rest.get(1) {
            Some(p) => {
                env_path = p.clone();
                rest = rest.get(2..).unwrap_or_default();
            }
            None => {
                eprintln!("kw: -f needs a path");
                return 2;
            }
        }
    }
    if rest.first().map(String::as_str) == Some("--") {
        rest = rest.get(1..).unwrap_or_default();
    }
    let Some((program, argv)) = rest.split_first() else {
        eprintln!("kw: nothing to run\n\nusage: kw exec [-f .env] -- <command>");
        return 2;
    };

    let text = std::fs::read_to_string(&env_path).unwrap_or_default();
    let found = find_all(&text);
    let mut names: Vec<String> = found
        .iter()
        .map(|f| f.reference.name().to_owned())
        .collect();
    names.sort();
    names.dedup();

    // `values` holds plaintext for handed secrets; `tokens` holds a session token
    // for brokered ones. Only the first ever reaches the child as a real
    // credential, and only the first needs scrubbing.
    let mut values: Vec<(String, String)> = Vec::new();
    let mut tokens: Vec<(String, String)> = Vec::new();
    let mut extra_env: Vec<(String, String)> = Vec::new();
    let mut sessions: Vec<String> = Vec::new();
    let mut brokered: Vec<String> = Vec::new();

    if !names.is_empty() {
        let mut client = match Client::connect() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("kw: {e}");
                return e.exit_code();
            }
        };
        let project = std::env::current_dir()
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()));

        // Try to broker first, always. Handing a value over is the fallback, not
        // the default — otherwise a secret that could have been protected quietly
        // is not.
        let mut to_hand: Vec<String> = Vec::new();
        for name in &names {
            // The project travels with the session, not with each request: the
            // broker only ever sees HTTP, and "which project used this key" is
            // one of the two questions the usage log exists to answer.
            let params = match &project {
                Some(p) => json!({ "name": name, "project": p }),
                None => json!({ "name": name }),
            };
            match client.call("broker.open", params) {
                Ok(result) => {
                    let token = result
                        .get("token")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned();
                    let base = result
                        .get("base_url")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned();
                    if let Some(session) = result.get("session").and_then(Value::as_str) {
                        sessions.push(session.to_owned());
                    }
                    // The daemon always names a variable now — the conventional one
                    // where a service has one, `KEYWARD_URL_<NAME>` where it does
                    // not. Exporting nothing was the old behaviour, and it left the
                    // child with a session token its SDK sent straight to the real
                    // host: a `401` with no visible cause.
                    let var = result
                        .get("base_url_env")
                        .and_then(Value::as_str)
                        .map_or_else(|| derived_base_url_env(name), str::to_owned);
                    extra_env.push((var, base.clone()));
                    tokens.push((name.clone(), token));
                }
                Err(Error::Refused { ref code, .. }) if code == "denied" => {
                    to_hand.push(name.clone());
                }
                Err(e) => {
                    eprintln!("kw: {e}");
                    return e.exit_code();
                }
            }
        }
        brokered = tokens.iter().map(|(n, _)| n.clone()).collect();

        if !to_hand.is_empty() {
            let request = json!({
                "names": to_hand,
                "actor": program,
                "project": project,
            });
            // The call blocks while a human is asked; this answers on the side.
            let stop = Arc::new(AtomicBool::new(false));
            let watcher = {
                let stop = Arc::clone(&stop);
                std::thread::spawn(move || watch_for_approval(stop))
            };
            let outcome = client.call("secret.hand", request);
            stop.store(true, Ordering::Relaxed);
            let _ = watcher.join();

            match outcome {
                Ok(result) => {
                    if let Some(map) = result.get("values").and_then(Value::as_object) {
                        for (name, value) in map {
                            if let Some(v) = value.as_str() {
                                values.push((name.clone(), v.to_owned()));
                            }
                        }
                    }
                }
                Err(e) => {
                    eprintln!("kw: {e}");
                    return e.exit_code();
                }
            }
        }
    }

    // Map each `KEY=keyward://name` line to the value it resolved to.
    let mut env_pairs: Vec<(String, String)> = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            continue;
        }
        let Some((key, raw)) = trimmed.split_once('=') else {
            continue;
        };
        let raw = raw.trim().trim_matches(['"', '\'']);
        let Ok(reference) = raw.parse::<Reference>() else {
            continue;
        };
        let key = key.trim().to_owned();
        if let Some((name, token)) = tokens.iter().find(|(n, _)| n == reference.name()) {
            eprintln!("  {key}  forwarded — `{name}` never leaves Keyward");
            env_pairs.push((key, token.clone()));
        } else if let Some((_, value)) = values.iter().find(|(n, _)| n == reference.name()) {
            eprintln!("  {key}  handed over — the value is in this process's environment");
            env_pairs.push((key, value.clone()));
        }
    }
    env_pairs.extend(extra_env);

    let rules: Vec<scrub::Rule> = values
        .iter()
        .flat_map(|(name, value)| scrub::Rule::expand(value, &format!("keyward://{name}")))
        .collect();
    let _ = &brokered;

    let mut child = match Command::new(program)
        .args(argv)
        .envs(env_pairs.iter().map(|(k, v)| (k.clone(), v.clone())))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!("kw: could not run `{program}`: {e}");
            return 127;
        }
    };

    // Values are in the child's environment now; drop this process's copies.
    for (_, mut value) in values {
        value.zeroize();
    }
    for (_, mut value) in env_pairs {
        value.zeroize();
    }

    let out = child.stdout.take();
    let err = child.stderr.take();
    let out_rules = rules;
    let err_rules: Vec<scrub::Rule> = out_rules
        .iter()
        .map(|r| scrub::Rule {
            needle: r.needle.clone(),
            replacement: r.replacement.clone(),
            match_prefix: r.match_prefix,
        })
        .collect();

    let pump_err = std::thread::spawn(move || {
        if let Some(err) = err {
            let _ = scrub::pipe(err, std::io::stderr(), &err_rules);
        }
    });
    if let Some(out) = out {
        let _ = scrub::pipe(out, std::io::stdout(), &out_rules);
    }
    let _ = pump_err.join();

    let code = child.wait().ok().and_then(|s| s.code()).unwrap_or(1);

    // Close every session. A token that outlives the process it was minted for is
    // a credential lying around with nothing watching it.
    if !sessions.is_empty()
        && let Ok(mut client) = Client::connect()
    {
        for session in &sessions {
            let _ = client.call("broker.close", json!({ "session": session }));
        }
    }
    code
}
