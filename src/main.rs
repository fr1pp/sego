mod address;
mod args;
mod cmd;
mod diagnostic;
mod domain;
mod escape;
mod output;
mod render;
mod sourcegraph;

use std::env;
use std::ffi::OsString;
use std::io::{self, Write};
use std::process::ExitCode;

use args::Cli;
use cmd::{Present, UsageError};
use diagnostic::{Diagnostic, report};
use output::Output;
use sourcegraph::transport::Transport;
use sourcegraph::{FetchError, Plan, Reply};

const DEFAULT_ENDPOINT: &str = "https://sourcegraph.com";

fn main() -> ExitCode {
    let args: Vec<OsString> = env::args_os().skip(1).collect();
    let json = args::wants_json(&args);
    let result = args::parse(args)
        .map_err(|error| Box::from(UsageError::from(error)))
        .and_then(|cli| answer(&cli));
    let mut stdout = Output::new(io::stdout().lock());
    let outcome = match result {
        Ok(present) => {
            let text = if json { present.json() } else { present.text() };
            match stdout.write(&text).and_then(|()| stdout.flush()) {
                Ok(()) => present.outcome(),
                Err(error) => {
                    to_stderr(&report(&error));
                    error.outcome()
                }
            }
        }
        Err(diagnostic) => {
            let written = if json {
                let document = render::json::diagnostic(&*diagnostic);
                stdout.write(&document).and_then(|()| stdout.flush())
            } else {
                Ok(())
            };
            to_stderr(&report(&*diagnostic));
            match written {
                Ok(()) => diagnostic.outcome(),
                Err(error) => {
                    to_stderr(&report(&error));
                    error.outcome()
                }
            }
        }
    };
    ExitCode::from(outcome.exit_code())
}

/// The answer to `cli`, building the transport only for a command that
/// needs the upstream, and only once its operands are known to be valid.
fn answer(cli: &Cli) -> Result<Box<dyn Present>, Box<dyn Diagnostic>> {
    if !cmd::needs_upstream(cli) {
        return cmd::run(cli, &offline);
    }
    cmd::validate(cli)?;
    let transport = Transport::new(&endpoint())?;
    cmd::run(cli, &|plan| transport.fetch(plan))
}

/// `SEGO_ENDPOINT`, unless it is unset or empty.
fn endpoint() -> String {
    env::var_os("SEGO_ENDPOINT")
        .filter(|endpoint| !endpoint.is_empty())
        .map_or(DEFAULT_ENDPOINT.into(), |endpoint| {
            endpoint.to_string_lossy().into_owned()
        })
}

/// The fetch for invocations that never reach the upstream: help, the
/// version and usage errors.
fn offline(_: &Plan) -> Result<Reply, FetchError> {
    unreachable!("only a command that needs the upstream fetches")
}

/// Writes to stderr, ignoring failure: there is nowhere left to report it.
fn to_stderr(text: &str) {
    let _ = io::stderr().write_all(text.as_bytes());
}
