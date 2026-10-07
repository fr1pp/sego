use std::collections::VecDeque;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Instant;

use sego::internal::upstream::{Reply, Transport, TransportError};

use super::fixture::Fixture;
use super::replay;

pub struct Call {
    pub method: &'static str,
    pub path: String,
    pub deadline: Instant,
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
}

impl Fake {
    fn with(source: Source) -> Fake {
        Fake {
            inner: Arc::new(Mutex::new(Inner { source, calls: Vec::new() })),
        }
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
        inner
            .calls
            .iter()
            .map(|call| Call {
                method: call.method,
                path: call.path.clone(),
                deadline: call.deadline,
            })
            .collect()
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
        inner.calls.push(Call { method, path: path.to_owned(), deadline });
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
