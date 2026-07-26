//! `kw mcp` — the agent-facing API (ARCHITECTURE.md §10.4).
//!
//! An earlier draft cut this, reasoning that an agent has no legitimate need to
//! query a vault. That was wrong. The way people actually talk to coding agents is
//! "go get the Stripe key from Keyward and put it in `.env`" — the agent *is* the
//! thing wiring the project up. Without an interface it either invents the
//! reference syntax or asks the user to paste a key, which is the exact outcome
//! this product exists to prevent.
//!
//! **The security property is structural, not conditional.** There is no code path
//! from here to plaintext: this module calls exactly one daemon method,
//! `vault.list`, which cannot return a value under any policy. Not "returns a value
//! if policy allows" — the request is never constructed. That is what makes it safe
//! to hand this interface to the threat actor, because enumerating a list of names
//! is harmless. Anything added here later must keep that true.
//!
//! **The tool descriptions are the operating instructions**, deliberately. The
//! model reads them on every call and cannot delete them the way it can ignore a
//! `CLAUDE.md` section. They state the `kw exec` requirement, and they state that
//! plaintext is unavailable *and why*, so an agent asked for a literal value
//! explains the situation instead of hunting for a workaround.
//!
//! Protocol: JSON-RPC 2.0, one object per line, over stdio. Written by hand against
//! the MCP spec rather than pulled from an SDK — the surface is three tools and
//! four methods, and a dependency here is a dependency in the binary that a coding
//! agent is invited to run.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::Path;

use serde_json::{Value, json};

use crate::client::Client;
use crate::scan;

/// Fallback protocol version when a client does not name one.
const PROTOCOL_VERSION: &str = "2025-06-18";

/// The port `kw mcp --http` listens on unless told otherwise.
///
/// Fixed rather than ephemeral, because the whole point of the HTTP transport is
/// that the line a user pastes into their agent stays the same forever. An
/// ephemeral port would put us back where the stdio transport was: a config that
/// has to be regenerated whenever something moves.
pub const DEFAULT_HTTP_PORT: u16 = 8787;

/// Serve MCP over loopback HTTP instead of stdio.
///
/// The reason this exists is not technical. Over stdio the client config has to
/// name a binary by absolute path — `/Applications/Keyward.app/Contents/MacOS/kw`
/// — which asks a person who has never opened a terminal to reason about the
/// filesystem, and breaks the moment the app is moved. Over HTTP the same config
/// is a URL that never changes.
pub fn run_http(port: u16) -> i32 {
    let listener = match TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, port))) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("kw: cannot listen on 127.0.0.1:{port}: {e}");
            return 1;
        }
    };
    // Loopback only, never 0.0.0.0: this server answers questions about which
    // secrets exist, and that is a question for this machine alone.
    println!("keyward mcp on http://127.0.0.1:{port}/mcp");
    let _ = std::io::stdout().flush();

    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        std::thread::spawn(move || {
            let _ = serve_http(stream);
        });
    }
    0
}

fn serve_http(mut stream: TcpStream) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    if reader.read_line(&mut request_line)? == 0 {
        return Ok(());
    }
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_owned();
    let path = parts.next().unwrap_or_default().to_owned();

    let mut length = 0usize;
    let mut host = String::new();
    let mut origin: Option<String> = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        match name.to_ascii_lowercase().as_str() {
            "content-length" => length = value.parse().unwrap_or(0),
            "host" => host = value.to_owned(),
            "origin" => origin = Some(value.to_owned()),
            _ => {}
        }
    }

    // A web page can make its browser POST to 127.0.0.1. It cannot set `Origin`,
    // so refusing every request that carries one keeps this endpoint out of reach
    // of anything running in a browser tab. The MCP spec asks for the `Host`
    // check for the same reason, against DNS rebinding.
    if origin.is_some() {
        return http_reply(&mut stream, 403, "requests from a browser are not served");
    }
    let hostname = host.split(':').next().unwrap_or_default();
    if !matches!(hostname, "127.0.0.1" | "localhost" | "[::1]" | "::1" | "") {
        return http_reply(&mut stream, 421, "this server answers on 127.0.0.1 only");
    }
    if !path.starts_with("/mcp") {
        return http_reply(&mut stream, 404, "the endpoint is /mcp");
    }
    if method != "POST" {
        // No SSE stream: every tool here answers immediately, so there is nothing
        // for a long-lived channel to carry.
        return http_reply(&mut stream, 405, "POST a JSON-RPC message to /mcp");
    }

    let mut body = vec![0u8; length];
    reader.read_exact(&mut body)?;

    let response = match serde_json::from_slice::<Value>(&body) {
        Ok(request) => handle(&request),
        Err(e) => Some(error_response(
            Value::Null,
            -32700,
            &format!("parse error: {e}"),
        )),
    };

    match response {
        // A notification gets no JSON-RPC reply; HTTP still needs a status.
        None => http_status(&mut stream, 202),
        Some(value) => {
            let body = value.to_string();
            write!(
                stream,
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                body.len(),
                body
            )?;
            stream.flush()
        }
    }
}

