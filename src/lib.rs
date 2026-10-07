mod address;
mod cli;
mod commit;
mod diff;
mod error;
mod escape;
mod log;
mod ls;
mod output;
mod quote;
mod refs;
mod search;
mod show;
mod submodule;
mod upstream;

#[doc(hidden)]
pub mod internal {
    pub mod address {
        pub use crate::address::*;
    }
    pub mod cli {
        pub use crate::cli::*;
    }
    pub mod commit {
        pub use crate::commit::*;
    }
    pub mod diff {
        pub use crate::diff::*;
    }
    pub mod error {
        pub use crate::error::*;
    }
    pub mod escape {
        pub use crate::escape::*;
    }
    pub mod log {
        pub use crate::log::*;
    }
    pub mod ls {
        pub use crate::ls::*;
    }
    pub mod output {
        pub use crate::output::*;
    }
    pub mod quote {
        pub use crate::quote::*;
    }
    pub mod refs {
        pub use crate::refs::*;
    }
    pub mod search {
        pub use crate::search::*;
    }
    pub mod submodule {
        pub use crate::submodule::*;
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
use output::{Answer, Notes};
use upstream::{DEADLINE, Endpoint, Transport};

pub struct Env {
    pub endpoint: Option<OsString>,
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
    let mut notes = Notes::new();
    let result = attempt(&args, env, connect, &mut notes, out, err);
    let failure = match result {
        Ok(()) => return ExitCode::SUCCESS,
        Err(Failure::Answer(error)) => {
            match notes.write(err).and_then(|()| error::report(&error, err)) {
                Ok(()) => return ExitCode::from(error.exit_code()),
                Err(e) => e,
            }
        }
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
    notes: &mut Notes,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<(), Failure> {
    dispatch(args, env, connect, notes, out, err)?;
    out.flush()?;
    Ok(())
}

fn dispatch(
    args: &[OsString],
    env: &Env,
    connect: &dyn Fn(Endpoint) -> Box<dyn Transport>,
    notes: &mut Notes,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<(), Failure> {
    let open = || -> Result<_, Error> {
        let endpoint = Endpoint::from_env(env.endpoint.as_deref())?;
        Ok((connect(endpoint), Instant::now() + DEADLINE))
    };
    let (answer, json): (Box<dyn Answer>, bool) = match cli::parse(args)? {
        Command::Help(topic) => {
            out.write_all(cli::help(topic).as_bytes())?;
            return Ok(());
        }
        Command::Version => {
            writeln!(out, "sego {}", env!("CARGO_PKG_VERSION"))?;
            return Ok(());
        }
        Command::Search { query, json } => {
            let (transport, deadline) = open()?;
            let searched = search::run(&query, &*transport, deadline, notes)?;
            (Box::new(searched), json)
        }
        Command::Show { address, lines, json } => {
            let (transport, deadline) = open()?;
            let shown = show::run(
                &address,
                lines.as_ref(),
                &*transport,
                deadline,
                notes,
            )?;
            (Box::new(shown), json)
        }
        Command::Ls { address, recursive, json } => {
            let (transport, deadline) = open()?;
            let listed =
                ls::run(&address, recursive, &*transport, deadline, notes)?;
            (Box::new(listed), json)
        }
        Command::Refs { repo, text, json } => {
            let (transport, deadline) = open()?;
            let listed = refs::run(
                &repo,
                text.as_deref(),
                json,
                &*transport,
                deadline,
                notes,
            )?;
            (Box::new(listed), json)
        }
        Command::ShowCommit { address, json } => {
            let (transport, deadline) = open()?;
            let committed =
                commit::run(&address, &*transport, deadline, notes)?;
            (Box::new(committed), json)
        }
        Command::Log { address, path, json } => {
            let (transport, deadline) = open()?;
            let logged = log::run(
                &address,
                path.as_deref(),
                json,
                &*transport,
                deadline,
                notes,
            )?;
            (Box::new(logged), json)
        }
        Command::Diff { address, paths, json } => {
            let (transport, deadline) = open()?;
            let diffed = diff::run(
                &address,
                &paths,
                json,
                &*transport,
                deadline,
                notes,
            )?;
            (Box::new(diffed), json)
        }
    };
    notes.write(err)?;
    if json {
        answer.write_json(out)?;
    } else {
        out.write_all(answer.text().as_bytes())?;
    }
    Ok(())
}
