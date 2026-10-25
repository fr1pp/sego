use std::cell::RefCell;
use std::fs;
use std::rc::Rc;
use std::time::Instant;

use serde_json::Value;

use sego::{Reply, Transport, TransportError};

use crate::case::{Case, Exchange, Fault, Request, Response, Served};

const GRAPHQL_PATH: &str = "/.api/graphql";
const CHALLENGE_LEN: usize = 5 * 1024;

/// The transport `run` connects through in a case: it serves the case's
/// exchanges in order, whatever is asked, and logs each request. A request
/// beyond the last exchange gets a network error. Clones share one log.
#[derive(Clone)]
pub struct Replay {
    state: Rc<RefCell<State>>,
}

struct State {
    exchanges: Vec<Exchange>,
    body_path: std::path::PathBuf,
    sent: Vec<Request>,
    problems: Vec<String>,
}

impl Replay {
    pub fn new(case: &Case) -> Replay {
        Replay {
            state: Rc::new(RefCell::new(State {
                exchanges: case.exchanges.clone(),
                body_path: case.body_path(),
                sent: Vec::new(),
                problems: Vec::new(),
            })),
        }
    }

    pub fn sent(&self) -> Vec<Request> {
        self.state.borrow().sent.clone()
    }

    /// What the transport could not make sense of: a request that is not
    /// GraphQL, a missing body file.
    pub fn problems(&self) -> Vec<String> {
        self.state.borrow().problems.clone()
    }

    fn serve(&self, request: Request) -> Result<Reply, TransportError> {
        let mut state = self.state.borrow_mut();
        let at = state.sent.len();
        let raw = matches!(request, Request::Raw { .. });
        state.sent.push(request);
        let Some(exchange) = state.exchanges.get(at) else {
            return Err(TransportError::Network("no exchange left"));
        };
        let response = exchange.response.clone();
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
                let body = if raw {
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
        _deadline: Instant,
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
        self.serve(Request::Graphql {
            operation: text("operationName"),
            variables: sent
                .as_ref()
                .and_then(|sent| sent.get("variables"))
                .cloned()
                .unwrap_or(Value::Null),
            query: text("query"),
        })
    }

    fn get(
        &self,
        path: &str,
        _deadline: Instant,
    ) -> Result<Reply, TransportError> {
        self.serve(Request::Raw {
            method: "GET".to_owned(),
            path: path.to_owned(),
        })
    }
}
