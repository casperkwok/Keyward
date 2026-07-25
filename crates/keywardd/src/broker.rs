//! The loopback broker (ARCHITECTURE.md §6.1).
//!
//! The one thing Keyward does that off-the-shelf secret managers do not. A child
//! process is pointed at `http://127.0.0.1:PORT` and given a session token; every
//! request it makes arrives here, where the token is stripped and the real
//! credential is attached on the way upstream.
//!
//! The consequence is the product's headline claim: **the value is never in the
//! child's address space.** `op run` and friends inject the real thing, and a
//! coding agent that is the child's parent can simply print it. Here there is
//! nothing to print — the token is worthless off this machine and dies with the
//! session.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use zeroize::Zeroize;

/// How long a session lives when the opener does not say (§6.1: "on child exit,
/// **or after TTL**").
///
/// `kw exec` closes its own session when the child dies, so this backstops
/// everything that cannot: a GUI-opened session, a `kw` killed with SIGKILL, a
/// crashed child. Without it a laptop left open for a week keeps a live route to a
/// real credential, which is the state the broker exists to avoid.
pub const DEFAULT_TTL_SECS: u64 = 3_600;

/// The longest TTL an opener may ask for. A day is longer than any plausible
/// agent run and short enough that a session survives no reboot-free week; a
/// parameter able to disable the backstop entirely would be a way to opt out of
/// §6.1 by typing a large number.
pub const MAX_TTL_SECS: u64 = 86_400;

/// How often the reaper sweeps. Expiry is also checked on every request, so this
/// only bounds how long a dead route occupies memory and the live-sessions list —
/// nothing is authorised in the gap.
const REAP_EVERY: Duration = Duration::from_secs(30);

/// Where one session's traffic goes and what it is authorised with.
#[derive(Clone)]
struct Route {
    upstream: String,
    credential: String,
    secret: String,
    opened_at: u64,
    /// Epoch seconds after which this token authorises nothing.
    expires_at: u64,
    requests: u64,
    /// The last `User-Agent` seen, so the app can say *what* is talking rather
    /// than only that something is.
    actor: Option<String>,
    /// The attested peer that opened the session (§7), carried so a forwarded
    /// request can be logged against a resolved identity rather than against the
    /// `User-Agent` the child chose for itself.
    owner: Option<String>,
    owner_label: Option<String>,
    /// The project the session was opened for. The broker cannot work this out —
    /// it sees an HTTP request, not a working directory — so the opener supplies
    /// it. Without it the "which project used this" column is empty for exactly
    /// the traffic Keyward protects best.
    project: Option<String>,
}

/// A route now dies on a timer rather than only on an explicit close, so the
/// credential it carries is scrubbed on the way out instead of being left in
/// freed heap for whatever allocates next.
impl Drop for Route {
    fn drop(&mut self) {
        self.credential.zeroize();
    }
}

/// What the daemon knows at `broker.open` time that the broker itself cannot
/// work out: how long the session may live, and who asked for it.
pub struct Opened {
    pub ttl_secs: u64,
    pub owner: Option<String>,
    pub owner_label: Option<String>,
    pub project: Option<String>,
}

/// One open session, as the app sees it. Carries no credential.
pub struct Session {
    pub token: String,
    pub secret: String,
    pub opened_at: u64,
    pub expires_at: u64,
    pub requests: u64,
    pub actor: Option<String>,
}

#[derive(Clone)]
pub struct Broker {
    port: u16,
    routes: Arc<Mutex<HashMap<String, Route>>>,
}

/// Recorded so the daemon can log a use without the broker knowing what a usage
/// log is.
pub struct Served {
    pub secret: String,
    pub actor: String,
    /// The attested opener of the session, when there was one.
    pub caller: Option<String>,
    pub project: Option<String>,
}

