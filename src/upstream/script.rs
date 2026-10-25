use std::cell::RefCell;
use std::collections::VecDeque;
use std::time::Instant;

use serde_json::Value;

use super::{Reply, Transport, TransportError};

/// A reply with only the headers a test sets.
pub fn reply(status: u16, content_type: Option<&str>, body: &str) -> Reply {
    Reply {
        status,
        content_type: content_type.map(str::to_owned),
        location: None,
        retry_after: None,
        cf_mitigated: None,
        body: body.as_bytes().to_vec(),
    }
}

/// A 200 reply carrying `body` as JSON.
pub fn json_reply(body: &Value) -> Reply {
    reply(200, Some("application/json"), &body.to_string())
}

pub struct Call {
    pub method: &'static str,
    pub path: String,
    pub deadline: Instant,
}

/// A transport that answers each request with the next queued result and logs
/// the requests it gets. An empty queue is a network error.
pub struct Script {
    host: String,
    replies: RefCell<VecDeque<Result<Reply, TransportError>>>,
    calls: RefCell<Vec<Call>>,
}

impl Script {
    pub fn new(replies: Vec<Result<Reply, TransportError>>) -> Script {
        Script {
            host: "sourcegraph.com".to_owned(),
            replies: RefCell::new(replies.into()),
            calls: RefCell::new(Vec::new()),
        }
    }

    /// A transport that answers its one request with `body` as a 200 JSON
    /// reply.
    pub fn graphql(body: &Value) -> Script {
        Script::new(vec![Ok(json_reply(body))])
    }

    pub fn named(mut self, host: &str) -> Script {
        host.clone_into(&mut self.host);
        self
    }

    pub fn calls(self) -> Vec<Call> {
        self.calls.into_inner()
    }

    fn answer(
        &self,
        method: &'static str,
        path: &str,
        deadline: Instant,
    ) -> Result<Reply, TransportError> {
        self.calls.borrow_mut().push(Call {
            method,
            path: path.to_owned(),
            deadline,
        });
        self.replies
            .borrow_mut()
            .pop_front()
            .unwrap_or(Err(TransportError::Network("script exhausted")))
    }
}

impl Transport for Script {
    fn host(&self) -> String {
        self.host.clone()
    }

    fn post(
        &self,
        path: &str,
        _body: &[u8],
        deadline: Instant,
    ) -> Result<Reply, TransportError> {
        self.answer("POST", path, deadline)
    }

    fn get(
        &self,
        path: &str,
        deadline: Instant,
    ) -> Result<Reply, TransportError> {
        self.answer("GET", path, deadline)
    }
}
