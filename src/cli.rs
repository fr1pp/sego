use std::ffi::OsString;

use lexopt::Arg;

use crate::address::{self, Address, LineRange, Repo, Rev};
use crate::error::Error;

pub const SYNOPSIS: &str = "sego COMMAND [--json] ...";

pub const SEARCH_SYNOPSIS: &str = "sego search [--json] QUERY...";
pub const SHOW_SYNOPSIS: &str = "sego show [--json] REPO[@REV]:PATH [LINES]";
pub const LS_SYNOPSIS: &str =
    "sego ls [--json] [--recursive] REPO[@REV][:PATH]";
pub const REFS_SYNOPSIS: &str = "sego refs [--json] REPO [TEXT]";

/// A command word, its synopsis and the text of its `--help`.
pub struct CommandEntry {
    pub name: &'static str,
    pub synopsis: &'static str,
    pub help: fn() -> String,
}

/// Every command. Parsing, the commands fact, the synopsis lookup and
/// `help` all read this table, so a command that is listed has a help topic.
pub const COMMANDS: &[CommandEntry] = &[
    CommandEntry {
        name: "search",
        synopsis: SEARCH_SYNOPSIS,
        help: search_help,
    },
    CommandEntry { name: "show", synopsis: SHOW_SYNOPSIS, help: show_help },
    CommandEntry { name: "ls", synopsis: LS_SYNOPSIS, help: ls_help },
    CommandEntry { name: "refs", synopsis: REFS_SYNOPSIS, help: refs_help },
];

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Search { query: String, json: bool },
    Show { address: Address, lines: Option<LineRange>, json: bool },
    Ls { address: Address, recursive: bool, json: bool },
    Refs { repo: Repo, text: Option<String>, json: bool },
    Help(Option<&'static str>),
    Version,
}

#[derive(Default)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "one independent switch per option"
)]
struct Flags {
    json: bool,
    recursive: bool,
    help: bool,
    version: bool,
}

/// `args` excludes the program name.
pub fn parse(args: &[OsString]) -> Result<Command, Error> {
    let args = utf8(args)?;
    let mut parser = lexopt::Parser::from_args(&args);
    let mut flags = Flags::default();
    let mut operands: Vec<String> = Vec::new();
    loop {
        let synopsis = synopsis_of(operands.first().map(String::as_str));
        if operands.first().is_some_and(|word| word == "search")
            && let Some(mut raw) = parser.try_raw_args()
            && let Some(token) = raw.next_if(is_query_dash)
        {
            operands.push(token.to_string_lossy().into_owned());
            continue;
        }
        let twice =
            |name: &str| Error::usage(format!("{name} given twice"), synopsis);
        let arg = match parser.next() {
            Ok(Some(arg)) => arg,
            Ok(None) => break,
            Err(lexopt::Error::UnexpectedValue { option, .. }) => {
                return Err(Error::usage(
                    format!("{option} takes no value"),
                    synopsis,
                ));
            }
            Err(other) => {
                return Err(Error::usage(other.to_string(), synopsis));
            }
        };
        match arg {
            Arg::Long("json") if flags.json => return Err(twice("--json")),
            Arg::Long("json") => flags.json = true,
            Arg::Long("recursive") if flags.recursive => {
                return Err(twice("--recursive"));
            }
            Arg::Long("recursive") => flags.recursive = true,
            Arg::Long("help") if flags.help => return Err(twice("--help")),
            Arg::Short('h') if flags.help => return Err(twice("-h")),
            Arg::Long("help") | Arg::Short('h') => flags.help = true,
            Arg::Long("version") if flags.version => {
                return Err(twice("--version"));
            }
            Arg::Long("version") => flags.version = true,
            Arg::Long(name) => {
                return Err(unknown(&format!("--{name}"), synopsis));
            }
            Arg::Short(c) => {
                return Err(unknown(&short_token(&args, c), synopsis));
            }
            Arg::Value(value) => {
                operands.push(value.to_string_lossy().into_owned());
            }
        }
    }
    let command = match operands.split_first() {
        Some((word, rest)) => {
            match COMMANDS.iter().find(|entry| entry.name == word) {
                Some(entry) => Some((entry.name, rest)),
                None => return Err(unknown_command(word)),
            }
        }
        None => None,
    };
    if flags.help {
        return Ok(Command::Help(command.map(|(topic, _)| topic)));
    }
    if flags.version {
        return Ok(Command::Version);
    }
    let Some((name, rest)) = command else {
        return Err(
            Error::usage("no command given", SYNOPSIS).fact(commands_fact())
        );
    };
    if flags.recursive && name != "ls" {
        return Err(Error::usage(
            format!("{name} takes no --recursive"),
            synopsis_of(Some(name)),
        ));
    }
    match name {
        "search" => parse_search(rest, flags.json),
        "show" => parse_show(rest, flags.json),
        "ls" => parse_ls(rest, flags.recursive, flags.json),
        "refs" => parse_refs(rest, flags.json),
        other => Err(unknown_command(other)),
    }
}