impl Broker {
    /// Bind an ephemeral loopback port and start serving.
    ///
    /// `127.0.0.1` explicitly, never `0.0.0.0`: a broker reachable from the
    /// network would hand a credential to anyone who could guess a token.
    pub fn start(on_use: impl Fn(Served) + Send + Sync + 'static) -> std::io::Result<Self> {
        let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))?;
        let port = listener.local_addr()?.port();
        let routes: Arc<Mutex<HashMap<String, Route>>> = Arc::new(Mutex::new(HashMap::new()));

        // The reaper. Requests already refuse an expired route, so this exists to
        // stop a forgotten session sitting in the live-sessions list — and in this
        // process's memory, still holding a credential — long after it could serve
        // anything.
        let reaping = Arc::clone(&routes);
        thread::spawn(move || {
            loop {
                thread::sleep(REAP_EVERY);
                if let Ok(mut routes) = reaping.lock() {
                    reap(&mut routes, epoch_secs());
                }
            }
        });

        let serving = Arc::clone(&routes);
        let notify = Arc::new(on_use);
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                let routes = Arc::clone(&serving);
                let notify = Arc::clone(&notify);
                thread::spawn(move || {
                    if let Err(e) = handle(stream, &routes, notify.as_ref()) {
                        eprintln!("broker: {e}");
                    }
                });
            }
        });

        Ok(Self { port, routes })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// Register a session. The token is 32 bytes of randomness from the OS,
    /// rendered hex — it authorises nothing beyond this machine and this run.
    ///
    /// `ttl_secs` is a ceiling on that run. It is clamped rather than trusted: a
    /// caller asking for a ten-year session is asking for the backstop to be
    /// removed, which is the one thing the parameter must not be able to do.
    pub fn open(
        &self,
        secret: &str,
        upstream: &str,
        credential: String,
        opened: Opened,
    ) -> Option<Session> {
        let token = format!("kws_{}", random_hex());
        let now = epoch_secs();
        let ttl = opened.ttl_secs.clamp(1, MAX_TTL_SECS);
        let route = Route {
            upstream: upstream.trim_end_matches('/').to_owned(),
            credential,
            secret: secret.to_owned(),
            opened_at: now,
            expires_at: now.saturating_add(ttl),
            requests: 0,
            actor: None,
            owner: opened.owner,
            owner_label: opened.owner_label,
            project: opened.project,
        };
        let session = Session {
            token: token.clone(),
            secret: route.secret.clone(),
            opened_at: route.opened_at,
            expires_at: route.expires_at,
            requests: 0,
            actor: None,
        };
        self.routes.lock().ok()?.insert(token, route);
        Some(session)
    }

    pub fn close(&self, token: &str) -> bool {
        self.routes
            .lock()
            .map(|mut r| r.remove(token).is_some())
            .unwrap_or(false)
    }

    pub fn open_sessions(&self) -> usize {
        let now = epoch_secs();
        self.routes
            .lock()
            .map(|r| r.values().filter(|route| route.expires_at > now).count())
            .unwrap_or(0)
    }

    /// Live sessions, newest first. Deliberately returns no credential — this
    /// feeds a UI, and a struct that could carry a secret into a view layer is a
    /// leak waiting for someone to log it.
    pub fn sessions(&self) -> Vec<Session> {
        let Ok(routes) = self.routes.lock() else {
            return Vec::new();
        };
        let now = epoch_secs();
        let mut out: Vec<Session> = routes
            .iter()
            // An expired route is not a session. Showing one would tell a user a
            // credential is in use when nothing can reach it, which is the exact
            // reading of the live indicator the product must never get wrong.
            .filter(|(_, route)| route.expires_at > now)
            .map(|(token, route)| Session {
                token: token.clone(),
                secret: route.secret.clone(),
                opened_at: route.opened_at,
                expires_at: route.expires_at,
                requests: route.requests,
                actor: route.actor.clone(),
            })
            .collect();
        out.sort_by_key(|s| std::cmp::Reverse(s.opened_at));
        out
    }
}

/// Drop everything that has expired, releasing its credential.
fn reap(routes: &mut HashMap<String, Route>, now: u64) {
    routes.retain(|_, route| route.expires_at > now);
}

