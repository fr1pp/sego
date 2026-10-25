use std::ffi::OsStr;
use std::fs;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use sego::{Endpoint, Reply, Transport, TransportError};

use crate::case::{Case, Exchange, Request, Served};
use crate::drift::{digest, drift};

module_tests!();

const GRAPHQL_PATH: &str = "/.api/graphql";
const DEADLINE: Duration = Duration::from_secs(90);

/// How `check` reaches the server a response was recorded from, given the
/// endpoint in its stamp.
pub type Connect<'a> = &'a dyn Fn(&str) -> Result<Box<dyn Transport>, String>;

/// What differs between the case's recorded responses and what the server
/// answers to the same requests now. A response without a stamp is not sent
/// for.
pub fn check(case: &Case, connect: Connect) -> Vec<String> {
    let mut failures = Vec::new();
    for (i, exchange) in case.exchanges.iter().enumerate() {
        let Some(stamp) = &exchange.response.stamp else { continue };
        let name = format!("{}: exchange {}", case.path.display(), i + 1);
        let Some(request) = &exchange.request else {
            failures.push(format!(
                "{name} is recorded but has no request to send again"
            ));
            continue;
        };
        let result = connect(&stamp.endpoint).and_then(|transport| {
            let reply = send(transport.as_ref(), request)?;
            compare(case, exchange, stamp.live_values, &reply)
        });
        if let Err(found) = result {
            failures.push(format!("{name} {}: {found}", describe(request)));
        }
    }
    failures
}

fn describe(request: &Request) -> String {
    match request {
        Request::Graphql { operation, .. } => operation.clone(),
        Request::Raw { method, path } => format!("{method} {path}"),
    }
}

fn send(
    transport: &dyn Transport,
    request: &Request,
) -> Result<Reply, String> {
    let deadline = Instant::now() + DEADLINE;
    let reply = match request {
        Request::Graphql { operation, variables, query } => {
            let body = json!({
                "query": query,
                "variables": variables,
                "operationName": operation,
            });
            transport.post(GRAPHQL_PATH, body.to_string().as_bytes(), deadline)
        }
        Request::Raw { path, .. } => transport.get(path, deadline),
    };
    reply.map_err(|e| format!("transport: {e:?}"))
}

fn compare(
    case: &Case,
    exchange: &Exchange,
    values: bool,
    reply: &Reply,
) -> Result<(), String> {
    let Served::Reply { status, .. } = exchange.response.served else {
        return Err("a recorded response is a transport fault".to_owned());
    };
    if reply.status != status {
        return Err(format!("status {}, was {status}", reply.status));
    }
    if matches!(exchange.request, Some(Request::Raw { .. })) {
        compare_raw(case, values, reply)
    } else {
        compare_graphql(exchange, values, reply)
    }
}

fn compare_raw(
    case: &Case,
    values: bool,
    reply: &Reply,
) -> Result<(), String> {
    if !values {
        return Ok(());
    }
    let path = case.body_path();
    let recorded =
        fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let (live, recorded) = (digest(&reply.body), digest(&recorded));
    if live == recorded {
        Ok(())
    } else {
        Err(format!("body {live}, was {recorded}"))
    }
}

fn compare_graphql(
    exchange: &Exchange,
    values: bool,
    reply: &Reply,
) -> Result<(), String> {
    let live: Value = serde_json::from_slice(&reply.body)
        .map_err(|_| format!("body is not JSON ({})", digest(&reply.body)))?;
    let recorded: Value = serde_json::from_str(&exchange.response.body)
        .map_err(|e| format!("the recorded body is not JSON: {e}"))?;
    drift(&recorded, &live, values).map_or(Ok(()), Err)
}

const PAUSE: Duration = Duration::from_secs(1);

static LAST_SENT: Mutex<Option<Instant>> = Mutex::new(None);
static BLOCKED: AtomicBool = AtomicBool::new(false);

/// The real transport to a recording's endpoint, spaced a second apart from
/// the last request of any case, and refusing every request once
/// sourcegraph.com has blocked one.
pub fn connect(endpoint: &str) -> Result<Box<dyn Transport>, String> {
    let endpoint = Endpoint::from_env(Some(OsStr::new(endpoint)))
        .map_err(|e| e.to_string())?;
    Ok(Box::new(Paced(sego::connect(endpoint))))
}

struct Paced(Box<dyn Transport>);

fn pace() -> Result<(), TransportError> {
    let mut last = LAST_SENT.lock().unwrap_or_else(PoisonError::into_inner);
    if BLOCKED.load(Ordering::SeqCst) {
        return Err(TransportError::Network(
            "stopped after sourcegraph.com blocked a request",
        ));
    }
    if let Some(at) = *last {
        thread::sleep(PAUSE.saturating_sub(at.elapsed()));
    }
    *last = Some(Instant::now());
    Ok(())
}

fn watch(
    reply: Result<Reply, TransportError>,
) -> Result<Reply, TransportError> {
    if let Ok(reply) = &reply
        && (reply.status == 429 || reply.cf_mitigated.is_some())
    {
        BLOCKED.store(true, Ordering::SeqCst);
    }
    reply
}

impl Transport for Paced {
    fn host(&self) -> String {
        self.0.host()
    }

    fn post(
        &self,
        path: &str,
        body: &[u8],
        deadline: Instant,
    ) -> Result<Reply, TransportError> {
        pace()?;
        watch(self.0.post(path, body, deadline))
    }

    fn get(
        &self,
        path: &str,
        deadline: Instant,
    ) -> Result<Reply, TransportError> {
        pace()?;
        watch(self.0.get(path, deadline))
    }
}
