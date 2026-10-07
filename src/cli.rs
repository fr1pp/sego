use std::ffi::OsString;

use lexopt::Arg;

use crate::address::{self, Address, LineRange, Rev};
use crate::error::Error;

pub const SYNOPSIS: &str = "sego COMMAND [--json] ...";
const COMMANDS_FACT: &str = "commands: search, show";

pub const SEARCH_SYNOPSIS: &str = "sego search [--json] QUERY...";
pub const SHOW_SYNOPSIS: &str = "sego show [--json] REPO[@REV]:PATH [LINES]";

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Search { query: String, json: bool },
    Show { address: Address, lines: Option<LineRange>, json: bool },
    Help(Option<&'static str>),
    Version,
}

#[derive(Default)]
struct Flags {
    json: bool,
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
        let synopsis = synopsis_of(operands.first());
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
        Some((command, rest)) => match command.as_str() {
            "search" => Some(("search", rest)),
            "show" => Some(("show", rest)),
            _ => return Err(unknown_command(command)),
        },
        None => None,
    };
    if flags.help {
        return Ok(Command::Help(command.map(|(topic, _)| topic)));
    }
    if flags.version {
        return Ok(Command::Version);
    }
    match command {
        None => {
            Err(Error::usage("no command given", SYNOPSIS).fact(COMMANDS_FACT))
        }
        Some(("search", rest)) => parse_search(rest, flags.json),
        Some((_, rest)) => parse_show(rest, flags.json),
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
            return synopsis_of(Some(&word.into_owned()));
        }
        options_ended = word == "--";
    }
    SYNOPSIS
}

fn synopsis_of(command: Option<&String>) -> &'static str {
    match command.map(String::as_str) {
        Some("search") => SEARCH_SYNOPSIS,
        Some("show") => SHOW_SYNOPSIS,
        _ => SYNOPSIS,
    }
}

fn unknown_command(word: &str) -> Error {
    Error::usage(format!("unknown command '{word}'"), SYNOPSIS)
        .fact(COMMANDS_FACT)
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
    if let Rev::Range { base, head, dots } = &address.rev {
        let dots = match dots {
            address::Dots::Two => "..",
            address::Dots::Three => "...",
        };
        return Err(usage(format!(
            "a range is only valid in log (A..B) and diff (A...B): \
             {base}{dots}{head}"
        )));
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

pub fn help(topic: Option<&str>) -> String {
    match topic {
        Some("search") => format!(
            "usage: {SEARCH_SYNOPSIS}\n\n\
             QUERY... is joined with single spaces and sent as one \
             Sourcegraph query,\nwith count:100 appended when the query has \
             no count:.\n\n{SEARCH_QUERY}\n{OPTIONS}\n\
             Search prints one line of context around each match.\n\
             Text prints the first 100 matches; --json carries every result \
             fetched.\n"
        ),
        Some("show") => format!(
            "usage: {SHOW_SYNOPSIS}\n\n{ADDRESSES}\n{OPTIONS}\n\
             A file over 1.5 MiB is read through Sourcegraph's raw route, a \
             second request.\n\
             Text shows lines 1-500 when LINES is not given; --json \
             carries the same lines.\n"
        ),
        _ => format!(
            "\
sego reads public source code at any revision through sourcegraph.com.

usage: {SYNOPSIS}

commands:
  search QUERY...                code matching a Sourcegraph query
  show REPO[@REV]:PATH [LINES]   lines of a file

{ADDRESSES}
{OPTIONS}
environment:
  SEGO_ENDPOINT   the Sourcegraph instance (default
                  https://sourcegraph.com)

exit codes: 0 answered, 1 not answered, 2 the command line is wrong
"
        ),
    }
}