/// Authorise one request against a token, counting it if it is good.
///
/// Expired and invented tokens are the *same* answer — `None` — and the caller
/// turns both into the same 401 with the same wording. A distinguishable "expired"
/// reply would confirm to whoever is guessing that the token was once real, and
/// that a session for that secret exists at all.
fn authorise(
    routes: &mut HashMap<String, Route>,
    token: &str,
    agent: Option<&str>,
    now: u64,
) -> Option<Route> {
    let route = routes.get_mut(token)?;
    if route.expires_at <= now {
        // Drop it here rather than waiting for the sweep, so the credential leaves
        // memory at the moment it is proven useless.
        routes.remove(token);
        return None;
    }
    route.requests += 1;
    if let Some(agent) = agent {
        route.actor = Some(agent.to_owned());
    }
    Some(route.clone())
}

fn epoch_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn random_hex() -> String {
    let mut bytes = [0u8; 32];
    // `/dev/urandom` rather than a crate: one read, no dependency, and the
    // failure mode is loud rather than a weak token.
    if let Ok(mut file) = std::fs::File::open("/dev/urandom") {
        let _ = file.read_exact(&mut bytes);
    }
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

// MARK: - Request handling

struct Request {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    token: Option<String>,
}

fn handle(
    mut stream: TcpStream,
    routes: &Arc<Mutex<HashMap<String, Route>>>,
    notify: &(dyn Fn(Served) + Send + Sync),
) -> std::io::Result<()> {
    let request = match read_request(&mut stream)? {
        Ok(r) => r,
        Err(reason) => return respond(&mut stream, 400, &reason),
    };

    let Some(token) = request.token.clone() else {
        return respond(&mut stream, 401, "no Keyward session token");
    };
    let agent = request
        .headers
        .iter()
        .find(|(k, _)| k == "user-agent")
        .map(|(_, v)| v.clone());

    // Look up and count in one lock, so the live view cannot show a session that
    // has already served a request it has not counted.
    let route = {
        let Ok(mut routes) = routes.lock() else {
            return respond(&mut stream, 503, "the broker is busy");
        };
        let Some(route) = authorise(&mut routes, &token, agent.as_deref(), epoch_secs()) else {
            // A closed, expired or invented session — one message for all three.
            return respond(&mut stream, 401, "not a live Keyward session");
        };
        route
    };

    notify(Served {
        secret: route.secret.clone(),
        // Two different questions, two different fields.
        //
        // `caller` answers "who may do this" and must be the kernel's answer —
        // it is what the approval prompt shows and what an "always allow"
        // decision is remembered against.
        //
        // `actor` answers "what should this row say", and there the `User-Agent`
        // is the more useful of the two even though the child chose it. The
        // attested opener is almost always `kw`, because `kw exec` is what opens
        // sessions; a log whose every row reads "kw" tells nobody which of their
        // programs used the key. Trusting it costs nothing: the attested identity
        // is right beside it in the same record.
        actor: agent
            .or_else(|| route.owner.clone())
            .unwrap_or_else(|| "unknown".into()),
        caller: route.owner_label.clone(),
        project: route.project.clone(),
    });

    forward(&mut stream, &request, &route)
}

/// Parse the request line, headers and body, and apply the loopback checks.
fn read_request(stream: &mut TcpStream) -> std::io::Result<Result<Request, String>> {
    let peek = stream.try_clone()?;
    let mut reader = BufReader::new(peek);

    let mut line = String::new();
    reader.read_line(&mut line)?;
    let mut parts = line.split_whitespace();
    let (Some(method), Some(path)) = (parts.next(), parts.next()) else {
        return Ok(Err("malformed request line".into()));
    };
    let method = method.to_owned();
    let path = path.to_owned();

    let mut headers = Vec::new();
    let mut token = None;
    let mut length = 0usize;
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header)? == 0 {
            break;
        }
        let header = header.trim_end();
        if header.is_empty() {
            break;
        }
        let Some((name, value)) = header.split_once(':') else {
            continue;
        };
        let name = name.trim().to_ascii_lowercase();
        let value = value.trim().to_owned();

        match name.as_str() {
            // Browser-only headers. Legitimate SDK traffic carries neither, so
            // their presence means something on this machine is trying to reach
            // the broker from a page (ARCHITECTURE.md §6.1).
            "origin" | "referer" => {
                return Ok(Err("the broker does not serve browser requests".into()));
            }
            // DNS-rebinding defence: a hostile page can resolve a name it owns to
            // 127.0.0.1, but it cannot make the browser send a loopback Host.
            "host" => {
                let host = value.split(':').next().unwrap_or_default();
                if host != "127.0.0.1" && host != "localhost" && host != "[::1]" {
                    return Ok(Err("unexpected Host".into()));
                }
            }
            "authorization" => {
                token = value
                    .strip_prefix("Bearer ")
                    .or_else(|| value.strip_prefix("bearer "))
                    .map(str::to_owned);
                continue;
            }
            "x-api-key" | "api-key" => {
                token = Some(value.clone());
                continue;
            }
            "content-length" => {
                length = value.parse().unwrap_or(0);
            }
            // Hop-by-hop headers must not be forwarded.
            "connection" | "keep-alive" | "transfer-encoding" | "upgrade" => continue,
            _ => {}
        }
        headers.push((name, value));
    }

    let mut body = vec![0u8; length];
    if length > 0 {
        reader.read_exact(&mut body)?;
    }

    Ok(Ok(Request {
        method,
        path,
        headers,
        body,
        token,
    }))
}

