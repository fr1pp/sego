use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::time::Instant;

use serde_json::Value;

use sego::{Response, Transport, TransportError};

use super::Logged;
use super::case::Case;
use super::recording::{Request, ServerResponse, Store};

const GRAPHQL_PATH: &str = "/.api/graphql";

/// A request that reached a transport, and the deadline it was sent with.
#[derive(Clone, Debug)]
pub struct Sent {
    pub request: Request,
    pub deadline: Instant,
}

/// The transport `run` connects through in a case: it answers a case with
/// responses with them in order, whatever is asked, and any other from the
/// recordings, and logs each request. A request it can't answer gets a network
/// error. Clones share one log.
#[derive(Clone)]
pub struct Replay {
    state: Rc<RefCell<State>>,
}

struct State {
    responses: VecDeque<ServerResponse>,
    store: Option<Store>,
    served: Vec<ServerResponse>,
    sent: Vec<Sent>,
    problems: Vec<String>,
}

impl Replay {
    pub fn new(case: &Case) -> Replay {
        let replay = Replay::serving(case.responses.clone());
        if case.responses.is_empty() {
            replay.state.borrow_mut().store = Some(Store::default());
        }
        replay
    }

    /// A replay that serves `responses` in order.
    pub fn serving(responses: Vec<ServerResponse>) -> Replay {
        let state = State {
            responses: responses.into(),
            store: None,
            served: Vec::new(),
            sent: Vec::new(),
            problems: Vec::new(),
        };
        Replay { state: Rc::new(RefCell::new(state)) }
    }

    fn serve(&self, request: &Request, deadline: Instant) -> Result<Response, TransportError> {
        let mut state = self.state.borrow_mut();
        state.sent.push(Sent { request: request.clone(), deadline });
        let found = match (state.responses.pop_front(), &state.store) {
            (Some(response), _) => Ok(response),
            (None, Some(store)) => store.find(request),
            (None, None) => return Err(TransportError::Network("no response left")),
        };
        match found {
            Ok(response) => {
                let served = response.served();
                state.served.push(response);
                Ok(served)
            }
            Err(problem) => {
                state.problems.push(problem);
                Err(TransportError::Network("the request can't be answered"))
            }
        }
    }
}

impl Logged for Replay {
    fn sent(&self) -> Vec<Sent> {
        self.state.borrow().sent.clone()
    }

    /// A POST that isn't GraphQL, or a request with no recording.
    fn problems(&self) -> Vec<String> {
        self.state.borrow().problems.clone()
    }

    fn served(&self) -> Vec<ServerResponse> {
        self.state.borrow().served.clone()
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
        self.serve(&request, deadline)
    }

    fn get(&self, path: &str, deadline: Instant) -> Result<Response, TransportError> {
        self.serve(&Request::Raw { path: path.to_owned() }, deadline)
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
