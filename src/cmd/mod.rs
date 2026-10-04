pub mod read;

use crate::args::{Cli, UsageError, did_you_mean};
use crate::diagnostic::{Diagnostic, Outcome};
use crate::escape::shell_word;
use crate::sourcegraph::Fetch;

pub const COMMANDS: &[&str] = &["search", "list", "read"];

const HELP: &str = "\
sego finds and inspects public source code at any revision, through
Sourcegraph's GraphQL API.

Usage:
  sego search [--json] <query>...
  sego list   [--json] [--depth N] <repo>[@<rev>] [<path>]
  sego read   [--json] <repo>[@<rev>] <path> [<lines>]
  sego <command> --help

Commands:
  search   where is it? Matching code, paths, symbols, commits and repos
  list     what is here? The entries of a directory
  read     what does it say? The lines of a file

Options:
  --json       write the outcome as one JSON document on stdout
  --depth N    list only: entries down to depth N, at least 1 (default 1)
  --help       print this help, or a command's help after its name
  --version    print sego's version

Options may appear anywhere before --; every other token is an operand, so
search negations such as -file:_test need no escaping.

Output:
  An answer goes to stdout; anything else is reported on stderr. With
  --json, a non-answer's document also goes to stdout.

Environment:
  SEGO_ENDPOINT   the Sourcegraph instance (default https://sourcegraph.com)

Exit codes:
  0  answered      use it
  1  answered no   ask something else
  2  invalid       fix the command
  3  unanswered    retry, or narrow and retry
";

const VERSION: &str = concat!("sego ", env!("CARGO_PKG_VERSION"), "\n");

/// An answer, ready to print.
pub trait Present {
    fn outcome(&self) -> Outcome;
    fn text(&self) -> String;
    fn json(&self) -> String;
}

/// Help or the version: the same text with or without `--json`.
struct Plain(&'static str);

impl Present for Plain {
    fn outcome(&self) -> Outcome {
        Outcome::Answered
    }

    fn text(&self) -> String {
        self.0.to_owned()
    }

    fn json(&self) -> String {
        self.0.to_owned()
    }
}

/// What an invocation asks for.
enum Action {
    /// Help for a command, or the top-level help.
    Help(Option<&'static str>),
    Version,
    Run(&'static str),
}

fn action(cli: &Cli) -> Result<Action, UsageError> {
    let command = match cli.command.as_deref() {
        Some(name) => Some(known(name, cli)?),
        None => None,
    };
    if let Some(command) = command
        && command != "list"
        && cli.depth.is_some()
    {
        let without = Cli { depth: None, ..cli.clone() };
        return Err(UsageError::new("--depth applies only to list")
            .with_hint(without.command_line()));
    }
    if cli.help {
        Ok(Action::Help(command))
    } else if cli.version {
        Ok(Action::Version)
    } else {
        match command {
            Some(command) => Ok(Action::Run(command)),
            None => Err(UsageError::new(
                "missing command; expected search, list or read",
            )
            .with_hint("sego --help".to_owned())),
        }
    }
}

/// `name` as one of `COMMANDS`; if it is none of them, a usage error
/// whose hint, when the name is close to one, is the corrected command.
fn known(name: &str, cli: &Cli) -> Result<&'static str, UsageError> {
    if let Some(&command) = COMMANDS.iter().find(|&&command| command == name) {
        return Ok(command);
    }
    let error =
        UsageError::new(format!("unknown command {}", shell_word(name)));
    let hint = match did_you_mean(name, COMMANDS) {
        Some(command) => {
            Cli { command: Some(command.to_owned()), ..cli.clone() }
                .command_line()
        }
        None => "sego --help".to_owned(),
    };
    Err(error.with_hint(hint))
}

fn not_implemented(command: &str) -> Box<dyn Diagnostic> {
    Box::new(UsageError::new(format!("{command} is not implemented yet")))
}

/// A command's help.
fn help(command: &str) -> Result<&'static str, Box<dyn Diagnostic>> {
    match command {
        "read" => Ok(read::HELP),
        command => Err(not_implemented(command)),
    }
}

/// Whether the invocation runs a command, which needs the upstream.
pub fn needs_upstream(cli: &Cli) -> bool {
    matches!(action(cli), Ok(Action::Run(_)))
}

/// Checks the invocation as far as possible without fetching anything.
pub fn validate(cli: &Cli) -> Result<(), Box<dyn Diagnostic>> {
    match action(cli)? {
        Action::Run("read") => {
            read::request(cli)?;
            Ok(())
        }
        Action::Run(command) => Err(not_implemented(command)),
        Action::Help(_) | Action::Version => Ok(()),
    }
}

pub fn run(
    cli: &Cli,
    fetch: Fetch,
) -> Result<Box<dyn Present>, Box<dyn Diagnostic>> {
    match action(cli)? {
        Action::Help(None) => Ok(Box::new(Plain(HELP))),
        Action::Help(Some(command)) => Ok(Box::new(Plain(help(command)?))),
        Action::Version => Ok(Box::new(Plain(VERSION))),
        Action::Run("read") => read::run(cli, fetch),
        Action::Run(command) => Err(not_implemented(command)),
    }
}