/// Send the request upstream with the real credential, and stream the response
/// back byte for byte.
fn forward(stream: &mut TcpStream, request: &Request, route: &Route) -> std::io::Result<()> {
    let url = format!("{}{}", route.upstream, request.path);
    // A 401, 429 or 422 from the upstream is a *response*, not a transport
    // failure. ureq's default turns them into `Err`, and the first version of
    // this function answered them with a Keyward-branded error body — destroying
    // the rate-limit headers, the validation message and the status the client
    // needed. Every real API returns non-2xx eventually, so this made brokering
    // work only against upstreams that never fail.
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .build()
        .into();

    // ureq 3 types the builder by whether the method carries a body, so the two
    // shapes cannot share a variable. Forwarded headers are applied in both.
    let forwarded: Vec<(&str, &str)> = request
        .headers
        .iter()
        .filter(|(name, _)| name != "host" && name != "content-length")
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect();
    let authorization = format!("Bearer {}", route.credential);

    let sent = match request.method.as_str() {
        "POST" | "PUT" | "PATCH" => {
            let mut builder = match request.method.as_str() {
                "PUT" => agent.put(&url),
                "PATCH" => agent.patch(&url),
                _ => agent.post(&url),
            }
            .header("authorization", &authorization);
            for (name, value) in &forwarded {
                builder = builder.header(*name, *value);
            }
            builder.send(request.body.as_slice())
        }
        "GET" | "HEAD" | "DELETE" => {
            let mut builder = match request.method.as_str() {
                "HEAD" => agent.head(&url),
                "DELETE" => agent.delete(&url),
                _ => agent.get(&url),
            }
            .header("authorization", &authorization);
            for (name, value) in &forwarded {
                builder = builder.header(*name, *value);
            }
            builder.call()
        }
        other => {
            return respond(stream, 405, &format!("method {other} is not forwarded"));
        }
    };

    // Only a genuine transport failure becomes a Keyward error. Anything the
    // upstream actually said is forwarded verbatim, status and body included.
    let mut response = match sent {
        Ok(r) => r,
        Err(e) => return respond(stream, 502, &format!("upstream unreachable: {e}")),
    };

    let status = response.status().as_u16();
    let mut head = format!("HTTP/1.1 {status} \r\n");
    for (name, value) in response.headers() {
        let name = name.as_str().to_ascii_lowercase();
        // Re-framed below, so the upstream's framing headers must not survive.
        if matches!(
            name.as_str(),
            "transfer-encoding" | "content-length" | "connection"
        ) {
            continue;
        }
        if let Ok(value) = value.to_str() {
            head.push_str(&format!("{name}: {value}\r\n"));
        }
    }
    // Chunked, so a streaming response (SSE) reaches the client as it arrives
    // rather than being buffered until the upstream closes.
    head.push_str("transfer-encoding: chunked\r\nconnection: close\r\n\r\n");
    stream.write_all(head.as_bytes())?;
    stream.flush()?;

    let mut body = response.body_mut().as_reader();
    let mut chunk = [0u8; 8192];
    // Some APIs reflect the credential back: an echo endpoint, an error that
    // quotes the key it rejected, a settings call that returns the token it just
    // stored. The credential then lands in the client's output and, if the client
    // is a coding agent, in a context window bound for a vendor — the exact
    // outcome brokering exists to prevent, reached through the strongest tier.
    //
    // The broker is the only component that both knows the value and sees these
    // bytes: `kw` cannot scrub what it was never given, which is the whole point
    // of brokering. A real agent session found this by calling an echo endpoint
    // and reading the key straight out of the response.
    let mut redactor = Redactor::new(route.credential.as_bytes());
    loop {
        let n = body.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        let out = redactor.push(chunk.get(..n).unwrap_or_default());
        if !out.is_empty() {
            write_chunk(stream, &out)?;
        }
    }
    let tail = redactor.finish();
    if !tail.is_empty() {
        write_chunk(stream, &tail)?;
    }
    stream.write_all(b"0\r\n\r\n")?;
    stream.flush()
}

