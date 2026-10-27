//! sego, a code search CLI for agents built on Sourcegraph. The `sego` binary
//! is this library's [`run`].
//!
//! [`run`] takes the arguments, the [`Env`] and a `connect` function that
//! makes the [`Transport`] for an [`Endpoint`], so a test can answer every
//! request itself. [`connect`] makes the one that sends them over HTTP.

mod address;
mod cache;
mod cli;
mod commands;
mod error;
mod escape;
mod output;
mod sourcegraph;

use std::ffi::{OsStr, OsString};
use std::io::{self, Write};

use cache::Cache;
use cli::{Invocation, SYNOPSIS};
use error::Error;
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
    /// `SEGO_ENDPOINT`, the server to read from; `None` for sourcegraph.com.
    pub endpoint: Option<OsString>,
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
        _ => 1,
    }
}

/// The endpoint `SEGO_ENDPOINT` names: sourcegraph.com when it is unset or
/// empty.
fn endpoint(value: Option<&OsStr>) -> Result<Endpoint, Error> {
    let Some(value) = value.filter(|value| !value.is_empty()) else {
        return Ok(Endpoint::default());
    };
    let Some(text) = value.to_str() else {
        return Err(Error::usage("SEGO_ENDPOINT is not valid UTF-8", SYNOPSIS));
    };
    text.parse().map_err(|reason| Error::usage(format!("SEGO_ENDPOINT {reason}: {text}"), SYNOPSIS))
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
    let command = match cli::parse(args)? {
        Invocation::Help(topic) => return Ok(out.write_all(cli::help::text(topic).as_bytes())?),
        Invocation::Version => return Ok(writeln!(out, "sego {}", env!("CARGO_PKG_VERSION"))?),
        Invocation::Command(command) => command,
    };
    let endpoint = endpoint(env.endpoint.as_deref())?;
    let cache = Cache::new(env.cache_dir.as_deref(), &endpoint);
    let client = Client::new(connect(endpoint));
    commands::run(command, &client, &cache, notes, out, err)?;
    out.flush()?;
    cache.trim();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usage(value: &OsStr) -> String {
        let Err(error) = endpoint(Some(value)) else { panic!("{value:?} was accepted") };
        error::report(&error)
    }

    #[test]
    fn an_unset_or_empty_sego_endpoint_is_sourcegraph_com() {
        for value in [None, Some(OsStr::new(""))] {
            let endpoint = endpoint(value).unwrap_or_else(|e| panic!("{e}"));
            assert_eq!(endpoint.url(""), "https://sourcegraph.com");
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_sego_endpoint_that_isnt_utf8_is_a_usage_error() {
        use std::os::unix::ffi::OsStrExt;
        assert_eq!(
            usage(OsStr::from_bytes(b"http://\xff")),
            "sego: error: SEGO_ENDPOINT is not valid UTF-8\nusage: sego COMMAND [--json] ...\n"
        );
    }
}
