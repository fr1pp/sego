pub mod list;
pub mod read;
pub mod search;

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

/// A command sego runs.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Command {
    Search,
    List,
    Read,
}

impl Command {
    fn parse(name: &str) -> Option<Command> {
        match name {
            "search" => Some(Command::Search),
            "list" => Some(Command::List),
            "read" => Some(Command::Read),
            _ => None,
        }
    }

    fn help(self) -> &'static str {
        match self {
            Command::Search => search::HELP,
            Command::List => list::HELP,
            Command::Read => read::HELP,
        }
    }
}

/// What an invocation asks for.
enum Action {
    /// Help for a command, or the top-level help.
    Help(Option<Command>),
    Version,
    Run(Command),
}

fn action(cli: &Cli) -> Result<Action, UsageError> {
    let command = match cli.command.as_deref() {
        Some(name) => Some(known(name, cli)?),
        None => None,
    };
    if let Some(command) = command
        && command != Command::List
        && cli.depth.is_some()
    {
        let without = Cli { depth: None, ..cli.clone() };
        return Err(UsageError::new("--depth applies only to list")
            .with_hint(correction(&without)));
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

/// `name` as a command; if it is none of `COMMANDS`, a usage error whose
/// hint, when the name is close to one, is the corrected command.
fn known(name: &str, cli: &Cli) -> Result<Command, UsageError> {
    if let Some(command) = Command::parse(name) {
        return Ok(command);
    }
    let error =
        UsageError::new(format!("unknown command {}", shell_word(name)));
    let hint = match did_you_mean(name, COMMANDS) {
        Some(command) => correction(&Cli {
            command: Some(command.to_owned()),
            ..cli.clone()
        }),
        None => "sego --help".to_owned(),
    };
    Err(error.with_hint(hint))
}

/// `cli`, an invocation with one mistake fixed, as a hint: its command line
/// if it runs, else its command's help, since another mistake remains.
fn correction(cli: &Cli) -> String {
    match validate(cli) {
        Ok(()) => cli.command_line(),
        Err(_) => cli.help_hint(),
    }
}

/// Whether the invocation runs a command, which needs the upstream.
pub fn needs_upstream(cli: &Cli) -> bool {
    matches!(action(cli), Ok(Action::Run(_)))
}

/// Checks the invocation as far as possible without fetching anything.
pub fn validate(cli: &Cli) -> Result<(), Box<dyn Diagnostic>> {
    match action(cli)? {
        Action::Run(Command::Search) => {
            search::request(cli)?;
        }
        Action::Run(Command::List) => {
            list::request(cli)?;
        }
        Action::Run(Command::Read) => {
            read::request(cli)?;
        }
        Action::Help(_) | Action::Version => {}
    }
    Ok(())
}

pub fn run(
    cli: &Cli,
    fetch: Fetch,
) -> Result<Box<dyn Present>, Box<dyn Diagnostic>> {
    match action(cli)? {
        Action::Help(None) => Ok(Box::new(Plain(HELP))),
        Action::Help(Some(command)) => Ok(Box::new(Plain(command.help()))),
        Action::Version => Ok(Box::new(Plain(VERSION))),
        Action::Run(Command::Search) => search::run(cli, fetch),
        Action::Run(Command::List) => list::run(cli, fetch),
        Action::Run(Command::Read) => read::run(cli, fetch),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_command_name_parses() {
        for &name in COMMANDS {
            assert!(Command::parse(name).is_some(), "{name}");
        }
    }

    #[test]
    fn corrections_that_still_fail_hint_help() {
        let invocations: [&[&str]; 6] = [
            &["serch", "x"],
            &["serch"],
            &["serch", "x", "--depth", "2"],
            &["serch", "--help", "--depth", "2"],
            &["read", "x/y", "a.go", "--depth", "2"],
            &["read", "x/y", "--depth", "2"],
        ];
        let hints: Vec<String> = invocations
            .iter()
            .map(|argv| {
                let argv = argv.iter().map(std::ffi::OsString::from);
                let cli = crate::args::parse(argv).unwrap();
                validate(&cli).err().unwrap().hints().join(" | ")
            })
            .collect();
        expect_test::expect![[r#"
            sego search x
            sego search --help
            sego search --help
            sego search --help
            sego read x/y a.go
            sego read --help
        "#]]
        .assert_eq(&(hints.join("\n") + "\n"));
    }
}
