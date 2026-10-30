use std::io;
use std::process::ExitCode;

use sego::{Env, connect, run};

fn main() -> ExitCode {
    let args = std::env::args_os().skip(1).collect();
    let env = Env { cache_dir: std::env::var_os("SEGO_CACHE_DIR") };
    let mut out = io::stdout().lock();
    let mut err = io::stderr().lock();
    ExitCode::from(run(args, &env, &connect, &mut out, &mut err))
}