fn write_chunk(stream: &mut TcpStream, bytes: &[u8]) -> std::io::Result<()> {
    write!(stream, "{:x}\r\n", bytes.len())?;
    stream.write_all(bytes)?;
    stream.write_all(b"\r\n")?;
    stream.flush()
}

/// Replaces the credential wherever it appears in a response stream.
///
/// Holds back the last `needle.len() - 1` bytes of every chunk, because a match
/// straddling a chunk boundary is the one an unbuffered replacer misses — and it
/// is not a rare case: at 8 KiB chunks a long-running SSE stream crosses a
/// boundary constantly.
struct Redactor {
    needle: Vec<u8>,
    held: Vec<u8>,
}

/// What the client sees instead of the credential. Deliberately not the same
/// length: a reader who sees this knows something was removed, whereas equal-
/// length asterisks read like part of the payload.
const REDACTED: &[u8] = b"[redacted by Keyward]";

impl Redactor {
    fn new(value: &[u8]) -> Self {
        Self {
            needle: value.to_vec(),
            held: Vec::new(),
        }
    }

    /// Feed a chunk, get back the bytes that are safe to emit now.
    fn push(&mut self, chunk: &[u8]) -> Vec<u8> {
        // An empty credential would match everywhere; nothing to do but pass through.
        if self.needle.is_empty() {
            return chunk.to_vec();
        }
        self.held.extend_from_slice(chunk);
        let mut out = Vec::with_capacity(self.held.len());
        let mut i = 0;
        while i < self.held.len() {
            let rest = self.held.get(i..).unwrap_or_default();
            if rest.starts_with(&self.needle) {
                out.extend_from_slice(REDACTED);
                i += self.needle.len();
            } else if rest.len() < self.needle.len() {
                // Might be the start of a match completed by the next chunk.
                break;
            } else {
                out.push(*rest.first().unwrap_or(&0));
                i += 1;
            }
        }
        self.held = self.held.get(i..).unwrap_or_default().to_vec();
        out
    }

    /// The held-back tail, once the upstream has closed and no match can complete.
    fn finish(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.held)
    }
}

