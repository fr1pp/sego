use std::cell::RefCell;
use std::fs;
use std::rc::Rc;
use std::time::Instant;

use serde_json::Value;

use sego::{Reply, Transport, TransportError};

use crate::case::{Case, Exchange, Fault, Request, Response, Served};
use crate::record::{Recorded, Recorder};

const GRAPHQL_PATH: &str = "/.api/graphql";
const CHALLENGE_LEN: usize = 5 * 1024;

/// The transport `run` connects through in a case: it serves the case's
/// exchanges in order, whatever is asked, and logs each request. A request
/// beyond the last exchange gets a network error, unless the replay records.
/// Clones share one log.
#[derive(Clone)]
pub struct Replay {
    state: Rc<RefCell<State>>,
}

struct State {
    exchanges: Vec<Exchange>,
    path: std::path::PathBuf,
    body_path: std::path::PathBuf,
    sent: Vec<Request>,
    problems: Vec<String>,
    recorder: Option<Recorder>,
    recorded: Vec<(usize, Recorded)>,
}

/// Sends the request being served to the upstream, as `run` sent it.
type Forward<'a> = &'a dyn Fn(&dyn Transport) -> Result<Reply, TransportError>;

impl Replay {
    pub fn new(case: &Case) -> Replay {
        Replay {
            state: Rc::new(RefCell::new(State {
                exchanges: case.exchanges.clone(),
                path: case.path.clone(),
                body_path: case.body_path(),
                sent: Vec::new(),
                problems: Vec::new(),
                recorder: None,
                recorded: Vec::new(),
            })),
        }
    }

    /// A replay that forwards through `recorder` a request with no exchange,
    /// or whose recorded exchange answered a different request.
    pub fn recording(case: &Case, recorder: Recorder) -> Replay {
        let replay = Replay::new(case);
        replay.state.borrow_mut().recorder = Some(recorder);
        replay
    }

    /// The exchanges recorded, with the position of each in the case.
    pub fn recorded(&self) -> Vec<(usize, Recorded)> {
        self.state.borrow().recorded.clone()
    }

    pub fn sent(&self) -> Vec<Request> {
        self.state.borrow().sent.clone()
    }

    /// What the transport could not make sense of: a request that is not
    /// GraphQL, a missing body file.
    pub fn problems(&self) -> Vec<String> {
        self.state.borrow().problems.clone()
    }

    fn serve(
        &self,
        request: &Request,
        forward: Forward<'_>,
    ) -> Result<Reply, TransportError> {
        let mut state = self.state.borrow_mut();
        let at = state.sent.len();
        let raw = matches!(request, Request::Raw { .. });
        state.sent.push(request.clone());
        let (exchange, recorded_body) =
            state.exchange(at, request, forward)?;
        let response = exchange.response;
        match response.served {
            Served::Fault(Fault::Network) => {
                Err(TransportError::Network("connection closed"))
            }
            Served::Fault(Fault::HtmlChallenge) => Ok(challenge()),
            Served::Fault(Fault::Redirect) => Ok(Reply {
                status: 302,
                content_type: None,
                location: None,
                retry_after: None,
                cf_mitigated: None,
                body: Vec::new(),
            }),
            Served::Reply { status, ref content_type } => {
                let body = if let Some(body) = recorded_body {
                    body
                } else if raw {
                    read_body(&mut state)
                } else {
                    json_bytes(&response)
                };
                Ok(Reply {
                    status,
                    content_type: content_type.clone(),
                    location: None,
                    retry_after: None,
                    cf_mitigated: None,
                    body,
                })
            }
        }
    }
}

impl State {
    /// The exchange that answers request number `at`, and the body it was
    /// just recorded with if it is a raw read.
    fn exchange(
        &mut self,
        at: usize,
        request: &Request,
        forward: Forward<'_>,
    ) -> Result<(Exchange, Option<Vec<u8>>), TransportError> {
        let written = self.exchanges.get(at);
        let Some(recorder) = &self.recorder else {
            return written
                .map(|exchange| (exchange.clone(), None))
                .ok_or(TransportError::Network("no exchange left"));
        };
        if let Some(written) = written
            && !answers_another_request(written, request)
        {
            return Ok((written.clone(), None));
        }
        match recorder.record(request, forward) {
            Ok(found) => {
                let served = (found.exchange.clone(), found.raw_body.clone());
                self.recorded.push((at, found));
                Ok(served)
            }
            Err(message) => {
                self.problems
                    .push(format!("{}: {message}", self.path.display()));
                Err(TransportError::Network("recording refused"))
            }
        }
    }
}

/// A recorded response is kept only for the request it answered.
fn answers_another_request(exchange: &Exchange, request: &Request) -> bool {
    exchange.response.stamp.is_some()
        && exchange
            .request
            .as_ref()
            .is_some_and(|written| !written.same_as(request))
}

fn read_body(state: &mut State) -> Vec<u8> {
    match fs::read(&state.body_path) {
        Ok(bytes) => bytes,
        Err(e) => {
            state.problems.push(format!("{}: {e}", state.body_path.display()));
            Vec::new()
        }
    }
}

/// The JSON under a response as the server sent it; text that is not JSON is
/// served as written.
fn json_bytes(response: &Response) -> Vec<u8> {
    match serde_json::from_str::<Value>(&response.body) {
        Ok(value) => value.to_string().into_bytes(),
        Err(_) => response.body.clone().into_bytes(),
    }
}

fn challenge() -> Reply {
    let mut body = b"<html><body>".to_vec();
    body.resize(CHALLENGE_LEN - b"</body></html>".len(), b'.');
    body.extend_from_slice(b"</body></html>");
    Reply {
        status: 429,
        content_type: Some("text/html".to_owned()),
        location: None,
        retry_after: None,
        cf_mitigated: Some("challenge".to_owned()),
        body,
    }
}

impl Transport for Replay {
    fn post(
        &self,
        path: &str,
        body: &[u8],
        deadline: Instant,
    ) -> Result<Reply, TransportError> {
        let sent: Option<Value> = serde_json::from_slice(body).ok();
        let text = |key: &str| {
            sent.as_ref()
                .and_then(|sent| sent.get(key))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        if path != GRAPHQL_PATH || sent.is_none() {
            self.state.borrow_mut().problems.push(format!(
                "POST {path} is not a GraphQL request: {}",
                String::from_utf8_lossy(body)
            ));
        }
        let request = Request::Graphql {
            operation: text("operationName"),
            variables: sent
                .as_ref()
                .and_then(|sent| sent.get("variables"))
                .cloned()
                .unwrap_or(Value::Null),
            query: text("query"),
        };
        self.serve(&request, &|upstream| upstream.post(path, body, deadline))
    }

    fn get(
        &self,
        path: &str,
        deadline: Instant,
    ) -> Result<Reply, TransportError> {
        let request =
            Request::Raw { method: "GET".to_owned(), path: path.to_owned() };
        self.serve(&request, &|upstream| upstream.get(path, deadline))
    }
}