/// A token with exactly one leading dash, other than `-h`, is query text.
fn is_query_dash(token: &std::ffi::OsStr) -> bool {
    token.to_str().is_none_or(|text| {
        text.len() > 1
            && text.starts_with('-')
            && !text.starts_with("--")
            && text != "-h"
    })
}

fn parse_search(operands: &[String], json: bool) -> Result<Command, Error> {
    let query = operands.join(" ");
    if query.trim().is_empty() {
        return Err(Error::usage("search needs a query", SEARCH_SYNOPSIS));
    }
    Ok(Command::Search { query, json })
}

fn utf8(args: &[OsString]) -> Result<Vec<String>, Error> {
    args.iter()
        .enumerate()
        .map(|(i, arg)| {
            arg.to_str().map(str::to_owned).ok_or_else(|| {
                Error::usage(
                    format!("argument {} is not valid UTF-8", i + 1),
                    synopsis_before(&args[..i]),
                )
            })
        })
        .collect()
}

/// The synopsis of the command named among `earlier` arguments, which are
/// all valid UTF-8.
fn synopsis_before(earlier: &[OsString]) -> &'static str {
    let mut options_ended = false;
    for arg in earlier {
        let word = arg.to_string_lossy();
        if options_ended || !word.starts_with('-') {
            return synopsis_of(Some(&word));
        }
        options_ended = word == "--";
    }
    SYNOPSIS
}

fn synopsis_of(command: Option<&str>) -> &'static str {
    command
        .and_then(|word| COMMANDS.iter().find(|entry| entry.name == word))
        .map_or(SYNOPSIS, |entry| entry.synopsis)
}

fn commands_fact() -> String {
    let names: Vec<&str> = COMMANDS.iter().map(|entry| entry.name).collect();
    format!("commands: {}", names.join(", "))
}

fn unknown_command(word: &str) -> Error {
    Error::usage(format!("unknown command '{word}'"), SYNOPSIS)
        .fact(commands_fact())
}

/// lexopt splits `-json` into short options; the whole token is what the
/// caller typed.
fn short_token(args: &[String], c: char) -> String {
    args.iter()
        .take_while(|arg| *arg != "--")
        .find(|arg| {
            arg.strip_prefix('-')
                .is_some_and(|rest| !rest.starts_with('-') && rest.contains(c))
        })
        .cloned()
        .unwrap_or_else(|| format!("-{c}"))
}

fn unknown(option: &str, synopsis: &'static str) -> Error {
    let error = Error::usage(format!("unknown option {option}"), synopsis);
    let fact = match option {
        "--limit" | "--count" => {
            format!("sego has no {option}; count: is a query filter")
        }
        "--context" => "sego has no --context; search prints one line of \
                        context, show prints any range"
            .to_owned(),
        "--repo" | "--lang" | "--file" => {
            let filter = &option[2..];
            format!("sego has no {option}; {filter}: is a query filter")
        }
        _ => return error,
    };
    error.fact(fact)
}

fn parse_show(operands: &[String], json: bool) -> Result<Command, Error> {
    let usage = |message: String| Error::usage(message, SHOW_SYNOPSIS);
    let [token, rest @ ..] = operands else {
        return Err(usage(
            "show needs an address, like REPO[@REV]:PATH".to_owned(),
        ));
    };
    if rest.len() > 1 {
        return Err(usage(
            "show takes an address and at most one LINES operand".to_owned(),
        ));
    }
    let address = address::parse(token).map_err(|e| usage(e.to_string()))?;
    if let Some(message) = range_message(&address.rev) {
        return Err(usage(message));
    }
    let names_root =
        address.path.as_deref().is_none_or(|path| matches!(path, "" | "."));
    if names_root && !matches!(address.rev, Rev::Split(_)) {
        return Err(usage(
            "show needs a file's path, like REPO[@REV]:PATH".to_owned(),
        ));
    }
    let operand = rest
        .first()
        .map(|text| address::parse_lines(text))
        .transpose()
        .map_err(|e| usage(e.to_string()))?;
    let lines = match (address.lines, operand) {
        (Some(url), Some(operand)) if url != operand => {
            return Err(usage(format!(
                "the URL's lines {} and the LINES operand {} disagree",
                url.display(),
                operand.display()
            )));
        }
        (url, operand) => url.or(operand),
    };
    Ok(Command::Show { address, lines, json })
}

fn range_message(rev: &Rev) -> Option<String> {
    let Rev::Range { base, head, dots } = rev else {
        return None;
    };
    let dots = match dots {
        address::Dots::Two => "..",
        address::Dots::Three => "...",
    };
    Some(format!(
        "a range is only valid in log (A..B) and diff (A...B): \
         {base}{dots}{head}"
    ))
}

