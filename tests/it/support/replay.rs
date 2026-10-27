use std::cell::RefCell;
use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Instant;

use serde_json::Value;

use sego::{Response, Transport, TransportError};

use super::Logged;
use super::case::{Body, Case, Exchange, Request};

const GRAPHQL_PATH: &str = "/.api/graphql";

/// A request that reached a transport, and the deadline it was sent with.
#[derive(Clone, Debug)]
pub struct Sent {
    pub request: Request,
    pub deadline: Instant,
}

/// The transport `run` connects through in a case: it serves the case's
/// exchanges in order, whatever is asked, and logs each request. A request
/// beyond the last exchange gets a network error. Clones share one log.
#[derive(Clone)]
pub struct Replay {
    state: Rc<RefCell<State>>,
}

struct State {
    exchanges: VecDeque<Exchange>,
    dir: PathBuf,
    sent: Vec<Sent>,
    problems: Vec<String>,
}

impl Replay {
    pub fn new(case: &Case) -> Replay {
        Replay::serving(case, case.exchanges.clone())
    }

    /// A replay of `case` that serves `exchanges` in place of its own.
    pub fn serving(case: &Case, exchanges: Vec<Exchange>) -> Replay {
        let dir = case.dir().to_path_buf();
        let state =
            State { exchanges: exchanges.into(), dir, sent: Vec::new(), problems: Vec::new() };
        Replay { state: Rc::new(RefCell::new(state)) }
    }

    fn serve(&self, request: Request, deadline: Instant) -> Result<Response, TransportError> {
        let mut state = self.state.borrow_mut();
        state.sent.push(Sent { request, deadline });
        let exchange =
            state.exchanges.pop_front().ok_or(TransportError::Network("no exchange left"))?;
        response(&exchange, &state.dir).map_err(|problem| {
            state.problems.push(problem);
            TransportError::Network("the exchange can't be served")
        })
    }
}

impl Logged for Replay {
    fn sent(&self) -> Vec<Sent> {
        self.state.borrow().sent.clone()
    }

    /// A POST that isn't GraphQL, a body file that can't be read.
    fn problems(&self) -> Vec<String> {
        self.state.borrow().problems.clone()
    }
}

impl Transport for Replay {
    fn post(&self, path: &str, body: &[u8], deadline: Instant) -> Result<Response, TransportError> {
        let request = graphql(path, body).unwrap_or_else(|problem| {
            self.state.borrow_mut().problems.push(problem);
            Request::Graphql {
                operation: String::new(),
                variables: Value::Null,
                query: String::new(),
            }
        });
        self.serve(request, deadline)
    }

    fn get(&self, path: &str, deadline: Instant) -> Result<Response, TransportError> {
        self.serve(Request::Raw { path: path.to_owned() }, deadline)
    }
}

/// The GraphQL request that a POST of `body` to `path` sends.
pub fn graphql(path: &str, body: &[u8]) -> Result<Request, String> {
    let sent: Option<Value> = serde_json::from_slice(body).ok();
    let Some(sent) = sent.filter(|_| path == GRAPHQL_PATH) else {
        return Err(format!(
            "POST {path} is not a GraphQL request: {}",
            String::from_utf8_lossy(body)
        ));
    };
    let text = |key: &str| sent.get(key).and_then(Value::as_str).unwrap_or_default().to_owned();
    Ok(Request::Graphql {
        operation: text("operationName"),
        variables: sent.get("variables").cloned().unwrap_or(Value::Null),
        query: text("query"),
    })
}

/// The response an exchange serves: a response's JSON compact as the server
/// sends it, other text as written, or the body file in `dir` that it names.
pub fn response(exchange: &Exchange, dir: &Path) -> Result<Response, String> {
    let body = match &exchange.body {
        Body::Text(text) => match serde_json::from_str::<Value>(text) {
            Ok(value) => value.to_string().into_bytes(),
            Err(_) => text.clone().into_bytes(),
        },
        Body::File(name) => {
            let path = dir.join(name);
            fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?
        }
    };
    Ok(response_of(exchange.status, exchange.content_type.as_deref(), body))
}

/// A response with only the status, content type and body set.
pub fn response_of(status: u16, content_type: Option<&str>, body: impl Into<Vec<u8>>) -> Response {
    Response {
        status,
        content_type: content_type.map(str::to_owned),
        location: None,
        retry_after: None,
        cf_mitigated: None,
        body: body.into(),
    }
}

/// A transport that answers each request with the next of its answers, and
/// a network error once they run out, logging the method of each request.
#[derive(Clone)]
pub struct Answers {
    state: Rc<RefCell<AnswersState>>,
}

struct AnswersState {
    answers: VecDeque<Result<Response, TransportError>>,
    methods: Vec<&'static str>,
}

impl Answers {
    pub fn new(answers: Vec<Result<Response, TransportError>>) -> Answers {
        let state = AnswersState { answers: answers.into(), methods: Vec::new() };
        Answers { state: Rc::new(RefCell::new(state)) }
    }

    pub fn methods(&self) -> Vec<&'static str> {
        self.state.borrow().methods.clone()
    }

    fn answer(&self, method: &'static str) -> Result<Response, TransportError> {
        let mut state = self.state.borrow_mut();
        state.methods.push(method);
        state.answers.pop_front().unwrap_or(Err(TransportError::Network("no answer left")))
    }
}

impl Transport for Answers {
    fn post(&self, _: &str, _: &[u8], _: Instant) -> Result<Response, TransportError> {
        self.answer("POST")
    }

    fn get(&self, _: &str, _: Instant) -> Result<Response, TransportError> {
        self.answer("GET")
    }
}
