use std::ffi::{OsStr, OsString};
use std::process::ExitCode;

use sego::{Endpoint, Env, Transport, run};

use crate::replay::Replay;

/// What one invocation of `run` wrote and returned.
pub struct Ran {
    pub code: ExitCode,
    pub stdout: String,
    pub stderr: String,
}

/// Runs `sego` with `args` and `cache_dir` as `SEGO_CACHE_DIR`, connecting
/// through `replay`.
pub fn sego(args: &[&str], cache_dir: &OsStr, replay: &Replay) -> Ran {
    let env = Env { endpoint: None, cache_dir: Some(cache_dir.to_owned()) };
    let handle = replay.clone();
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
