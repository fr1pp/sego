use std::ffi::OsStr;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Instant;

use serde_json::Value;

use sego::internal::upstream::{
    DEADLINE, Endpoint, GRAPHQL_PATH, Reply, Transport, TransportError, Ureq,
};

use super::fixture::{Fault, Fixture};
use super::http::{Request, Response, Server};

const LARGE_BODY: usize = 1_000_000;
const QUERY_NAME_LEN: usize = 80;
const CHALLENGE_LEN: usize = 5 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Replay,
    RecordMissing,
    Refresh,
}

impl Mode {
    pub fn from_env() -> Mode {
        match std::env::var("SEGO_RECORD").as_deref() {
            Ok("" | "0") | Err(_) => Mode::Replay,
            Ok("missing") => Mode::RecordMissing,
            Ok("refresh") => Mode::Refresh,
            Ok(other) => {
                panic!("SEGO_RECORD must be missing or refresh, not {other}")
            }
        }
    }
}

enum Query {
    Graphql { operation: String, variables: Value, query: String },
    Raw { method: String, path: String },
}

impl Query {
    fn parse(method: &str, path: &str, body: &[u8]) -> Result<Query, String> {
        if method == "GET" {
            return Ok(Query::Raw {
                method: method.to_owned(),
                path: path.to_owned(),
            });
        }
        if method != "POST" || path != GRAPHQL_PATH {
            return Err(format!("no fixture for {method} {path}"));
        }
        let sent: Value = serde_json::from_slice(body)
            .map_err(|e| format!("the request body is not JSON: {e}"))?;
        let text = |key: &str| {
            sent.get(key)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        Ok(Query::Graphql {
            operation: text("operationName"),
            variables: sent.get("variables").cloned().unwrap_or(Value::Null),
            query: text("query"),
        })
    }

    fn matches(&self, fixture: &Fixture) -> bool {
        match self {
            Query::Graphql { operation, variables, .. } => {
                fixture.operation.as_ref() == Some(operation)
                    && fixture.variables.as_ref().unwrap_or(&Value::Null)
                        == variables
            }
            Query::Raw { method, path } => {
                fixture.request.as_ref().is_some_and(|request| {
                    request.method == *method && request.path == *path
                })
            }
        }
    }
}

fn plain(status: u16, text: String) -> Reply {
    Reply {
        status,
        content_type: Some("text/plain".to_owned()),
        location: None,
        retry_after: None,
        cf_mitigated: None,
        body: text.into_bytes(),
    }
}

fn normalise(query: &str) -> String {
    query.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn header(fixture: &Fixture, name: &str) -> Option<String> {
    fixture
        .headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.clone())
}

fn from_fixture(fixture: &Fixture, body: Vec<u8>) -> Reply {
    Reply {
        status: fixture.status,
        content_type: header(fixture, "content-type"),
        location: header(fixture, "location"),
        retry_after: header(fixture, "retry-after"),
        cf_mitigated: header(fixture, "cf-mitigated"),
        body,
    }
}

fn serve(fixture: &Fixture, query: &Query) -> Result<Reply, TransportError> {
    if let (Query::Graphql { query: sent, .. }, Some(stored)) =
        (query, &fixture.query)
        && normalise(stored) != normalise(sent)
    {
        return Ok(plain(
            500,
            format!(
                "stale fixture {}: query changed\n--- fixture\n{}\n--- \
                 sent\n{}",
                fixture.file.display(),
                normalise(stored),
                normalise(sent)
            ),
        ));
    }
    match fixture.fault {
        Some(Fault::Network) => {
            Err(TransportError::Network("connection closed"))
        }
        Some(Fault::HtmlChallenge) => {
            let mut body = b"<html><body>".to_vec();
            body.resize(CHALLENGE_LEN - b"</body></html>".len(), b'.');
            body.extend_from_slice(b"</body></html>");
            Ok(Reply {
                status: 429,
                content_type: Some("text/html".to_owned()),
                location: None,
                retry_after: None,
                cf_mitigated: Some("challenge".to_owned()),
                body,
            })
        }
        Some(Fault::Redirect) => {
            Ok(Reply { status: 302, ..from_fixture(fixture, Vec::new()) })
        }
        Some(Fault::Status) => Ok(from_fixture(fixture, Vec::new())),
        None => Ok(from_fixture(fixture, fixture.body())),
    }
}

fn missing(fixtures: &[Fixture], query: &Query) -> Reply {
    let (what, candidates): (String, Vec<&Fixture>) = match query {
        Query::Graphql { operation, variables, .. } => (
            format!("{operation} {variables}"),
            fixtures
                .iter()
                .filter(|f| f.operation.as_ref() == Some(operation))
                .collect(),
        ),
        Query::Raw { method, path } => (
            format!("{method} {path}"),
            fixtures.iter().filter(|f| f.request.is_some()).collect(),
        ),
    };
    let mut message = format!("no fixture for {what}");
    if candidates.is_empty() {
        message.push_str("\nthere are no fixtures of that kind");
    } else {
        message.push_str("\nfixtures of that kind:");
        for fixture in candidates {
            let _ = write!(
                message,
                "
  {}: {}",
                fixture.file.display(),
                fixture.key()
            );
        }
    }
    plain(500, message)
}

/// The matching core: the HTTP server and in-process fakes both call it.
pub fn answer(
    fixtures: &[Fixture],
    method: &str,
    path: &str,
    body: &[u8],
) -> Result<Reply, TransportError> {
    let query = match Query::parse(method, path, body) {
        Ok(query) => query,
        Err(message) => return Ok(plain(500, message)),
    };
    match fixtures.iter().find(|f| query.matches(f)) {
        Some(fixture) => serve(fixture, &query),
        None => Ok(missing(fixtures, &query)),
    }
}

type Upstream = Box<dyn Transport + Send + Sync>;

struct State {
    fixtures: Vec<Fixture>,
    mode: Mode,
    root: PathBuf,
    upstream: Upstream,
}

impl State {
    fn respond(&mut self, request: &Request) -> Result<Reply, TransportError> {
        let query = match Query::parse(
            &request.method,
            &request.path,
            &request.body,
        ) {
            Ok(query) => query,
            Err(message) => return Ok(plain(500, message)),
        };
        let hit = self.fixtures.iter().position(|f| query.matches(f));
        let forward = match (self.mode, hit) {
            (Mode::Replay, _) | (Mode::RecordMissing, Some(_)) => false,
            (Mode::RecordMissing | Mode::Refresh, None) => true,
            (Mode::Refresh, Some(i)) => !self.fixtures[i].handwritten,
        };
        if forward {
            let recorded = match record(
                &self.root,
                &*self.upstream,
                &self.fixtures,
                hit,
                &query,
                &request.body,
            ) {
                Ok(recorded) => recorded,
                Err(message) => return Ok(plain(500, message)),
            };
            if let Some(i) = hit {
                self.fixtures[i] = recorded;
            } else {
                self.fixtures.push(recorded);
            }
        }
        answer(&self.fixtures, &request.method, &request.path, &request.body)
    }
}

fn record(
    root: &Path,
    upstream: &dyn Transport,
    fixtures: &[Fixture],
    existing: Option<usize>,
    query: &Query,
    body: &[u8],
) -> Result<Fixture, String> {
    let deadline = Instant::now() + DEADLINE;
    let reply = match query {
        Query::Graphql { .. } => upstream.post(GRAPHQL_PATH, body, deadline),
        Query::Raw { path, .. } => upstream.get(path, deadline),
    }
    .map_err(|e| format!("recording failed: {e:?}"))?;
    if is_error_reply(&reply) {
        let mut message = format!(
            "recording failed: upstream answered HTTP {}",
            reply.status
        );
        if let Some(value) = &reply.cf_mitigated {
            let _ = write!(message, " (cf-mitigated: {value})");
        }
        return Err(message);
    }

    let mut fixture = existing.map_or_else(Fixture::default, |i| {
        let old = &fixtures[i];
        Fixture { pinned: old.pinned, ..Fixture::default() }
    });
    fixture.status = reply.status;
    for (name, value) in [
        ("content-type", &reply.content_type),
        ("location", &reply.location),
        ("retry-after", &reply.retry_after),
        ("cf-mitigated", &reply.cf_mitigated),
    ] {
        if let Some(value) = value {
            fixture.headers.insert(name.to_owned(), value.clone());
        }
    }
    let dir = match query {
        Query::Graphql { operation, variables, query } => {
            fixture.operation = Some(operation.clone());
            fixture.variables =
                (!variables.is_null()).then(|| variables.clone());
            fixture.query = Some(query.clone());
            operation.clone()
        }
        Query::Raw { method, path } => {
            fixture.request = Some(super::fixture::RawRequest {
                method: method.clone(),
                path: path.clone(),
            });
            "raw".to_owned()
        }
    };

    let file = match existing {
        Some(i) => fixtures[i].file.clone(),
        None => root.join(&dir).join(format!(
            "{}.yaml",
            unique_name(root, &dir, fixtures, query)
        )),
    };
    fixture.file.clone_from(&file);

    let large = reply.body.len() > LARGE_BODY;
    let json = matches!(query, Query::Graphql { .. })
        .then(|| serde_json::from_slice::<Value>(&reply.body).ok())
        .flatten()
        .map(|mut value| {
            strip(&mut value);
            value
        });
    let mut external = None;
    match json {
        Some(value) if !large => fixture.response = Some(value),
        Some(value) => external = Some(value.to_string().into_bytes()),
        None => match String::from_utf8(reply.body) {
            Ok(text) if !large => fixture.response = Some(Value::String(text)),
            Ok(text) => external = Some(text.into_bytes()),
            Err(error) => external = Some(error.into_bytes()),
        },
    }
    if let Some(bytes) = external {
        let body_file = file.with_extension("body");
        fixture.body_file = body_file
            .file_name()
            .map(|name| name.to_string_lossy().into_owned());
        write(&body_file, &bytes)?;
    }
    write(&file, fixture.to_yaml().as_bytes())?;
    Ok(fixture)
}

/// Redirects, rate limits, server errors and Cloudflare challenges describe
/// the moment of recording, not the API; other 4xx replies are the API's own
/// answers.
fn is_error_reply(reply: &Reply) -> bool {
    matches!(reply.status, 300..=399 | 429 | 500..=599)
        || reply.cf_mitigated.is_some()
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let write = || -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, bytes)
    };
    write().map_err(|e| format!("writing {}: {e}", path.display()))
}

