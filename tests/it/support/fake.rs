use std::collections::VecDeque;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Instant;

use sego::{Reply, Transport, TransportError};

use super::fixture::Fixture;
use super::replay;

#[derive(Clone)]
pub struct Call {
    pub method: &'static str,
    pub path: String,
    pub deadline: Instant,
    pub body: Vec<u8>,
}

enum Source {
    Script(VecDeque<Result<Reply, TransportError>>),
    Fixtures(Vec<Fixture>),
}

struct Inner {
    source: Source,
    calls: Vec<Call>,
}

/// An in-process transport. Clones share one script and one call log, so a
/// test keeps a handle while `run` owns another.
#[derive(Clone)]
pub struct Fake {
    inner: Arc<Mutex<Inner>>,
    host: Option<String>,
}

impl Fake {
    fn with(source: Source) -> Fake {
        Fake {
            inner: Arc::new(Mutex::new(Inner { source, calls: Vec::new() })),
            host: None,
        }
    }

    /// Reports `host` from `Transport::host` instead of the default.
    pub fn named(mut self, host: &str) -> Fake {
        self.host = Some(host.to_owned());
        self
    }

    /// Answers each request with the next queued result; an empty queue is a
    /// network error.
    pub fn scripted(replies: Vec<Result<Reply, TransportError>>) -> Fake {
        Fake::with(Source::Script(replies.into()))
    }

    pub fn fixtures(fixtures: Vec<Fixture>) -> Fake {
        Fake::with(Source::Fixtures(fixtures))
    }

    pub fn calls(&self) -> Vec<Call> {
        let inner = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        inner.calls.clone()
    }

    fn request(
        &self,
        method: &'static str,
        path: &str,
        body: &[u8],
        deadline: Instant,
    ) -> Result<Reply, TransportError> {
        let mut inner =
            self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        inner.calls.push(Call {
            method,
            path: path.to_owned(),
            deadline,
            body: body.to_vec(),
        });
        match &mut inner.source {
            Source::Script(queue) => queue
                .pop_front()
                .unwrap_or(Err(TransportError::Network("script exhausted"))),
            Source::Fixtures(fixtures) => {
                replay::answer(fixtures, method, path, body)
            }
        }
    }
}

impl Transport for Fake {
    fn host(&self) -> String {
        self.host.clone().unwrap_or_else(|| "sourcegraph.com".to_owned())
    }

    fn post(
        &self,
        path: &str,
        body: &[u8],
        deadline: Instant,
    ) -> Result<Reply, TransportError> {
        self.request("POST", path, body, deadline)
    }

    fn get(
        &self,
        path: &str,
        deadline: Instant,
    ) -> Result<Reply, TransportError> {
        self.request("GET", path, &[], deadline)
    }
}
