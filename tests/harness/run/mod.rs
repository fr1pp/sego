use std::ffi::OsString;
use std::io::{self, Write};
use std::os::unix::ffi::OsStringExt;
use std::process::ExitCode;

use tempfile::TempDir;

use sego::{Endpoint, Env, Transport, run};

use crate::case::{Case, Request};
use crate::replay::Replay;

module_tests!();

pub struct Outcome {
    pub exit: i32,
    pub stdout: String,
    pub stderr: String,
    pub sent: Vec<Request>,
    pub problems: Vec<String>,
}

/// A writer for a closed stdout pipe.
struct Closed;

impl Write for Closed {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::ErrorKind::BrokenPipe.into())
    }

    fn flush(&mut self) -> io::Result<()> {
        Err(io::ErrorKind::BrokenPipe.into())
    }
}

/// Runs the case's command line in-process against a replay of its
/// exchanges, with an empty cache directory of its own unless the case sets
/// `SEGO_CACHE_DIR`.
pub fn run_case(case: &Case) -> Result<Outcome, String> {
    run_with(case, &Replay::new(case))
}

/// Runs the case's command line in-process through `replay`.
pub fn run_with(case: &Case, replay: &Replay) -> Result<Outcome, String> {
    let cache = TempDir::new().map_err(|e| format!("cache directory: {e}"))?;
    let mut endpoint = None;
    let mut cache_dir = cache.path().as_os_str().to_owned();
    for (name, value) in &case.env {
        match name.as_str() {
            "SEGO_ENDPOINT" => endpoint = Some(OsString::from(value)),
            "SEGO_CACHE_DIR" => cache_dir = OsString::from(value),
            _ => return Err(format!("env: {name} is not read by run")),
        }
    }
    let env = Env { endpoint, cache_dir: Some(cache_dir) };
    let args = case.args.iter().cloned().map(OsString::from_vec).collect();
    let handle = replay.clone();
    let connect =
        move |_: Endpoint| -> Box<dyn Transport> { Box::new(handle.clone()) };
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let code = if case.stdout_closed {
        run(args, &env, &connect, &mut Closed, &mut stderr)
    } else {
        run(args, &env, &connect, &mut stdout, &mut stderr)
    };
    Ok(Outcome {
        exit: exit_code(code)?,
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
        sent: replay.sent(),
        problems: replay.problems(),
    })
}

/// `ExitCode` is opaque, so the code is found by trying each one.
fn exit_code(code: ExitCode) -> Result<i32, String> {
    (0..=u8::MAX)
        .find(|&n| code == ExitCode::from(n))
        .map(i32::from)
        .ok_or_else(|| "run returned an unknown exit code".to_owned())
}
