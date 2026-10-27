//! One module per command. Each command's `run` asks Sourcegraph through the
//! client and returns its `Answer`, which prints as text or as JSON.

pub mod commit;
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
use crate::cache::Cache;
use crate::cli::Command;
use crate::error::Error;
use crate::output::{Notes, print};
use crate::sourcegraph::Client;

/// Runs `command`, then writes its notes to `err` and its answer to `out`.
pub fn run(
    command: Command,
    client: &Client,
    cache: &Cache,
    notes: &mut Notes,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<(), Failure> {
    match command {
        Command::Search(args) => {
            write(search::run(&args, client, notes), args.json, notes, out, err)
        }
        Command::Show(args) => {
            write(show::run(&args, client, cache, notes), args.json, notes, out, err)
        }
        Command::ShowCommit(args) => {
            write(commit::run(&args, client, notes), args.json, notes, out, err)
        }
        Command::Ls(args) => write(ls::run(&args, client), args.json, notes, out, err),
        Command::Refs(args) => write(refs::run(&args, client), args.json, notes, out, err),
        Command::Log(args) => write(log::run(&args, client, notes), args.json, notes, out, err),
        Command::Diff(args) => write(diff::run(&args, client), args.json, notes, out, err),
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
    print(&answer, json, out)?;
    Ok(())
}