fn http_reply(stream: &mut TcpStream, status: u16, message: &str) -> std::io::Result<()> {
    let body = format!("{{\"error\":{{\"source\":\"keyward\",\"message\":\"{message}\"}}}}");
    write!(
        stream,
        "HTTP/1.1 {status} \r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    )?;
    stream.flush()
}

fn http_status(stream: &mut TcpStream, status: u16) -> std::io::Result<()> {
    write!(stream, "HTTP/1.1 {status} \r\ncontent-length: 0\r\nconnection: close\r\n\r\n")?;
    stream.flush()
}

pub fn run() -> i32 {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();

    for line in stdin.lock().lines() {
        let Ok(line) = line else {
            return 1;
        };
        if line.trim().is_empty() {
            continue;
        }
        let response = match serde_json::from_str::<Value>(&line) {
            Ok(request) => handle(&request),
            Err(e) => Some(error_response(
                Value::Null,
                -32700,
                &format!("parse error: {e}"),
            )),
        };
        // A notification gets no reply at all. Answering one — even with a success
        // envelope — is a protocol violation that some clients treat as fatal.
        let Some(response) = response else { continue };
        if writeln!(stdout, "{response}").is_err() || stdout.flush().is_err() {
            return 1;
        }
    }
    0
}

/// Dispatch one message. `None` means "this was a notification, stay silent".
fn handle(request: &Value) -> Option<Value> {
    let method = request.get("method").and_then(Value::as_str).unwrap_or("");
    let id = request.get("id").cloned();

    // The absence of `id`, not the method name, is what makes a message a
    // notification — so an unknown notification must also go unanswered rather
    // than producing a reply to an id that does not exist.
    let id = id.filter(|id| !id.is_null())?;

    let response = match method {
        "initialize" => ok_response(id, initialize(request)),
        "tools/list" => ok_response(id, json!({ "tools": tools() })),
        "tools/call" => ok_response(id, call_tool(request)),
        "ping" => ok_response(id, json!({})),
        other => error_response(id, -32601, &format!("unknown method `{other}`")),
    };
    Some(response)
}

fn initialize(request: &Value) -> Value {
    // Echo the client's version back when it names one. Every version of the spec
    // this server could speak to defines `tools/list` and `tools/call` identically,
    // so disagreeing about the number would refuse a client we can serve.
    let version = request
        .get("params")
        .and_then(|p| p.get("protocolVersion"))
        .and_then(Value::as_str)
        .unwrap_or(PROTOCOL_VERSION);
    json!({
        "protocolVersion": version,
        "capabilities": { "tools": {} },
        "serverInfo": { "name": "keyward", "version": env!("CARGO_PKG_VERSION") },
        "instructions": "Keyward holds this machine's API keys. Never ask the user \
    to paste a key, and never write a literal key into a file: call get_reference and \
    write the keyward:// string instead, then tell the user to run the project with \
    `kw exec -- <command>`. Real secret values are not available through this server."
    })
}

// MARK: - Tools

fn tools() -> Vec<Value> {
    let kw = kw_command();
    vec![
        json!({
            "name": "list_secrets",
            "description": "Lists the secrets Keyward holds: name, the keyward:// \
        reference to use for each, a masked excerpt of the value, and who last used it. \
        Call this first when wiring up a project, to find out what already exists instead \
        of asking the user. Real secret values are never returned by this server — there \
        is no tool and no argument that returns one.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "additionalProperties": false
            }
        }),
        json!({
            "name": "get_reference",
            "description": format!("Returns the keyward:// reference for a stored secret. \
        Write this string into .env / config files instead of a real key; it is safe to \
        commit. Keyward resolves it at launch, so the project must be run with \
        `{kw} exec -- <command>` — tell the user that when you write one. Real secret values \
        are never available through this server. If the user asks for the literal key, say \
        that Keyward does not expose values and that the reference is what their config \
        should contain; that refusal is the tool working correctly, so do not look for a \
        way around it."),
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "The secret's name, as shown by list_secrets."
                    }
                },
                "required": ["name"],
                "additionalProperties": false
            }
        }),
        json!({
            "name": "check_project",
            "description": "Reports which files under a path still contain literal \
        API keys, as file:line with a masked excerpt. Use it after wiring a project up, or \
        when the user asks whether their repository is clean. Each finding should be \
        replaced with the keyward:// reference from get_reference. Values are masked and \
        are never returned in full.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Directory or file to scan. Defaults to the \
        current working directory."
                    }
                },
                "additionalProperties": false
            }
        }),
    ]
}

