use std::ffi::OsString;
use std::io::{self, Write};
use std::process::ExitCode;

use sego::{Endpoint, Env, Transport, run};

struct Closed;

impl Write for Closed {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::ErrorKind::BrokenPipe.into())
    }

    fn flush(&mut self) -> io::Result<()> {
        Err(io::ErrorKind::BrokenPipe.into())
    }
}

fn never_connects(_: Endpoint) -> Box<dyn Transport> {
    unreachable!("a usage error sends nothing")
}

#[test]
fn a_closed_stderr_is_quiet_when_a_usage_error_is_reported() {
    let env = Env { endpoint: None, cache_dir: Some(OsString::from("off")) };
    let mut stdout = Vec::new();

    let code = run(
        vec![OsString::from("shwo")],
        &env,
        &never_connects,
        &mut stdout,
        &mut Closed,
    );

    assert_eq!(code, ExitCode::SUCCESS);
    assert_eq!(stdout, b"");
}
