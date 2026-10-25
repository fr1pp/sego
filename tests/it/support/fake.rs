use std::collections::VecDeque;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Instant;

use sego::{Reply, Transport, TransportError};

#[derive(Clone)]
pub struct Call {
    pub path: String,
    pub body: Vec<u8>,
}

struct Inner {
    script: VecDeque<Result<Reply, TransportError>>,
    calls: Vec<Call>,
}

/// An in-process transport. Clones share one script and one call log, so a
/// test keeps a handle while `run` owns another.
#[derive(Clone)]
pub struct Fake {
    inner: Arc<Mutex<Inner>>,
}

impl Fake {
    /// Answers each request with the next queued result; an empty queue is a
    /// network error.
    pub fn scripted(replies: Vec<Result<Reply, TransportError>>) -> Fake {
        Fake {
            inner: Arc::new(Mutex::new(Inner {
                script: replies.into(),
                calls: Vec::new(),
            })),
        }
    }

    pub fn calls(&self) -> Vec<Call> {
        let inner = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        inner.calls.clone()
    }

    fn request(
        &self,
        path: &str,
        body: &[u8],
    ) -> Result<Reply, TransportError> {
        let mut inner =
            self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        inner.calls.push(Call { path: path.to_owned(), body: body.to_vec() });
        inner
            .script
            .pop_front()
            .unwrap_or(Err(TransportError::Network("script exhausted")))
    }
}

impl Transport for Fake {
    fn post(
        &self,
        path: &str,
        body: &[u8],
        _: Instant,
    ) -> Result<Reply, TransportError> {
        self.request(path, body)
    }

    fn get(&self, path: &str, _: Instant) -> Result<Reply, TransportError> {
        self.request(path, &[])
    }
}
