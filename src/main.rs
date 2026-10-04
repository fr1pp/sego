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
use std::io;
use std::process::ExitCode;

use args::Cli;
use cmd::{Action, Present, UsageError};
use diagnostic::{Diagnostic, report};
use output::{Output, to_stderr};
use sourcegraph::transport::{InvalidEndpoint, Transport};

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

/// The answer to `cli`, building the transport only for a command, and
/// only once its operands are known to be valid.
fn answer(cli: &Cli) -> Result<Box<dyn Present>, Box<dyn Diagnostic>> {
    match cmd::action(cli)? {
        Action::Print(plain) => Ok(Box::new(plain)),
        Action::Run(command) => {
            command.validate(cli)?;
            let transport = Transport::new(&endpoint()?)?;
            command.run(cli, &|plan| transport.fetch(plan))
        }
    }
}

/// `SEGO_ENDPOINT`, or `DEFAULT_ENDPOINT` when it is unset or empty.
fn endpoint() -> Result<String, InvalidEndpoint> {
    match env::var_os("SEGO_ENDPOINT").filter(|value| !value.is_empty()) {
        None => Ok(DEFAULT_ENDPOINT.to_owned()),
        Some(value) => {
            value.into_string().map_err(|_| InvalidEndpoint::NotUtf8)
        }
    }
}
