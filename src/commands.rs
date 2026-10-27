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

use crate::error::{Error, Failure};
use crate::output::{Notes, print};
use crate::sourcegraph::{Client, Revisions};

/// A command that asks Sourcegraph, with its arguments narrowed to what it
/// acts on.
#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Search(search::Args),
    Show(show::Args),
    ShowCommit(show::CommitArgs),
    Ls(ls::Args),
    Refs(refs::Args),
    Log(log::Args),
    Diff(diff::Args),
}

/// What `log` and `diff` cover: a revision, or the default branch for
/// `None`; or a range, its head against its base.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Revs {
    One(Option<String>),
    Range { base: String, head: String },
}

impl Revs {
    /// The revisions as the client takes them.
    fn revisions(&self) -> Revisions<'_> {
        match self {
            Revs::One(rev) => Revisions::One(rev.as_deref()),
            Revs::Range { base, head } => Revisions::Range { base, head },
        }
    }
}

/// What an address names, as `show` and `ls` say in JSON.
#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AddressKind {
    File,
    Directory,
    Submodule,
    Commit,
}

/// Runs `command`, then writes its notes to `err` and its answer to `out`.
pub fn run(
    command: Command,
    client: Client,
    notes: &mut Notes,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<(), Failure> {
    match command {
        Command::Search(args) => {
            write(search::run(&args, client, notes), args.json, notes, out, err)
        }
        Command::Show(args) => write(show::run(&args, client, notes), args.json, notes, out, err),
        Command::ShowCommit(args) => {
            write(show::run_commit(&args, client, notes), args.json, notes, out, err)
        }
        Command::Ls(args) => write(ls::run(&args, client, notes), args.json, notes, out, err),
        Command::Refs(args) => write(refs::run(&args, client, notes), args.json, notes, out, err),
        Command::Log(args) => write(log::run(&args, client, notes), args.json, notes, out, err),
        Command::Diff(args) => write(diff::run(&args, client, notes), args.json, notes, out, err),
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
