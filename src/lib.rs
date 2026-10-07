mod address;
mod cli;
mod error;
mod escape;
mod output;
mod quote;
mod upstream;

#[doc(hidden)]
pub mod internal {
    pub mod address {
        pub use crate::address::*;
    }
    pub mod cli {
        pub use crate::cli::*;
    }
    pub mod error {
        pub use crate::error::*;
    }
    pub mod escape {
        pub use crate::escape::*;
    }
    pub mod output {
        pub use crate::output::*;
    }
    pub mod quote {
        pub use crate::quote::*;
    }
    pub mod upstream {
        pub use crate::upstream::*;
    }
}

use std::ffi::OsString;
use std::io::{self, Write};
use std::process::ExitCode;
use std::time::Instant;

use cli::Command;
use error::Error;
use output::Notes;
use upstream::{DEADLINE, Endpoint, Transport};

pub struct Env {
    pub endpoint: Option<OsString>,
}

struct Answer {
    notes: Notes,
    body: Vec<u8>,
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "main hands over the arguments it collected"
)]
pub fn run(
    args: Vec<OsString>,
    env: &Env,
    connect: &dyn Fn(Endpoint) -> Box<dyn Transport>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> ExitCode {
    let failure = match attempt(&args, env, connect, out, err) {
        Ok(()) => return ExitCode::SUCCESS,
        Err(Failure::Answer(error)) => match error::report(&error, err) {
            Ok(()) => return ExitCode::from(error.exit_code()),
            Err(e) => e,
        },
        Err(Failure::Write(e)) => e,
    };
    if failure.kind() == io::ErrorKind::BrokenPipe {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

enum Failure {
    Answer(Error),
    Write(io::Error),
}

impl From<Error> for Failure {
    fn from(error: Error) -> Failure {
        Failure::Answer(error)
    }
}

impl From<io::Error> for Failure {
    fn from(error: io::Error) -> Failure {
        Failure::Write(error)
    }
}

fn attempt(
    args: &[OsString],
    env: &Env,
    connect: &dyn Fn(Endpoint) -> Box<dyn Transport>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<(), Failure> {
    dispatch(args, env, connect, out, err)?;
    out.flush()?;
    Ok(())
}

fn dispatch(
    args: &[OsString],
    env: &Env,
    connect: &dyn Fn(Endpoint) -> Box<dyn Transport>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<(), Failure> {
    let command = cli::parse(args)?;
    match command {
        Command::Help(topic) => {
            out.write_all(cli::help(topic).as_bytes())?;
            return Ok(());
        }
        Command::Version => {
            writeln!(out, "sego {}", env!("CARGO_PKG_VERSION"))?;
            return Ok(());
        }
        Command::Show { .. } => {}
    }
    let endpoint = Endpoint::from_env(env.endpoint.as_deref())?;
    let transport = connect(endpoint);
    let deadline = Instant::now() + DEADLINE;
    let answer = execute(command, &*transport, deadline)?;
    answer.notes.write(err)?;
    out.write_all(&answer.body)?;
    Ok(())
}

fn execute(
    _command: Command,
    _transport: &dyn Transport,
    _deadline: Instant,
) -> Result<Answer, Error> {
    Err(Error::upstream("show is not wired yet"))
}
