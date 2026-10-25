use std::io;
use std::process::ExitCode;

use sego::internal::upstream::{Transport, Ureq};
use sego::{Env, run};

fn main() -> ExitCode {
    let args = std::env::args_os().skip(1).collect();
    let env = Env {
        endpoint: std::env::var_os("SEGO_ENDPOINT"),
        cache_dir: std::env::var_os("SEGO_CACHE_DIR"),
    };
    let mut out = io::stdout().lock();
    let mut err = io::stderr().lock();
    run(
        args,
        &env,
        &|endpoint| Box::new(Ureq::new(endpoint)) as Box<dyn Transport>,
        &mut out,
        &mut err,
    )
}