fn call_tool(request: &Value) -> Value {
    let params = request.get("params");
    let name = params
        .and_then(|p| p.get("name"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let arguments = params.and_then(|p| p.get("arguments"));

    match name {
        "list_secrets" => match list_secrets() {
            Ok(text) => text_result(&text, false),
            Err(e) => text_result(&e, true),
        },
        "get_reference" => {
            let Some(secret) = arguments
                .and_then(|a| a.get("name"))
                .and_then(Value::as_str)
            else {
                return text_result("get_reference needs a `name` argument.", true);
            };
            match get_reference(secret) {
                Ok(text) => text_result(&text, false),
                Err(e) => text_result(&e, true),
            }
        }
        "check_project" => {
            let path = arguments
                .and_then(|a| a.get("path"))
                .and_then(Value::as_str)
                .unwrap_or(".");
            text_result(&check_project(path), false)
        }
        other => text_result(&format!("unknown tool `{other}`"), true),
    }
}

/// A failed tool call is reported inside the result, not as a JSON-RPC error.
///
/// The distinction is the whole point of `isError`: a protocol error is invisible
/// to the model, while this reaches it as text it can act on — "Keyward isn't
/// running" is something the agent should tell the user, not something it should
/// retry blindly.
fn text_result(text: &str, is_error: bool) -> Value {
    json!({
        "content": [{ "type": "text", "text": text }],
        "isError": is_error
    })
}

/// How to invoke `kw` in a command an agent will actually run.
///
/// Not the bare word `kw`. The symlink into `/usr/local/bin` is something the user
/// has to go and click in Settings, and a real agent session proved what happens
/// when they have not: it wired the client correctly, ran `kw exec`, and got
/// `command not found`. This server is `kw`, so it knows exactly where it is —
/// guessing was never necessary.
fn kw_command() -> String {
    std::env::current_exe()
        .ok()
        .filter(|p| p.is_absolute())
        .map_or_else(|| "kw".to_owned(), |p| p.display().to_string())
}

fn list_secrets() -> Result<String, String> {
    let result = Client::connect()
        .and_then(|mut c| c.call("vault.list", Value::Null))
        .map_err(|e| e.to_string())?;
    let secrets = result
        .get("secrets")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    if secrets.is_empty() {
        return Ok(format!(
            "Keyward holds no secrets yet. Ask the user to add one in the Keyward \
app — never ask them to paste a key to you. (`{kw} add <name>` does the same from a \
terminal, if they prefer one.)",
            kw = kw_command()
        ));
    }

    let mut out = String::from("name\treference\tdelivery\tmasked\tlast use\n");
    for secret in &secrets {
        let name = secret.get("name").and_then(Value::as_str).unwrap_or("?");
        let reference = secret
            .get("ref")
            .and_then(Value::as_str)
            .map_or_else(|| format!("keyward://{name}"), str::to_owned);
        let masked = secret.get("masked").and_then(Value::as_str).unwrap_or("—");
        let delivery = secret
            .get("delivery")
            .and_then(Value::as_str)
            .unwrap_or("handed");
        let used = secret
            .get("last_use")
            .and_then(|u| {
                let actor = u.get("actor").and_then(Value::as_str)?;
                let at = u.get("at").and_then(Value::as_str).unwrap_or("");
                Some(match u.get("project").and_then(Value::as_str) {
                    Some(project) => format!("{actor} in {project} {at}"),
                    None => format!("{actor} {at}"),
                })
            })
            .unwrap_or_else(|| "never used".into());
        out.push_str(&format!("{name}\t{reference}\t{delivery}\t{masked}\t{used}\n"));
    }
    out.push_str(&format!(
        "\nWrite the reference into config files, never the value. \
Run the project with `{kw} exec -- <command>` so the references resolve. \
Call get_reference before using one: `brokered` and `handed` secrets are wired \
into a client differently, and get_reference gives the exact steps.",
        kw = kw_command()
    ));
    Ok(out)
}

fn get_reference(name: &str) -> Result<String, String> {
    // Confirm the secret exists before handing back a reference. A reference to
    // nothing is worse than an error: it lands in a `.env`, looks correct in review,
    // and fails much later inside somebody else's "invalid API key" message.
    let result = Client::connect()
        .and_then(|mut c| c.call("vault.list", Value::Null))
        .map_err(|e| e.to_string())?;
    let secrets = result
        .get("secrets")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let known: Vec<&str> = secrets
        .iter()
        .filter_map(|s| s.get("name").and_then(Value::as_str))
        .collect();

    if !known.contains(&name) {
        return Err(format!(
            "No secret named `{name}`. Keyward holds: {held}. Ask the user to add it \
in the Keyward app, under that name. Do not ask them to paste the value to you — \
keeping it out of this conversation is the point. (`{kw} add {name}` is the terminal \
equivalent.)",
            kw = kw_command(),
            held = if known.is_empty() {
                "nothing yet".to_owned()
            } else {
                known.join(", ")
            }
        ));
    }
    // The reference alone is not enough to use the secret, and a client wired the
    // wrong way fails at request time with a `401` that looks like a bad key. A
    // brokered secret resolves to a *session token* that only the loopback broker
    // accepts, so the client has to be pointed at the broker as well — which is
    // the one step an agent will not infer, and did not.
    let secret = secrets
        .iter()
        .find(|s| s.get("name").and_then(Value::as_str) == Some(name));
    let brokered = secret.and_then(|s| s.get("delivery")).and_then(Value::as_str) == Some("brokered");
    let base_url_env = secret
        .and_then(|s| s.get("base_url_env"))
        .and_then(Value::as_str);
    let upstream = secret
        .and_then(|s| s.get("upstream"))
        .and_then(Value::as_str)
        .unwrap_or("the service this key belongs to");

    if let (true, Some(var)) = (brokered, base_url_env) {
        return Ok(format!(
            "keyward://{name}   (forwarded — the value never reaches the program)\n\n\
1. Put `keyward://{name}` in the config file where the key would have gone. It is \
safe to commit.\n\
2. Run the project with `{kw} exec -- <command>`. At launch the reference becomes a \
session token, and `${var}` is set to Keyward's local address. Requests sent there \
are forwarded to {upstream}, so the paths stay exactly as the API documents them.\n\
3. Point the client at that address, or the token goes to the real API and is \
rejected. Read the base URL from `${var}` — the address changes every run, so it \
cannot be hard-coded. Most SDKs take it as a `base_url` / `baseURL` argument.\n\n\
Keyward swaps the session token for the real credential on the way out, so the \
token is useless anywhere else and expires with the process.",
            kw = kw_command()
        ));
    }

    Ok(format!(
        "keyward://{name}   (handed to the program — no forwarding is possible for \
this kind of credential)\n\nPut this string in the config file where the key would \
have gone. It is safe to commit. Run the project with `{kw} exec -- <command>` so \
Keyward resolves it at launch. Keyward removes the value from the program's output, \
but the program itself does hold it — so do not print it or copy it elsewhere.",
        kw = kw_command()
    ))
}

fn check_project(path: &str) -> String {
    let findings = scan::scan_path(Path::new(path));
    if findings.is_empty() {
        return format!("No literal secrets found under `{path}`.");
    }
    let mut out = format!("{} literal secret(s) still in files:\n", findings.len());
    for finding in &findings {
        out.push_str(&format!("{finding}\n"));
    }
    out.push_str(&format!(
        "\nReplace each one with the keyward:// reference from get_reference. \
If a secret is not in Keyward yet, ask the user to add it in the Keyward app, so the \
value never enters this conversation.",
    ));
    out
}

// MARK: - Envelopes

fn ok_response(id: Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

fn error_response(id: Value, code: i32, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialize_answers_with_tools_capability() {
        let request = json!({"jsonrpc":"2.0","id":1,"method":"initialize",
            "params":{"protocolVersion":"2025-03-26","clientInfo":{"name":"t","version":"0"}}});
        let Some(response) = handle(&request) else {
            unreachable!("initialize is a request, not a notification")
        };
        assert_eq!(
            response.pointer("/result/protocolVersion"),
            Some(&json!("2025-03-26"))
        );
        assert!(response.pointer("/result/capabilities/tools").is_some());
        assert_eq!(response.pointer("/id"), Some(&json!(1)));
    }

    #[test]
    fn notifications_get_no_reply() {
        // Answering one is a protocol violation, and some clients treat a reply to
        // an id that does not exist as fatal.
        let request = json!({"jsonrpc":"2.0","method":"notifications/initialized"});
        assert!(handle(&request).is_none());
    }

    #[test]
    fn exposes_exactly_three_tools() {
        let names: Vec<String> = tools()
            .iter()
            .filter_map(|t| t.get("name").and_then(Value::as_str).map(str::to_owned))
            .collect();
        assert_eq!(names, ["list_secrets", "get_reference", "check_project"]);
    }

    #[test]
    fn every_description_says_values_are_unavailable() {
        // The description is the only instruction the model is guaranteed to read,
        // and it is what stops an agent from going looking for a workaround.
        for tool in tools() {
            let text = tool
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_ascii_lowercase();
            assert!(
                text.contains("never returned")
                    || text.contains("never available")
                    || text.contains("masked"),
                "no plaintext disclaimer: {tool}"
            );
        }
    }

    #[test]
    fn unknown_method_is_a_jsonrpc_error_not_a_result() {
        let request = json!({"jsonrpc":"2.0","id":9,"method":"resources/list"});
        let Some(response) = handle(&request) else {
            unreachable!("a request with an id must be answered")
        };
        assert_eq!(response.pointer("/error/code"), Some(&json!(-32601)));
    }

    #[test]
    fn unknown_tool_is_a_result_with_is_error() {
        // Tool failures reach the model as text; protocol errors do not.
        let request = json!({"jsonrpc":"2.0","id":3,"method":"tools/call",
            "params":{"name":"read_secret","arguments":{"name":"stripe"}}});
        let Some(response) = handle(&request) else {
            unreachable!("a request with an id must be answered")
        };
        assert_eq!(response.pointer("/result/isError"), Some(&json!(true)));
    }

    #[test]
    fn check_project_masks_what_it_finds() {
        let dir = std::env::temp_dir().join(format!("kw-mcp-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join(".env"), "STRIPE=sk_live_51H8xQ2eZvKYlo2C9\n");

        let report = check_project(&dir.to_string_lossy());
        assert!(report.contains(".env:1"), "{report}");
        assert!(
            !report.contains("51H8xQ2eZvKY"),
            "the report leaked the value: {report}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
