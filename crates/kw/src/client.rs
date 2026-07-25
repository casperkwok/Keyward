//! IPC client. Newline-delimited JSON over the daemon's Unix socket
//! (ARCHITECTURE.md §7).
//!
//! The CLI never touches the keychain and holds no privileges of its own. That is
//! the point of routing through the daemon: policy and the usage log live in one
//! place, and this binary is replaceable without weakening either.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

use serde_json::{Value, json};

pub struct Client {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
    next_id: u64,
}

#[derive(Debug)]
pub enum Error {
    NotRunning,
    Io(String),
    Refused { code: String, message: String },
    Malformed(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::NotRunning => write!(
                f,
                "Keyward isn't running. Open the Keyward app, then try again."
            ),
            Error::Io(m) => write!(f, "could not reach Keyward: {m}"),
            Error::Refused { message, .. } => write!(f, "{message}"),
            Error::Malformed(m) => write!(f, "Keyward sent something unexpected: {m}"),
        }
    }
}

impl Error {
    /// Exit code. 77 (`EX_NOPERM`) for a policy refusal specifically, so a script
    /// can tell "you are not allowed" from "it broke" (ARCHITECTURE.md §9).
    ///
    /// Every refusal that means "Keyward decided no" belongs here, including the
    /// ones that arrive after a human said no or failed to answer. A timeout is
    /// still a refusal: the value was not disclosed, and a caller that retried on
    /// exit 1 would be retrying a decision rather than an error.
    pub fn exit_code(&self) -> i32 {
        const REFUSALS: [&str; 4] = [
            "denied",
            "approval_required",
            "approval_denied",
            "approval_timeout",
        ];
        match self {
            Error::Refused { code, .. } if REFUSALS.contains(&code.as_str()) => 77,
            _ => 1,
        }
    }
}

pub fn socket_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(home).join("Library/Application Support/Keyward/keywardd.sock")
}

impl Client {
    pub fn connect() -> Result<Self, Error> {
        let path = socket_path();
        if !path.exists() {
            return Err(Error::NotRunning);
        }
        let stream = UnixStream::connect(&path).map_err(|_| Error::NotRunning)?;
        let writer = stream.try_clone().map_err(|e| Error::Io(e.to_string()))?;
        Ok(Self {
            reader: BufReader::new(stream),
            writer,
            next_id: 1,
        })
    }

    pub fn call(&mut self, method: &str, params: Value) -> Result<Value, Error> {
        let id = self.next_id;
        self.next_id += 1;

        let mut request = json!({"id": id, "method": method});
        if !params.is_null()
            && let Some(object) = request.as_object_mut()
        {
            object.insert("params".into(), params);
        }
        writeln!(self.writer, "{request}").map_err(|e| Error::Io(e.to_string()))?;
        self.writer.flush().map_err(|e| Error::Io(e.to_string()))?;

        let mut line = String::new();
        let read = self
            .reader
            .read_line(&mut line)
            .map_err(|e| Error::Io(e.to_string()))?;
        if read == 0 {
            return Err(Error::Io("the daemon closed the connection".into()));
        }

        let response: Value =
            serde_json::from_str(&line).map_err(|e| Error::Malformed(e.to_string()))?;

        if response.get("ok").and_then(Value::as_bool) == Some(true) {
            return Ok(response.get("result").cloned().unwrap_or(Value::Null));
        }
        let error = response.get("error");
        Err(Error::Refused {
            code: error
                .and_then(|e| e.get("code"))
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_owned(),
            message: error
                .and_then(|e| e.get("message"))
                .and_then(Value::as_str)
                .unwrap_or("Keyward refused the request")
                .to_owned(),
        })
    }
}
