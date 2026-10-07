use std::ffi::OsString;
use std::process::ExitCode;

use sego::internal::upstream::{Endpoint, Transport};
use sego::{Env, run};

pub struct Ran {
    pub code: ExitCode,
    pub out: Vec<u8>,
    pub err: Vec<u8>,
}

pub fn args(words: &[&str]) -> Vec<OsString> {
    words.iter().map(OsString::from).collect()
}

/// Runs `sego::run` in-process on an argument vector.
pub fn exec_os(
    args: Vec<OsString>,
    endpoint: Option<&str>,
    connect: &dyn Fn(Endpoint) -> Box<dyn Transport>,
) -> Ran {
    let env = Env { endpoint: endpoint.map(OsString::from) };
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = run(args, &env, connect, &mut out, &mut err);
    Ran { code, out, err }
}

pub fn exec(
    words: &[&str],
    endpoint: Option<&str>,
    connect: &dyn Fn(Endpoint) -> Box<dyn Transport>,
) -> Ran {
    exec_os(args(words), endpoint, connect)
}
