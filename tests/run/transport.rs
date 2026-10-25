use std::cell::RefCell;
use std::collections::VecDeque;
use std::ffi::{OsStr, OsString};
use std::process::ExitCode;
use std::rc::Rc;
use std::time::Instant;

use sego::{Endpoint, Env, Reply, Transport, TransportError, run};

/// A request that reached the transport.
#[derive(Clone, Debug)]
pub struct Call {
    pub method: &'static str,
    pub deadline: Instant,
}

struct Inner {
    replies: VecDeque<Result<Reply, TransportError>>,
    calls: Vec<Call>,
}

/// The transport `run` connects through: it answers each request with the
/// next reply or error it was given, a network error once they run out, and
/// logs every request. Clones share one script and one log.
#[derive(Clone)]
pub struct Server {
    inner: Rc<RefCell<Inner>>,
}

impl Server {
    pub fn new(replies: Vec<Result<Reply, TransportError>>) -> Server {
        Server {
            inner: Rc::new(RefCell::new(Inner {
                replies: replies.into(),
                calls: Vec::new(),
            })),
        }
    }

    pub fn calls(&self) -> Vec<Call> {
        self.inner.borrow().calls.clone()
    }

    pub fn methods(&self) -> Vec<&'static str> {
        self.calls().iter().map(|call| call.method).collect()
    }

    fn request(
        &self,
        method: &'static str,
        deadline: Instant,
    ) -> Result<Reply, TransportError> {
        let mut inner = self.inner.borrow_mut();
        inner.calls.push(Call { method, deadline });
        inner
            .replies
            .pop_front()
            .unwrap_or(Err(TransportError::Network("no reply left")))
    }
}

impl Transport for Server {
    fn post(
        &self,
        _: &str,
        _: &[u8],
        deadline: Instant,
    ) -> Result<Reply, TransportError> {
        self.request("POST", deadline)
    }

    fn get(
        &self,
        _: &str,
        deadline: Instant,
    ) -> Result<Reply, TransportError> {
        self.request("GET", deadline)
    }
}

pub fn reply(status: u16, content_type: &str, body: &[u8]) -> Reply {
    Reply {
        status,
        content_type: Some(content_type.to_owned()),
        location: None,
        retry_after: None,
        cf_mitigated: None,
        body: body.to_vec(),
    }
}

/// What one invocation of `run` wrote and returned.
pub struct Ran {
    pub code: ExitCode,
    pub stdout: String,
    pub stderr: String,
}

/// Runs `sego` with `args` and `cache_dir` as `SEGO_CACHE_DIR`, connecting
/// through `server`.
pub fn sego(args: &[&str], cache_dir: &OsStr, server: &Server) -> Ran {
    let env = Env { endpoint: None, cache_dir: Some(cache_dir.to_owned()) };
    let handle = server.clone();
    let connect =
        move |_: Endpoint| -> Box<dyn Transport> { Box::new(handle.clone()) };
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let code = run(
        args.iter().map(OsString::from).collect(),
        &env,
        &connect,
        &mut stdout,
        &mut stderr,
    );
    Ran {
        code,
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
    }
}