fn respond(stream: &mut TcpStream, status: u16, message: &str) -> std::io::Result<()> {
    let body = format!("{{\"error\":{{\"source\":\"keyward\",\"message\":\"{message}\"}}}}");
    write!(
        stream,
        "HTTP/1.1 {status} \r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    )?;
    stream.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Routes are built directly so expiry is tested against a clock the test
    /// controls. Sleeping for a real TTL would make the suite slow and flaky, and
    /// a TTL test that cannot run a year forward is not testing the TTL.
    fn route(expires_at: u64) -> Route {
        Route {
            upstream: "https://api.stripe.com".into(),
            credential: "sk_live_real".into(),
            secret: "stripe".into(),
            opened_at: 1_000,
            expires_at,
            requests: 0,
            actor: None,
            owner: Some("kw".into()),
            owner_label: Some("/opt/homebrew/bin/kw".into()),
            project: Some("my-shop".into()),
        }
    }

    fn routes(expires_at: u64) -> HashMap<String, Route> {
        HashMap::from([("kws_live".to_owned(), route(expires_at))])
    }

    #[test]
    fn the_credential_is_removed_from_a_response_body() {
        let mut r = Redactor::new(b"sk-secret");
        let mut out = r.push(b"{\"key\":\"sk-secret\"}");
        out.extend(r.finish());
        assert_eq!(out, b"{\"key\":\"[redacted by Keyward]\"}".to_vec());
    }

    #[test]
    fn a_credential_split_across_two_chunks_is_still_removed() {
        // The case an unbuffered replacer misses, and the common one: an 8 KiB
        // read boundary lands mid-token sooner or later on any real stream.
        let mut r = Redactor::new(b"sk-secret");
        let mut out = r.push(b"prefix sk-se");
        out.extend(r.push(b"cret suffix"));
        out.extend(r.finish());
        assert_eq!(out, b"prefix [redacted by Keyward] suffix".to_vec());
    }

    #[test]
    fn a_response_that_never_contains_the_credential_passes_through_byte_for_byte() {
        let mut r = Redactor::new(b"sk-secret");
        let mut out = r.push(b"{\"ok\":true}");
        out.extend(r.finish());
        assert_eq!(out, b"{\"ok\":true}".to_vec());
    }

    #[test]
    fn a_trailing_partial_match_is_emitted_rather_than_swallowed() {
        // Held-back bytes that never complete a match must still reach the client;
        // dropping them would truncate every response ending in a near-miss.
        let mut r = Redactor::new(b"sk-secret");
        let mut out = r.push(b"ends with sk-se");
        out.extend(r.finish());
        assert_eq!(out, b"ends with sk-se".to_vec());
    }

    #[test]
    fn a_live_token_is_authorised_and_counted() {
        let mut routes = routes(2_000);
        match authorise(&mut routes, "kws_live", Some("codex/1.0"), 1_500) {
            Some(r) => assert_eq!(r.credential, "sk_live_real"),
            None => unreachable!("a live session must serve"),
        }
        match routes.get("kws_live") {
            Some(r) => {
                assert_eq!(r.requests, 1);
                assert_eq!(r.actor.as_deref(), Some("codex/1.0"));
            }
            None => unreachable!("the route should survive its own request"),
        }
    }

    #[test]
    fn an_expired_token_is_refused_exactly_like_an_invented_one() {
        let mut expired = routes(2_000);
        assert!(
            authorise(&mut expired, "kws_live", None, 2_000).is_none(),
            "the TTL is inclusive: a session is dead at its expiry, not after it"
        );
        let mut empty: HashMap<String, Route> = HashMap::new();
        assert_eq!(
            authorise(&mut empty, "kws_invented", None, 2_000).is_none(),
            authorise(&mut expired, "kws_live", None, 2_500).is_none(),
            "both answers are None, so both callers get the same 401"
        );
    }

    #[test]
    fn refusing_an_expired_token_drops_its_credential_immediately() {
        let mut routes = routes(2_000);
        let _ = authorise(&mut routes, "kws_live", None, 9_999);
        assert!(
            routes.is_empty(),
            "a proven-useless credential must not wait for the sweep"
        );
    }

    #[test]
    fn the_reaper_keeps_the_living_and_drops_the_dead() {
        let mut routes = routes(2_000);
        routes.insert("kws_stale".into(), route(1_200));
        reap(&mut routes, 1_500);
        assert_eq!(routes.len(), 1);
        assert!(routes.contains_key("kws_live"));
    }

    #[test]
    fn an_opener_cannot_ask_for_a_session_that_never_expires() {
        let ttl = u64::MAX.clamp(1, MAX_TTL_SECS);
        assert_eq!(ttl, MAX_TTL_SECS);
        assert_eq!(0u64.clamp(1, MAX_TTL_SECS), 1, "zero must not mean forever");
        assert_eq!(DEFAULT_TTL_SECS.clamp(1, MAX_TTL_SECS), DEFAULT_TTL_SECS);
    }
}
