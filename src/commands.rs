//! One module per command. Each command's `run` asks Sourcegraph through the
//! client and returns its `Answer`, which prints as text or as JSON.

pub mod diff;
pub mod log;
pub mod ls;
pub mod refs;
pub mod search;
pub mod show;

use std::fmt::Display;
use std::io::Write;

use serde::Serialize;

use crate::Failure;
use crate::cli::Command;
use crate::error::Error;
use crate::output::{Notes, print};
use crate::sourcegraph::{Client, Resolved};

/// Runs `command`, then writes its notes to `err` and its answer to `out`.
/// A usage error found from the server's response takes the command's
/// synopsis here.
pub fn run(
    command: Command,
    client: Client,
    notes: &mut Notes,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<(), Failure> {
    let (json, synopsis) = command.json_and_synopsis();
    let with_synopsis = |error: Error| error.with_synopsis(synopsis);
    match command {
        Command::Search(args) => {
            let answer = search::run(&args, client, notes).map_err(with_synopsis);
            write(answer, json, notes, out, err)
        }
        Command::Show(args) => {
            write(show::run(&args, client, notes).map_err(with_synopsis), json, notes, out, err)
        }
        Command::ShowCommit(args) => {
            let answer = show::run_commit(&args, client, notes).map_err(with_synopsis);
            write(answer, json, notes, out, err)
        }
        Command::Ls(args) => {
            write(ls::run(&args, client, notes).map_err(with_synopsis), json, notes, out, err)
        }
        Command::Refs(args) => {
            write(refs::run(&args, client).map_err(with_synopsis), json, notes, out, err)
        }
        Command::Log(args) => {
            write(log::run(&args, client, notes).map_err(with_synopsis), json, notes, out, err)
        }
        Command::Diff(args) => {
            write(diff::run(&args, client, notes).map_err(with_synopsis), json, notes, out, err)
        }
    }
}

/// The notes every resolved address may carry: a failed lookup of the other
/// host of `owner/name`.
pub fn resolved_notes(at: &Resolved, notes: &mut Notes) {
    if let Err(error) = &at.other_host {
        notes.push(error.to_string());
    }
}

fn write<A: Display + Serialize>(
    answer: Result<A, Error>,
    json: bool,
    notes: &Notes,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<(), Failure> {
    let answer = answer?;
    notes.write(err)?;
    print(&answer, json, notes, out)?;
    Ok(())
}