fn parse_ls(
    operands: &[String],
    recursive: bool,
    json: bool,
) -> Result<Command, Error> {
    let usage = |message: String| Error::usage(message, LS_SYNOPSIS);
    let [token] = operands else {
        return Err(usage(if operands.is_empty() {
            "ls needs an address, like REPO[@REV][:PATH]".to_owned()
        } else {
            "ls takes one address".to_owned()
        }));
    };
    let address = address::parse(token).map_err(|e| usage(e.to_string()))?;
    if let Some(message) = range_message(&address.rev) {
        return Err(usage(message));
    }
    if address.lines.is_some() {
        return Err(usage("ls takes no LINES".to_owned()));
    }
    Ok(Command::Ls { address, recursive, json })
}

fn parse_refs(operands: &[String], json: bool) -> Result<Command, Error> {
    let usage = |message: String| Error::usage(message, REFS_SYNOPSIS);
    let [token, rest @ ..] = operands else {
        return Err(usage("refs needs a repository, like REPO".to_owned()));
    };
    if rest.len() > 1 {
        return Err(usage(
            "refs takes a repository and at most one TEXT".to_owned(),
        ));
    }
    let address = address::parse(token).map_err(|e| usage(e.to_string()))?;
    let has_path =
        address.path.as_deref().is_some_and(|path| !path.is_empty());
    if address.rev != Rev::Default || has_path {
        return Err(usage(format!(
            "refs names a repository, not a revision or path: {token}"
        )));
    }
    let text = rest.first().cloned();
    if text.as_deref().is_some_and(|text| text.trim().is_empty()) {
        return Err(usage("the TEXT operand is empty".to_owned()));
    }
    Ok(Command::Refs { repo: address.repo, text, json })
}

const ADDRESSES: &str = "\
addresses:
  REPO     host/owner/name, owner/name, or a GitHub, GitLab or
           Sourcegraph URL
  @REV     a branch, tag or commit; the default branch without it
  :PATH    a path from the repository root
  LINES    N, N-M, N,M or N- (lines are numbered from 1)
";

const OPTIONS: &str = "\
options:
  --json       one JSON document on stdout
  -h, --help   this help, or a command's help after its name
  --version    sego's version
";

const SEARCH_QUERY: &str = "\
query:
  -term          literal text, not an option (-h is still help)
  and, or, not   operators, in any case
  count:, repo:, lang:, file:
                 query filters
  -- TEXT        text that is one of sego's options, such as --json
";

const LS_HELP: &str = "\
usage: sego ls [--json] [--recursive] REPO[@REV][:PATH]

addresses:
  REPO     host/owner/name, owner/name, or a GitHub, GitLab or
           Sourcegraph URL
  @REV     a branch, tag or commit; the default branch without it
  :PATH    a directory from the repository root; the root without it

options:
  --json        one JSON document on stdout
  --recursive   the whole subtree, not one directory
  -h, --help    this help, or a command's help after its name
  --version     sego's version

Rows are paths from the repository root, sorted segment by segment.
Text prints 200 entries; --json carries every entry.
";

const REFS_HELP: &str = "\
usage: sego refs [--json] REPO [TEXT]

operands:
  REPO     host/owner/name, owner/name, or a GitHub, GitLab or
           Sourcegraph URL
  TEXT     only refs whose name contains TEXT, in any case

options:
  --json       one JSON document on stdout
  -h, --help   this help, or a command's help after its name
  --version    sego's version

Branches come default first, then newest first; tags newest first.
Text prints 100 branches and 100 tags; --json carries up to 17000 of each.
";

fn search_help() -> String {
    format!(
        "usage: {SEARCH_SYNOPSIS}\n\n\
         QUERY... is joined with single spaces and sent as one \
         Sourcegraph query,\nwith count:100 appended when the query has \
         no count:.\n\n{SEARCH_QUERY}\n{OPTIONS}\n\
         Search prints one line of context around each match.\n\
         Text prints the first 100 matches; --json carries every result \
         fetched.\n"
    )
}

fn show_help() -> String {
    format!(
        "usage: {SHOW_SYNOPSIS}\n\n{ADDRESSES}\n{OPTIONS}\n\
         A file over 1.5 MiB is read through Sourcegraph's raw route, a \
         second request.\n\
         Text shows lines 1-500 when LINES is not given; --json \
         carries the same lines.\n"
    )
}

fn ls_help() -> String {
    LS_HELP.to_owned()
}

fn refs_help() -> String {
    REFS_HELP.to_owned()
}

pub fn help(topic: Option<&str>) -> String {
    if let Some(entry) =
        topic.and_then(|word| COMMANDS.iter().find(|e| e.name == word))
    {
        return (entry.help)();
    }
    format!(
        "\
sego reads public source code at any revision through sourcegraph.com.

usage: {SYNOPSIS}

commands:
  search QUERY...                code matching a Sourcegraph query
  show REPO[@REV]:PATH [LINES]   lines of a file
  ls REPO[@REV][:PATH]           entries of a directory
  refs REPO [TEXT]               branches and tags

{ADDRESSES}
{OPTIONS}
environment:
  SEGO_ENDPOINT   the Sourcegraph instance (default
                  https://sourcegraph.com)

exit codes: 0 answered, 1 not answered, 2 the command line is wrong
"
    )
}
