use std::io::{self, Write};
use std::process::ExitCode;

use sego::internal::upstream::{Transport, Ureq};
use sego::{Env, run};

fn main() -> ExitCode {
    let args = std::env::args_os().skip(1).collect();
    let env = Env { endpoint: std::env::var_os("SEGO_ENDPOINT") };
    let mut out = io::stdout().lock();
    let mut err = io::stderr().lock();
    let code = run(
        args,
        &env,
        &|endpoint| Box::new(Ureq::new(endpoint)) as Box<dyn Transport>,
        &mut out,
        &mut err,
    );
    let _ = out.flush();
    code
}