fn strip(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.remove("extensions");
            map.remove("requestId");
            map.values_mut().for_each(strip);
        }
        Value::Array(items) => items.iter_mut().for_each(strip),
        _ => {}
    }
}

fn slug(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('_') {
            out.push('_');
        }
    }
    out.trim_matches('_').to_owned()
}

fn first_value(variables: &Value, key: &str) -> Option<String> {
    match variables.get(key)? {
        Value::String(text) => Some(text.clone()),
        Value::Array(items) => items.first()?.as_str().map(str::to_owned),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

fn unique_name(
    root: &Path,
    dir: &str,
    fixtures: &[Fixture],
    query: &Query,
) -> String {
    let name = match query {
        Query::Graphql { variables, .. }
            if variables.as_object().is_some_and(|map| map.len() == 1)
                && first_value(variables, "query").is_some() =>
        {
            let name =
                slug(&first_value(variables, "query").unwrap_or_default());
            name.chars().take(QUERY_NAME_LEN).collect()
        }
        Query::Graphql { variables, .. } => {
            ["repo0", "rev0", "path_0", "query"]
                .iter()
                .filter_map(|key| {
                    let part = slug(&first_value(variables, key)?);
                    Some(part.chars().take(QUERY_NAME_LEN).collect::<String>())
                })
                .collect::<Vec<_>>()
                .join("--")
        }
        Query::Raw { path, .. } => slug(path),
    };
    let name = if name.is_empty() { "fixture".to_owned() } else { name };
    let taken = |candidate: &str| {
        let file = root.join(dir).join(format!("{candidate}.yaml"));
        file.exists() || fixtures.iter().any(|f| f.file == file)
    };
    let mut candidate = name.clone();
    let mut n = 2;
    while taken(&candidate) {
        candidate = format!("{name}-{n}");
        n += 1;
    }
    candidate
}

pub struct Replay {
    server: Server,
    seen: AtomicUsize,
}

impl Replay {
    pub fn start(fixtures: Vec<Fixture>, mode: Mode) -> Replay {
        let endpoint = Endpoint::from_env(None::<&OsStr>)
            .unwrap_or_else(|e| panic!("{e}"));
        Replay::start_recording(
            fixtures,
            mode,
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures"),
            Box::new(Ureq::new(endpoint)),
        )
    }

    /// Recording asks one request at a time because the handler holds the
    /// state lock while it forwards.
    pub fn start_recording(
        fixtures: Vec<Fixture>,
        mode: Mode,
        root: PathBuf,
        upstream: Upstream,
    ) -> Replay {
        let state =
            Arc::new(Mutex::new(State { fixtures, mode, root, upstream }));
        let server = Server::start(move |request| {
            let mut state =
                state.lock().unwrap_or_else(PoisonError::into_inner);
            match state.respond(&request) {
                Ok(reply) => response(reply),
                Err(_) => Response::drop_connection(),
            }
        });
        Replay { server, seen: AtomicUsize::new(0) }
    }

    pub fn url(&self) -> String {
        self.server.url()
    }

    pub fn take_requests(&self) -> usize {
        let total = self.server.requests().len();
        total - self.seen.swap(total, Ordering::SeqCst)
    }
}

fn response(reply: Reply) -> Response {
    let mut response = Response::new(reply.status).body(reply.body);
    for (name, value) in [
        ("content-type", reply.content_type),
        ("location", reply.location),
        ("retry-after", reply.retry_after),
        ("cf-mitigated", reply.cf_mitigated),
    ] {
        if let Some(value) = value {
            response = response.header(name, &value);
        }
    }
    response
}
