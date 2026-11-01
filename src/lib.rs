//! sego, a code search CLI for agents built on Sourcegraph. The `sego` binary
//! is this library's [`run`].
//!
//! [`run`] takes the arguments, the [`Env`] and a `connect` function that
//! makes the [`Transport`] for an [`Endpoint`], so a test can answer every
//! request itself. [`connect`] makes the one that sends them over HTTP.
//!
//! This interface exists for the binary and sego's tests, and has no semantic
//! version guarantees.

mod address;
mod cache;
mod cli;
mod commands;
mod error;
mod escape;
mod output;
mod pointer;
mod sourcegraph;

use std::ffi::OsString;
use std::io::{self, Write};
use std::time::Instant;

use cli::Invocation;
use error::Failure;
use output::Notes;
use sourcegraph::Client;

pub use sourcegraph::{Endpoint, Response, Transport, TransportError};

/// The transport that reads from `endpoint` over HTTP, for `run`'s `connect`.
#[must_use]
pub fn connect(endpoint: Endpoint) -> Box<dyn Transport> {
    Box::new(sourcegraph::Ureq::new(endpoint))
}

/// The process environment that `run` reads.
pub struct Env {
    /// `SEGO_CACHE_DIR`, the root of the cache; `None` for the
    /// default location.
    pub cache_dir: Option<OsString>,
}

/// Runs one sego invocation, writes its answer to `out` and its notes and
/// errors to `err`, and returns the process exit code (ADR 0005).
///
/// `connect` is handed the endpoint once the command needs the server and
/// returns the transport that carries every request.
#[expect(clippy::needless_pass_by_value, reason = "main hands over the arguments it collected")]
pub fn run(
    args: Vec<OsString>,
    env: &Env,
    connect: &dyn Fn(Endpoint) -> Box<dyn Transport>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> u8 {
    let mut notes = Notes::new();
    let result = attempt(&args, env, connect, &mut notes, out, err);
    let failure = match result {
        Ok(()) => return 0,
        Err(Failure::Answer(error)) => {
            match notes.write(err).and_then(|()| err.write_all(error::report(&error).as_bytes())) {
                Ok(()) => return error.exit_code(),
                Err(e) => e,
            }
        }
        Err(Failure::Write(e)) => e,
    };
    match failure.kind() {
        io::ErrorKind::BrokenPipe => 0,
        _ => 3,
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
    let command = match cli::parse(args)? {
        Invocation::Help(topic) => return Ok(out.write_all(cli::help::text(topic).as_bytes())?),
        Invocation::Version => return Ok(writeln!(out, "sego {}", env!("CARGO_PKG_VERSION"))?),
        Invocation::Command(command) => command,
    };
    let endpoint = Endpoint::default();
    let cache = endpoint.cache(env.cache_dir.as_deref());
    let deadline = Instant::now() + sourcegraph::DEADLINE;
    let client = Client::new(connect(endpoint.clone()), endpoint, &cache, deadline);
    let synopsis = cli::synopsis(&command);
    commands::run(command, client, notes, out, err)
        .map_err(|failure| failure.with_synopsis(synopsis))?;
    out.flush()?;
    cache.trim();
    Ok(())
}
