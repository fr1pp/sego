use std::cell::RefCell;
use std::collections::VecDeque;
use std::fs;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Instant;

use serde_json::Value;

use sego::{Reply, Transport, TransportError};

use crate::case::{Case, Exchange, Fault, Request, Response, Served};
use crate::record::{Recorded, Recorder};

const GRAPHQL_PATH: &str = "/.api/graphql";
const CHALLENGE_LEN: usize = 5 * 1024;

/// A reply with only the status, content type and body a test sets.
pub fn reply(
    status: u16,
    content_type: Option<&str>,
    body: impl AsRef<[u8]>,
) -> Reply {
    Reply {
        status,
        content_type: content_type.map(str::to_owned),
        location: None,
        retry_after: None,
        cf_mitigated: None,
        body: body.as_ref().to_vec(),
    }
}

/// A request that reached the transport.
#[derive(Clone, Debug)]
pub struct Call {
    pub(crate) method: &'static str,
    pub deadline: Instant,
}

/// The transport `run` connects through in a case: it serves the case's
/// exchanges in order, whatever is asked, and logs each request. A request
/// beyond the last exchange gets a network error, unless the replay records.
/// A replay made with `answering` serves the replies and errors it was given
/// instead. Clones share one log.
#[derive(Clone)]
pub struct Replay {
    state: Rc<RefCell<State>>,
}

#[derive(Default)]
struct State {
    exchanges: Vec<Exchange>,
    path: PathBuf,
    body_path: PathBuf,
    answers: Option<VecDeque<Result<Reply, TransportError>>>,
    sent: Vec<Request>,
    calls: Vec<Call>,
    problems: Vec<String>,
    recorder: Option<Recorder>,
    recorded: Vec<(usize, Recorded)>,
}

/// Sends the request being served to the upstream, as `run` sent it.
type Forward<'a> = &'a dyn Fn(&dyn Transport) -> Result<Reply, TransportError>;

impl Replay {
    pub(crate) fn new(case: &Case) -> Replay {
        Replay::serving(case, case.exchanges.clone())
    }

    /// A replay of `case` that serves `exchanges` in its place.
    pub(crate) fn serving(case: &Case, exchanges: Vec<Exchange>) -> Replay {
        Replay::with(State {
            exchanges,
            path: case.path.clone(),
            body_path: case.body_path(),
            ..State::default()
        })
    }

    /// A replay that answers each request with the next of `answers`, and a
    /// network error once they run out.
    pub fn answering(answers: Vec<Result<Reply, TransportError>>) -> Replay {
        Replay::with(State {
            answers: Some(answers.into()),
            ..State::default()
        })
    }

    fn with(state: State) -> Replay {
        Replay { state: Rc::new(RefCell::new(state)) }
    }

    /// A replay that forwards through `recorder` a request with no exchange,
    /// or whose recorded exchange answered a different request.
    pub(crate) fn recording(case: &Case, recorder: Recorder) -> Replay {
        let replay = Replay::new(case);
        replay.state.borrow_mut().recorder = Some(recorder);
        replay
    }

    /// The exchanges recorded, with the position of each in the case.
    pub(crate) fn recorded(&self) -> Vec<(usize, Recorded)> {
        self.state.borrow().recorded.clone()
    }

    pub(crate) fn sent(&self) -> Vec<Request> {
        self.state.borrow().sent.clone()
    }

    pub fn calls(&self) -> Vec<Call> {
        self.state.borrow().calls.clone()
    }

    pub fn methods(&self) -> Vec<&'static str> {
        self.calls().iter().map(|call| call.method).collect()
    }

    /// What the transport could not make sense of: a request that is not
    /// GraphQL, a missing body file.
    pub(crate) fn problems(&self) -> Vec<String> {
        self.state.borrow().problems.clone()
    }

    fn log(&self, method: &'static str, deadline: Instant) {
        self.state.borrow_mut().calls.push(Call { method, deadline });
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
        if let Some(answers) = &mut state.answers {
            return answers
                .pop_front()
                .unwrap_or(Err(TransportError::Network("no reply left")));
        }
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
            Served::Reply { status, ref content_type, ref headers } => {
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
                    location: headers.location.clone(),
                    retry_after: headers.retry_after.clone(),
                    cf_mitigated: headers.cf_mitigated.clone(),
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
        let live_values = written
            .and_then(|written| written.response.stamp.as_ref())
            .is_some_and(|stamp| stamp.live_values);
        match recorder.record(request, forward) {
            Ok(mut found) => {
                if let Some(stamp) = &mut found.exchange.response.stamp {
                    stamp.live_values = live_values;
                }
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
        self.log("POST", deadline);
        self.serve(&request, &|upstream| upstream.post(path, body, deadline))
    }

    fn get(
        &self,
        path: &str,
        deadline: Instant,
    ) -> Result<Reply, TransportError> {
        let request =
            Request::Raw { method: "GET".to_owned(), path: path.to_owned() };
        self.log("GET", deadline);
        self.serve(&request, &|upstream| upstream.get(path, deadline))
    }
}
