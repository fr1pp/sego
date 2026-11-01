pub mod help;

use std::ffi::OsString;

use lexopt::Arg;

use crate::address::{self, Dots, Rev, Target};
use crate::commands::{Command, Revs, diff, log, ls, refs, search, show};
use crate::error::Error;
use crate::sourcegraph::COMMIT_PAGE;

pub const SYNOPSIS: &str = "sego COMMAND [--json] ...";

pub const SEARCH_SYNOPSIS: &str = "sego search [--json] QUERY...";
pub const SHOW_SYNOPSIS: &str = "\
sego show [--json] REPO[@REV]:PATH [LINES]\n\
sego show [--json] REPO@REV";
pub const LS_SYNOPSIS: &str = "sego ls [--json] [--recursive] REPO[@REV][:PATH]";
pub const REFS_SYNOPSIS: &str = "sego refs [--json] REPO [TEXT]";
pub const LOG_SYNOPSIS: &str = "\
sego log [--json] [-n N] [--skip N] REPO[@REV][:PATH] [PATH]\n\
sego log [--json] [-n N] [--skip N] REPO@A..B[:PATH] [PATH]\n\
sego log [--json] [-n N] --follow REPO[@REV][:PATH] [PATH]";
pub const DIFF_SYNOPSIS: &str = "\
sego diff [--json] REPO@A...B[:PATH] [PATH...]\n\
sego diff [--json] REPO@REV[:PATH] [PATH...]";

/// A command word, its synopsis and the text of its `--help`.
pub struct CommandEntry {
    pub name: &'static str,
    pub synopsis: &'static str,
    pub help: fn() -> String,
}

/// Every command. Parsing, the commands fact, the synopsis lookup and
/// `help` all read this table, so a command that is listed has a help topic.
pub const COMMANDS: &[CommandEntry] = &[
    CommandEntry { name: "search", synopsis: SEARCH_SYNOPSIS, help: help::search },
    CommandEntry { name: "show", synopsis: SHOW_SYNOPSIS, help: help::show },
    CommandEntry { name: "ls", synopsis: LS_SYNOPSIS, help: help::ls },
    CommandEntry { name: "refs", synopsis: REFS_SYNOPSIS, help: help::refs },
    CommandEntry { name: "log", synopsis: LOG_SYNOPSIS, help: help::log },
    CommandEntry { name: "diff", synopsis: DIFF_SYNOPSIS, help: help::diff },
];

/// What a command line asks for.
#[derive(Debug, PartialEq, Eq)]
pub enum Invocation {
    /// The help of a command, or the general help for `None`.
    Help(Option<&'static str>),
    Version,
    Command(Command),
}

/// The synopsis of `command`, which a usage error found from its response
/// carries.
pub fn synopsis(command: &Command) -> &'static str {
    match command {
        Command::Search(_) => SEARCH_SYNOPSIS,
        Command::Show(_) | Command::ShowCommit(_) => SHOW_SYNOPSIS,
        Command::Ls(_) => LS_SYNOPSIS,
        Command::Refs(_) => REFS_SYNOPSIS,
        Command::Log(_) => LOG_SYNOPSIS,
        Command::Diff(_) => DIFF_SYNOPSIS,
    }
}

#[derive(Default)]
#[expect(clippy::struct_excessive_bools, reason = "one independent switch per option")]
struct Flags {
    json: bool,
    recursive: bool,
    follow: bool,
    help: bool,
    version: bool,
    count: Option<u32>,
    skip: Option<u32>,
}

/// `args` excludes the program name.
pub fn parse(args: &[OsString]) -> Result<Invocation, Error> {
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
        let twice = |name: &str| Error::usage(format!("{name} given twice"), synopsis);
        let arg = match parser.next() {
            Ok(Some(arg)) => arg,
            Ok(None) => break,
            Err(lexopt::Error::UnexpectedValue { option, .. }) => {
                return Err(Error::usage(format!("{option} takes no value"), synopsis));
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
            Arg::Long("follow") if flags.follow => return Err(twice("--follow")),
            Arg::Long("follow") => flags.follow = true,
            Arg::Short('n') if flags.count.is_some() => return Err(twice("-n")),
            Arg::Short('n') => flags.count = Some(count(&mut parser, synopsis)?),
            Arg::Long("skip") if flags.skip.is_some() => return Err(twice("--skip")),
            Arg::Long("skip") => flags.skip = Some(skip(&mut parser, synopsis)?),
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
        Some((word, rest)) => match COMMANDS.iter().find(|entry| entry.name == word) {
            Some(entry) => Some((entry.name, rest)),
            None => return Err(unknown_command(word)),
        },
        None => None,
    };
    if flags.help {
        return Ok(Invocation::Help(command.map(|(topic, _)| topic)));
    }
    if flags.version {
        return Ok(Invocation::Version);
    }
    let Some((name, rest)) = command else {
        return Err(Error::usage("no command given", SYNOPSIS).fact(commands_fact()));
    };
    if flags.recursive && name != "ls" {
        return Err(Error::usage(format!("{name} takes no --recursive"), synopsis_of(Some(name))));
    }
    let log_only = [
        (flags.follow, "--follow"),
        (flags.count.is_some(), "-n"),
        (flags.skip.is_some(), "--skip"),
    ];
    for (given, option) in log_only {
        if given && name != "log" {
            return Err(Error::usage(format!("{name} takes no {option}"), synopsis_of(Some(name))));
        }
    }
    let command = match name {
        "search" => parse_search(rest, flags.json),
        "show" => parse_show(rest, flags.json),
        "ls" => parse_ls(rest, flags.recursive, flags.json),
        "refs" => parse_refs(rest, flags.json),
        "log" => parse_log(rest, &flags),
        "diff" => parse_diff(rest, flags.json),
        other => Err(unknown_command(other)),
    };
    command.map(Invocation::Command)
}

/// The value of the option `parser` has just read.
fn value(parser: &mut lexopt::Parser, synopsis: &'static str) -> Result<String, Error> {
    let value = parser.value().map_err(|e| Error::usage(e.to_string(), synopsis))?;
    Ok(value.to_string_lossy().into_owned())
}

fn count(parser: &mut lexopt::Parser, synopsis: &'static str) -> Result<u32, Error> {
    let text = value(parser, synopsis)?;
    text.parse().ok().filter(|&count| count > 0).ok_or_else(|| {
        Error::usage(format!("-n takes a whole number of at least 1: {text}"), synopsis)
    })
}

fn skip(parser: &mut lexopt::Parser, synopsis: &'static str) -> Result<u32, Error> {
    let text = value(parser, synopsis)?;
    text.parse().map_err(|_| Error::usage(format!("--skip takes a whole number: {text}"), synopsis))
}

/// A token with exactly one leading dash, other than `-h`, is query text.
fn is_query_dash(token: &std::ffi::OsStr) -> bool {
    token.to_str().is_none_or(|text| {
        text.len() > 1 && text.starts_with('-') && !text.starts_with("--") && text != "-h"
    })
}

fn parse_search(operands: &[String], json: bool) -> Result<Command, Error> {
    let query = operands.join(" ");
    if query.trim().is_empty() {
        return Err(Error::usage("search needs a query", SEARCH_SYNOPSIS));
    }
    Ok(Command::Search(search::Args { query, json }))
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
    Error::usage(format!("unknown command '{word}'"), SYNOPSIS).fact(commands_fact())
}

/// lexopt splits `-json` into short options; the whole token is what the
/// caller typed.
fn short_token(args: &[String], c: char) -> String {
    args.iter()
        .take_while(|arg| *arg != "--")
        .find(|arg| {
            arg.strip_prefix('-').is_some_and(|rest| !rest.starts_with('-') && rest.contains(c))
        })
        .cloned()
        .unwrap_or_else(|| format!("-{c}"))
}

fn unknown(option: &str, synopsis: &'static str) -> Error {
    Error::usage(format!("unknown option {option}"), synopsis)
}

fn parse_show(operands: &[String], json: bool) -> Result<Command, Error> {
    let usage = |message: String| Error::usage(message, SHOW_SYNOPSIS);
    let [token, rest @ ..] = operands else {
        return Err(usage("show needs an address, like REPO[@REV]:PATH".to_owned()));
    };
    if rest.len() > 1 {
        return Err(usage("show takes an address and at most one LINES operand".to_owned()));
    }
    let address = address::parse(token).map_err(|e| usage(e.to_string()))?;
    let target = match (address.rev, address.path) {
        (Rev::Range { base, head, dots }, _) => {
            return Err(usage(range_message(&base, dots, &head)));
        }
        (Rev::One(rev), None) => {
            if !rest.is_empty() {
                return Err(usage("a commit takes no LINES".to_owned()));
            }
            return Ok(Command::ShowCommit(show::CommitArgs { repo: address.repo, rev, json }));
        }
        (Rev::Default, None) => {
            return Err(usage(
                "show needs a file's path or a revision, like \
                 REPO[@REV]:PATH or REPO@REV"
                    .to_owned(),
            ));
        }
        (Rev::One(rev), path) => Target { rev: Some(rev), path },
        (Rev::Default, path) => Target { rev: None, path },
    };
    let lines = rest
        .first()
        .map(|text| address::parse_lines(text))
        .transpose()
        .map_err(|e| usage(e.to_string()))?;
    Ok(Command::Show(show::Args { repo: address.repo, target, lines, json }))
}

/// The usage error for a range given to `show` or `ls`.
fn range_message(base: &str, dots: Dots, head: &str) -> String {
    format!("a range is only valid in log (A..B) and diff (A...B): {base}{}{head}", dots.as_str())
}

fn parse_ls(operands: &[String], recursive: bool, json: bool) -> Result<Command, Error> {
    let usage = |message: String| Error::usage(message, LS_SYNOPSIS);
    let [token] = operands else {
        return Err(usage(if operands.is_empty() {
            "ls needs an address, like REPO[@REV][:PATH]".to_owned()
        } else {
            "ls takes one address".to_owned()
        }));
    };
    let address = address::parse(token).map_err(|e| usage(e.to_string()))?;
    let target = match address.rev {
        Rev::Range { base, head, dots } => return Err(usage(range_message(&base, dots, &head))),
        Rev::One(rev) => Target { rev: Some(rev), path: address.path },
        Rev::Default => Target { rev: None, path: address.path },
    };
    Ok(Command::Ls(ls::Args { repo: address.repo, target, recursive, json }))
}

fn parse_refs(operands: &[String], json: bool) -> Result<Command, Error> {
    let usage = |message: String| Error::usage(message, REFS_SYNOPSIS);
    let [token, rest @ ..] = operands else {
        return Err(usage("refs needs a repository, like REPO".to_owned()));
    };
    if rest.len() > 1 {
        return Err(usage("refs takes a repository and at most one TEXT".to_owned()));
    }
    let address = address::parse(token).map_err(|e| usage(e.to_string()))?;
    let has_path = address.path.as_deref().is_some_and(|path| !path.is_empty());
    if address.rev != Rev::Default || has_path {
        return Err(usage(format!("refs names a repository, not a revision or path: {token}")));
    }
    let text = rest.first().cloned();
    if text.as_deref().is_some_and(|text| text.trim().is_empty()) {
        return Err(usage("the TEXT operand is empty".to_owned()));
    }
    Ok(Command::Refs(refs::Args { repo: address.repo, text, json }))
}

/// The path an operand names, or `None` for the root.
fn named_path(path: &str, synopsis: &'static str) -> Result<Option<String>, Error> {
    let path = address::parse_operand(path).map_err(|e| Error::usage(e.to_string(), synopsis))?;
    Ok(Some(path).filter(|path| !path.is_empty()))
}

fn parse_log(operands: &[String], flags: &Flags) -> Result<Command, Error> {
    let follow = flags.follow;
    let usage = |message: &str| Error::usage(message, LOG_SYNOPSIS);
    let [token, rest @ ..] = operands else {
        return Err(usage("log needs an address, like REPO[@REV][:PATH]"));
    };
    if rest.len() > 1 {
        return Err(usage("log takes an address and at most one PATH"));
    }
    let address = address::parse(token).map_err(|e| Error::usage(e.to_string(), LOG_SYNOPSIS))?;
    let revs = match address.rev {
        Rev::Range { dots: Dots::Three, .. } => {
            return Err(usage("log takes A..B, not A...B"));
        }
        Rev::Range { base, head, .. } => Revs::Range { base, head },
        Rev::One(rev) => Revs::One(Some(rev)),
        Rev::Default => Revs::One(None),
    };
    if rest.iter().any(String::is_empty) {
        return Err(usage("log takes no empty path"));
    }
    let in_address = address.path.filter(|path| !path.is_empty());
    let operand = rest.first().map(|path| named_path(path, LOG_SYNOPSIS)).transpose()?.flatten();
    let path = match (in_address, operand) {
        (Some(_), Some(_)) => return Err(usage("log takes one path")),
        (path, None) | (None, path) => path,
    };
    let plain = path.as_deref().is_some_and(address::is_plain_path);
    if follow && (!plain || matches!(revs, Revs::Range { .. })) {
        return Err(usage("--follow needs one revision and one plain path"));
    }
    if follow && flags.skip.is_some() {
        return Err(usage("--follow takes no --skip"));
    }
    if follow && flags.count.is_some_and(|count| count > COMMIT_PAGE) {
        return Err(usage(&log::follow_limit()));
    }
    Ok(Command::Log(log::Args {
        repo: address.repo,
        revs,
        path,
        follow,
        json: flags.json,
        count: flags.count,
        skip: flags.skip.unwrap_or(0),
    }))
}

fn parse_diff(operands: &[String], json: bool) -> Result<Command, Error> {
    let usage = |message: &str| Error::usage(message, DIFF_SYNOPSIS);
    let [token, rest @ ..] = operands else {
        return Err(usage("diff needs an address, like REPO@A...B or REPO@REV"));
    };
    let address = address::parse(token).map_err(|e| Error::usage(e.to_string(), DIFF_SYNOPSIS))?;
    let revs = match address.rev {
        Rev::Range { dots: Dots::Two, .. } => {
            return Err(usage("diff takes A...B (Sourcegraph compares from the merge base)"));
        }
        Rev::Range { base, head, .. } => Revs::Range { base, head },
        Rev::One(rev) => Revs::One(Some(rev)),
        Rev::Default => {
            return Err(usage(
                "diff needs a revision or a range, like REPO@REV or \
                 REPO@A...B",
            ));
        }
    };
    if rest.iter().any(String::is_empty) {
        return Err(usage("diff takes no empty path"));
    }
    let mut paths: Vec<String> = Vec::new();
    let in_address = address.path.into_iter().filter(|path| !path.is_empty());
    let operands = rest.iter().map(|path| named_path(path, DIFF_SYNOPSIS));
    for path in in_address.map(Ok).chain(operands.filter_map(Result::transpose)) {
        let path = path?;
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
    Ok(Command::Diff(diff::Args { repo: address.repo, revs, paths, json }))
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::ffi::OsString;
    use std::fmt::Write as _;
    use std::os::unix::ffi::OsStringExt;

    use crate::address::{self, End, LineRange, Repo, Rev, Target};
    use crate::commands::{Command, Revs, diff, log, ls, refs, search, show};
    use crate::error::{Kind, report, usage_block};

    const COMMAND_LIST: &str = "commands: search, show, ls, refs, log, diff";
    const NEEDS_SHOW_ADDRESS: &str = "show needs an address, like REPO[@REV]:PATH";
    const RANGE_ONLY_IN_HISTORY: &str = "a range is only valid in log (A..B) and diff (A...B)";

    fn os(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    fn address(token: &str) -> address::Address {
        address::parse(token).unwrap_or_else(|e| panic!("{token}: {e}"))
    }

    fn lines(start: u32, end: u32) -> LineRange {
        LineRange { start, end: End::Line(end) }
    }

    fn search(query: &str, json: bool) -> Invocation {
        Invocation::Command(Command::Search(search::Args { query: query.to_owned(), json }))
    }

    /// What `show` and `ls` read at the address `token`.
    fn target(token: &str) -> (Repo, Target) {
        let address = address(token);
        let target = match address.rev {
            Rev::One(rev) => Target { rev: Some(rev), path: address.path },
            Rev::Default => Target { rev: None, path: address.path },
            Rev::Range { .. } => panic!("{token} is a range"),
        };
        (address.repo, target)
    }

    fn show(token: &str, lines: Option<LineRange>, json: bool) -> Invocation {
        let (repo, target) = target(token);
        Invocation::Command(Command::Show(show::Args { repo, target, lines, json }))
    }

    fn commit(token: &str, json: bool) -> Invocation {
        let address = address(token);
        let Rev::One(rev) = address.rev else { panic!("{token} names no one revision") };
        Invocation::Command(Command::ShowCommit(show::CommitArgs { repo: address.repo, rev, json }))
    }

    fn ls(token: &str, recursive: bool, json: bool) -> Invocation {
        let (repo, target) = target(token);
        Invocation::Command(Command::Ls(ls::Args { repo, target, recursive, json }))
    }

    fn refs(token: &str, text: Option<&str>, json: bool) -> Invocation {
        let text = text.map(str::to_owned);
        Invocation::Command(Command::Refs(refs::Args { repo: address(token).repo, text, json }))
    }

    fn log(token: &str, path: Option<&str>, json: bool) -> Invocation {
        let address = address(token);
        let revs = match address.rev {
            Rev::Range { base, head, .. } => Revs::Range { base, head },
            Rev::One(rev) => Revs::One(Some(rev)),
            Rev::Default => Revs::One(None),
        };
        Invocation::Command(Command::Log(log::Args {
            repo: address.repo,
            revs,
            path: path.map(str::to_owned),
            follow: false,
            json,
            count: None,
            skip: 0,
        }))
    }

    #[test]
    fn follow_belongs_to_a_log_of_one_revision_and_one_plain_path() {
        let followed = |token: &str, path: &str| {
            let Invocation::Command(Command::Log(args)) = log(token, Some(path), false) else {
                panic!("not a log")
            };
            Invocation::Command(Command::Log(log::Args { follow: true, ..args }))
        };
        check_parses(vec![
            (vec!["log", "--follow", "x/y@v1", "a.go"], followed("x/y@v1", "a.go")),
            (vec!["log", "x/y:src/a.go", "--follow"], followed("x/y", "src/a.go")),
        ]);
        let needs = "--follow needs one revision and one plain path";
        check_errors(
            LOG_SYNOPSIS,
            &[
                (&["log", "--follow", "x/y"], needs),
                (&["log", "--follow", "x/y@a..b", "a.go"], needs),
                (&["log", "--follow", "x/y", "*.go"], needs),
                (&["log", "--follow", "--follow", "x/y", "a.go"], "--follow given twice"),
            ],
        );
        check_errors(DIFF_SYNOPSIS, &[(&["diff", "--follow", "x/y@v1"], "diff takes no --follow")]);
    }

    fn diff(token: &str, paths: &[&str], json: bool) -> Invocation {
        let address = address(token);
        let revs = match address.rev {
            Rev::Range { base, head, .. } => Revs::Range { base, head },
            Rev::One(rev) => Revs::One(Some(rev)),
            Rev::Default => panic!("{token} names no revision or range"),
        };
        let paths = paths.iter().map(|p| (*p).to_owned()).collect();
        Invocation::Command(Command::Diff(diff::Args { repo: address.repo, revs, paths, json }))
    }

    fn check_parses(table: Vec<(Vec<&str>, Invocation)>) {
        for (args, command) in table {
            let parsed = parse(&os(&args)).unwrap_or_else(|e| panic!("{args:?}: {e}"));
            assert_eq!(parsed, command, "{args:?}");
        }
    }

    /// What a usage error prints: the message, its facts indented, then the
    /// synopsis.
    fn reported(args: &[OsString]) -> String {
        let error = match parse(args) {
            Ok(command) => panic!("{args:?} parsed as {command:?}"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), Kind::Usage, "{args:?}");
        assert_eq!(error.exit_code(), 2);
        report(&error)
    }

    fn expected(message: &str, facts: &[&str], synopsis: &str) -> String {
        let mut text = format!("sego: error: {message}\n");
        for fact in facts {
            let _ = writeln!(text, "  {fact}");
        }
        let _ = writeln!(text, "{}", usage_block(synopsis));
        text
    }

    fn check_errors(synopsis: &str, table: &[(&[&str], &str)]) {
        for (args, message) in table {
            assert_eq!(reported(&os(args)), expected(message, &[], synopsis), "{args:?}");
        }
    }

    #[test]
    fn show_parses_a_file_address_with_lines_and_options() {
        check_parses(vec![
            (vec!["show", "github.com/x/y:a.go"], show("github.com/x/y:a.go", None, false)),
            (vec!["show", "x/y@v1:"], show("x/y@v1:", None, false)),
            (vec!["show", "x/y:."], show("x/y:.", None, false)),
            (vec!["show", "--json", "x/y:a.go"], show("x/y:a.go", None, true)),
            (vec!["show", "x/y:a.go", "--json"], show("x/y:a.go", None, true)),
            (vec!["--json", "show", "x/y:a.go"], show("x/y:a.go", None, true)),
            (vec!["show", "x/y@v1:a.go", "3-9"], show("x/y@v1:a.go", Some(lines(3, 9)), false)),
            (
                vec!["show", "x/y:a.go", "4-"],
                show("x/y:a.go", Some(LineRange { start: 4, end: End::Open }), false),
            ),
            (vec!["show", "x/y:a.go", "3,9"], show("x/y:a.go", Some(lines(3, 9)), false)),
            (vec!["show", "--", "x/y:a.go"], show("x/y:a.go", None, false)),
        ]);
    }

    #[test]
    fn show_of_a_revision_is_a_commit() {
        check_parses(vec![
            (vec!["show", "x/y@v1"], commit("x/y@v1", false)),
            (vec!["show", "--json", "x/y@v1"], commit("x/y@v1", true)),
            (vec!["show", "x/y@v1", "--json"], commit("x/y@v1", true)),
            (vec!["show", "github.com/x/y@HEAD~2"], commit("github.com/x/y@HEAD~2", false)),
        ]);
    }

    #[test]
    fn help_and_version_parse_with_or_without_a_command() {
        check_parses(vec![
            (vec!["--help"], Invocation::Help(None)),
            (vec!["-h"], Invocation::Help(None)),
            (vec!["show", "--help"], Invocation::Help(Some("show"))),
            (vec!["-h", "show"], Invocation::Help(Some("show"))),
            (vec!["show", "-h", "--json"], Invocation::Help(Some("show"))),
            (vec!["log", "--help"], Invocation::Help(Some("log"))),
            (vec!["-h", "diff"], Invocation::Help(Some("diff"))),
            (vec!["diff", "x/y@v1", "-h"], Invocation::Help(Some("diff"))),
            (vec!["ls", "--help"], Invocation::Help(Some("ls"))),
            (vec!["-h", "ls"], Invocation::Help(Some("ls"))),
            (vec!["ls", "--recursive", "--help"], Invocation::Help(Some("ls"))),
            (vec!["refs", "--help"], Invocation::Help(Some("refs"))),
            (vec!["refs", "x/y", "-h"], Invocation::Help(Some("refs"))),
            (vec!["search", "foo", "-h"], Invocation::Help(Some("search"))),
            (vec!["search", "--help"], Invocation::Help(Some("search"))),
            (vec!["-h", "search"], Invocation::Help(Some("search"))),
            (vec!["--version"], Invocation::Version),
            (vec!["show", "--version"], Invocation::Version),
            (vec!["ls", "--version"], Invocation::Version),
            (vec!["search", "--version"], Invocation::Version),
        ]);
    }

    #[test]
    fn search_joins_its_operands_into_a_query() {
        check_parses(vec![
            (vec!["search", "repo:x", "foo", "bar", "--json"], search("repo:x foo bar", true)),
            (
                vec!["search", "-file:_test\\.go", "NewRouter"],
                search("-file:_test\\.go NewRouter", false),
            ),
            (vec!["search", "-C", "3", "foo"], search("-C 3 foo", false)),
            (vec!["search", "foo", "-n"], search("foo -n", false)),
            (vec!["search", "--", "--json"], search("--json", false)),
            (vec!["search", "--", "-h"], search("-h", false)),
            (vec!["search", "--json", "--", "-x"], search("-x", true)),
            (vec!["--json", "search", "foo"], search("foo", true)),
        ]);
    }

    #[test]
    fn ls_parses_an_address_with_recursive_and_json() {
        check_parses(vec![
            (vec!["ls", "x/y"], ls("x/y", false, false)),
            (vec!["ls", "x/y:src"], ls("x/y:src", false, false)),
            (vec!["ls", "x/y@v1:src"], ls("x/y@v1:src", false, false)),
            (vec!["ls", "x/y:"], ls("x/y:", false, false)),
            (vec!["ls", "x/y:."], ls("x/y:.", false, false)),
            (vec!["ls", "--json", "x/y"], ls("x/y", false, true)),
            (vec!["ls", "x/y", "--recursive"], ls("x/y", true, false)),
            (vec!["--recursive", "ls", "x/y"], ls("x/y", true, false)),
            (vec!["ls", "--recursive", "--json", "x/y:src"], ls("x/y:src", true, true)),
            (vec!["ls", "--", "x/y"], ls("x/y", false, false)),
        ]);
    }

    #[test]
    fn refs_parses_a_repository_and_text() {
        check_parses(vec![
            (vec!["refs", "x/y"], refs("x/y", None, false)),
            (vec!["refs", "--json", "x/y"], refs("x/y", None, true)),
            (vec!["refs", "x/y", "v1"], refs("x/y", Some("v1"), false)),
            (vec!["refs", "x/y", "a b "], refs("x/y", Some("a b "), false)),
        ]);
    }

    #[test]
    fn log_parses_an_address_and_one_path() {
        check_parses(vec![
            (vec!["log", "x/y"], log("x/y", None, false)),
            (vec!["log", "--json", "x/y"], log("x/y", None, true)),
            (vec!["log", "x/y", "--json"], log("x/y", None, true)),
            (vec!["log", "x/y@v1"], log("x/y@v1", None, false)),
            (vec!["log", "x/y@a..b"], log("x/y@a..b", None, false)),
            (vec!["log", "x/y:src"], log("x/y:src", Some("src"), false)),
            (vec!["log", "x/y@v1:src/"], log("x/y@v1:src/", Some("src"), false)),
            (vec!["log", "x/y", "src"], log("x/y", Some("src"), false)),
            (vec!["log", "x/y", "/src/a.go/"], log("x/y", Some("src/a.go/"), false)),
            (vec!["log", "x/y@a..b:s"], log("x/y@a..b:s", Some("s"), false)),
            (vec!["log", "x/y@a..b", "s"], log("x/y@a..b", Some("s"), false)),
            (vec!["log", "x/y@v1:."], log("x/y@v1:.", None, false)),
            (vec!["log", "x/y@v1:"], log("x/y@v1:", None, false)),
            (vec!["log", "x/y@v1", "."], log("x/y@v1", None, false)),
            (vec!["log", "x/y@v1", "/"], log("x/y@v1", None, false)),
            (vec!["log", "x/y:.", "src"], log("x/y:.", Some("src"), false)),
            (vec!["log", "x/y:src", "."], log("x/y:src", Some("src"), false)),
            (vec!["log", "--", "x/y"], log("x/y", None, false)),
        ]);
    }

    #[test]
    fn diff_parses_a_revision_or_three_dot_range_and_paths() {
        check_parses(vec![
            (vec!["diff", "x/y@v1"], diff("x/y@v1", &[], false)),
            (vec!["diff", "--json", "x/y@v1"], diff("x/y@v1", &[], true)),
            (vec!["diff", "x/y@a...b"], diff("x/y@a...b", &[], false)),
            (vec!["diff", "x/y@a...b:src"], diff("x/y@a...b:src", &["src"], false)),
            (vec!["diff", "x/y@a...b", "src", "lib/"], diff("x/y@a...b", &["src", "lib/"], false)),
            (vec!["diff", "x/y@a...b:src", "/lib"], diff("x/y@a...b:src", &["src", "lib"], false)),
            (vec!["diff", "x/y@v1", "//a/b//", "c"], diff("x/y@v1", &["a/b/", "c"], false)),
            (vec!["diff", "x/y@v1...v2", "."], diff("x/y@v1...v2", &[], false)),
            (vec!["diff", "x/y@v1...v2", "/"], diff("x/y@v1...v2", &[], false)),
            (vec!["diff", "x/y@v1...v2:."], diff("x/y@v1...v2:.", &[], false)),
            (vec!["diff", "x/y@v1...v2", ".", "src", "/"], diff("x/y@v1...v2", &["src"], false)),
            (vec!["diff", "x/y@v1:"], diff("x/y@v1:", &[], false)),
        ]);
    }

    #[test]
    fn usage_errors_before_a_command_carry_the_general_synopsis() {
        let no_command = expected("no command given", &[COMMAND_LIST], SYNOPSIS);
        let unknown =
            |word: &str| expected(&format!("unknown command '{word}'"), &[COMMAND_LIST], SYNOPSIS);
        let table = [
            (vec![], no_command.clone()),
            (vec!["--json"], no_command),
            (vec!["shwo"], unknown("shwo")),
            (vec!["list", "x/y"], unknown("list")),
            (vec!["list", "x/y:a.go", "3-9"], unknown("list")),
            (vec!["find", "x"], unknown("find")),
            (vec!["shwo", "--help"], unknown("shwo")),
            (vec!["shwo", "--version"], unknown("shwo")),
            (vec!["-C", "search", "x"], expected("unknown option -C", &[], SYNOPSIS)),
            (vec!["-x", "search", "foo"], expected("unknown option -x", &[], SYNOPSIS)),
            (vec!["--json", "--json"], expected("--json given twice", &[], SYNOPSIS)),
            (vec!["--json=1"], expected("--json takes no value", &[], SYNOPSIS)),
            (vec!["--limit", "5", "show"], expected("unknown option --limit", &[], SYNOPSIS)),
        ];
        for (args, text) in table {
            assert_eq!(reported(&os(&args)), text, "{args:?}");
        }
    }

    #[test]
    fn usage_errors_in_the_options_of_show() {
        check_errors(
            SHOW_SYNOPSIS,
            &[
                (&["show", "--json", "--json", "x/y:a"], "--json given twice"),
                (&["show", "--json", "x/y:a", "--json"], "--json given twice"),
                (&["show", "--json=1", "x/y:a"], "--json takes no value"),
                (&["show", "--help=1"], "--help takes no value"),
                (&["show", "-x", "x/y:a"], "unknown option -x"),
                (&["show", "-json", "x/y:a"], "unknown option -json"),
                (&["show", "--recursive", "x/y:a"], "show takes no --recursive"),
            ],
        );
    }

    #[test]
    fn usage_errors_in_the_operands_of_show() {
        let needs_either = "show needs a file's path or a revision, like \
                        REPO[@REV]:PATH or REPO@REV";
        let range = |range: &str| format!("{RANGE_ONLY_IN_HISTORY}: {range}");
        check_errors(
            SHOW_SYNOPSIS,
            &[
                (&["show"], NEEDS_SHOW_ADDRESS),
                (&["show", "--json"], NEEDS_SHOW_ADDRESS),
                (
                    &["show", "x/y:a", "1", "2"],
                    "show takes an address and at most one LINES operand",
                ),
                (&["show", "github.com/x/y"], needs_either),
                (&["show", "x/y@a..b:f"], &range("a..b")),
                (&["show", "x/y@a...b:f"], &range("a...b")),
                (&["show", "x/y:f", "0"], "invalid line range 0; lines are numbered from 1"),
                (&["show", "x/y:f", "5-2"], "invalid line range 5-2; the end is before the start"),
                (&["show", "x/y@:f"], "the address has an empty revision: x/y@:f"),
            ],
        );
    }

    #[test]
    fn usage_errors_in_a_commit_address_of_show() {
        let needs_either = "show needs a file's path or a revision, like \
                        REPO[@REV]:PATH or REPO@REV";
        check_errors(
            SHOW_SYNOPSIS,
            &[
                (&["show", "x/y"], needs_either),
                (&["show", "x/y@v1", "3"], "a commit takes no LINES"),
                (&["show", "x/y@v1", "abc"], "a commit takes no LINES"),
                (
                    &["show", "x/y@v1", "1", "2"],
                    "show takes an address and at most one LINES operand",
                ),
            ],
        );
    }

    #[test]
    fn usage_errors_in_the_operands_of_ls() {
        let needs_address = "ls needs an address, like REPO[@REV][:PATH]";
        let range = |range: &str| format!("{RANGE_ONLY_IN_HISTORY}: {range}");
        check_errors(
            LS_SYNOPSIS,
            &[
                (&["ls"], needs_address),
                (&["ls", "--json", "--recursive"], needs_address),
                (&["ls", "x/y", "x/z"], "ls takes one address"),
                (&["ls", "x/y", "src", "a"], "ls takes one address"),
                (&["ls", "x/y@a..b"], &range("a..b")),
                (&["ls", "x/y@a...b:src"], &range("a...b")),
                (&["ls", "x/y@:src"], "the address has an empty revision: x/y@:src"),
            ],
        );
    }

    #[test]
    fn usage_errors_in_the_operands_of_refs() {
        let needs_repo = "refs needs a repository, like REPO";
        let names_repo =
            |token: &str| format!("refs names a repository, not a revision or path: {token}");
        check_errors(
            REFS_SYNOPSIS,
            &[
                (&["refs"], needs_repo),
                (&["refs", "--json"], needs_repo),
                (&["refs", "x/y", "a", "b"], "refs takes a repository and at most one TEXT"),
                (&["refs", "x/y@v1"], &names_repo("x/y@v1")),
                (&["refs", "github.com/x/y@v1"], &names_repo("github.com/x/y@v1")),
                (&["refs", "x/y:src"], &names_repo("x/y:src")),
                (&["refs", "x/y@v1:src", "t"], &names_repo("x/y@v1:src")),
                (&["refs", "x/y", ""], "the TEXT operand is empty"),
                (&["refs", "x/y", "  "], "the TEXT operand is empty"),
                (&["refs", "x/y@:"], "the address has an empty revision: x/y@:"),
            ],
        );
    }

    #[test]
    fn usage_errors_in_the_operands_of_log() {
        let needs_address = "log needs an address, like REPO[@REV][:PATH]";
        check_errors(
            LOG_SYNOPSIS,
            &[
                (&["log"], needs_address),
                (&["log", "--json"], needs_address),
                (&["log", "x/y", "a", "b"], "log takes an address and at most one PATH"),
                (&["log", "x/y@a...b"], "log takes A..B, not A...B"),
                (&["log", "x/y@a...b:src"], "log takes A..B, not A...B"),
                (&["log", "x/y", ""], "log takes no empty path"),
                (&["log", "x/y@v1:", ""], "log takes no empty path"),
                (&["log", "x/y@a..b", ""], "log takes no empty path"),
                (&["log", "x/y:src", ""], "log takes no empty path"),
                (&["log", "x/y:src", "lib"], "log takes one path"),
                (&["log", "x/y@a..b:src", "lib"], "log takes one path"),
                (&["log", "x/y@:src"], "the address has an empty revision: x/y@:src"),
            ],
        );
    }

    /// `log` of `token`, listing `count` commits after skipping `skip`.
    fn windowed(token: &str, count: Option<u32>, skip: u32) -> Invocation {
        let Invocation::Command(Command::Log(args)) = log(token, None, false) else {
            panic!("not a log")
        };
        Invocation::Command(Command::Log(log::Args { count, skip, ..args }))
    }

    #[test]
    fn log_takes_a_count_and_a_skip_anywhere_on_the_line() {
        check_parses(vec![
            (vec!["log", "-n", "5", "x/y"], windowed("x/y", Some(5), 0)),
            (vec!["log", "-n5", "x/y"], windowed("x/y", Some(5), 0)),
            (vec!["log", "x/y", "-n", "5"], windowed("x/y", Some(5), 0)),
            (vec!["log", "--skip", "10", "x/y"], windowed("x/y", None, 10)),
            (vec!["log", "--skip=10", "x/y"], windowed("x/y", None, 10)),
            (vec!["log", "--skip", "0", "x/y"], windowed("x/y", None, 0)),
            (vec!["log", "-n", "5", "--skip", "10", "x/y@a..b"], windowed("x/y@a..b", Some(5), 10)),
            (vec!["-n", "5", "log", "x/y"], windowed("x/y", Some(5), 0)),
        ]);
    }

    #[test]
    fn a_count_above_a_page_is_for_a_log_that_can_page() {
        check_parses(vec![
            (vec!["log", "-n", "20001", "x/y"], windowed("x/y", Some(20_001), 0)),
            (vec!["log", "-n", "4294967295", "x/y"], windowed("x/y", Some(u32::MAX), 0)),
        ]);
        let limit = "--follow lists at most 20000 commits";
        check_errors(LOG_SYNOPSIS, &[(&["log", "--follow", "-n", "20001", "x/y", "a"], limit)]);
        let Invocation::Command(Command::Log(args)) = log("x/y", Some("a"), false) else {
            panic!("not a log")
        };
        let at_the_limit = log::Args { follow: true, count: Some(20_000), ..args };
        check_parses(vec![(
            vec!["log", "--follow", "-n", "20000", "x/y", "a"],
            Invocation::Command(Command::Log(at_the_limit)),
        )]);
    }

    #[test]
    fn usage_errors_in_the_count_and_skip_of_log() {
        let count = |value: &str| format!("-n takes a whole number of at least 1: {value}");
        let skip = |value: &str| format!("--skip takes a whole number: {value}");
        check_errors(
            LOG_SYNOPSIS,
            &[
                (&["log", "-n", "0", "x/y"], &count("0")),
                (&["log", "-n", "-1", "x/y"], &count("-1")),
                (&["log", "-n", "abc", "x/y"], &count("abc")),
                (&["log", "-n", "1.5", "x/y"], &count("1.5")),
                (&["log", "-n", "4294967296", "x/y"], &count("4294967296")),
                (&["log", "-n", "1", "-n", "2", "x/y"], "-n given twice"),
                (&["log", "--skip", "-1", "x/y"], &skip("-1")),
                (&["log", "--skip", "abc", "x/y"], &skip("abc")),
                (&["log", "--skip", "1", "--skip", "2", "x/y"], "--skip given twice"),
                (&["log", "x/y", "-n"], "missing argument for option '-n'"),
                (&["log", "x/y", "--skip"], "missing argument for option '--skip'"),
                (&["log", "--follow", "--skip", "1", "x/y", "a"], "--follow takes no --skip"),
            ],
        );
    }

    #[test]
    fn a_count_and_a_skip_belong_to_log() {
        check_errors(SHOW_SYNOPSIS, &[(&["show", "-n", "1", "x/y:a"], "show takes no -n")]);
        check_errors(LS_SYNOPSIS, &[(&["ls", "--skip", "1", "x/y"], "ls takes no --skip")]);
        check_errors(REFS_SYNOPSIS, &[(&["refs", "x/y", "-n", "1"], "refs takes no -n")]);
        check_errors(
            DIFF_SYNOPSIS,
            &[(&["diff", "--skip", "1", "x/y@v1"], "diff takes no --skip")],
        );
        check_errors(
            SEARCH_SYNOPSIS,
            &[(&["search", "--skip", "1", "foo"], "search takes no --skip")],
        );
    }

    #[test]
    fn usage_errors_in_the_operands_of_diff() {
        let needs_revision = "diff needs a revision or a range, like REPO@REV or \
                          REPO@A...B";
        let two_dots = "diff takes A...B (Sourcegraph compares from the merge base)";
        check_errors(
            DIFF_SYNOPSIS,
            &[
                (&["diff"], "diff needs an address, like REPO@A...B or REPO@REV"),
                (&["diff", "x/y"], needs_revision),
                (&["diff", "x/y:src"], needs_revision),
                (&["diff", "x/y@a..b"], two_dots),
                (&["diff", "x/y@a..b:src", "lib"], two_dots),
                (&["diff", "x/y@v1", ""], "diff takes no empty path"),
                (&["diff", "x/y@v1", "src", ""], "diff takes no empty path"),
                (&["diff", "x/y@a....b"], "invalid range a....b; a range is A..B or A...B"),
            ],
        );
    }

    #[test]
    fn recursive_belongs_to_ls() {
        let takes_none = |command: &str| format!("{command} takes no --recursive");
        check_errors(
            SHOW_SYNOPSIS,
            &[
                (&["show", "x/y:a", "--recursive"], &takes_none("show")),
                (&["--recursive", "show", "x/y:a"], &takes_none("show")),
            ],
        );
        check_errors(
            SEARCH_SYNOPSIS,
            &[
                (&["search", "--recursive", "foo"], &takes_none("search")),
                (&["search", "foo", "--recursive"], &takes_none("search")),
            ],
        );
        check_errors(
            REFS_SYNOPSIS,
            &[
                (&["refs", "x/y", "--recursive"], &takes_none("refs")),
                (&["refs", "--json", "--json", "x/y"], "--json given twice"),
            ],
        );
        check_errors(LOG_SYNOPSIS, &[(&["log", "--recursive", "x/y"], &takes_none("log"))]);
        check_errors(DIFF_SYNOPSIS, &[(&["diff", "x/y@v1", "--recursive"], &takes_none("diff"))]);
        check_errors(
            LS_SYNOPSIS,
            &[
                (&["ls", "--recursive", "x/y", "--recursive"], "--recursive given twice"),
                (&["ls", "--recursive=1", "x/y"], "--recursive takes no value"),
                (&["ls", "-r", "x/y"], "unknown option -r"),
            ],
        );
        check_errors(SYNOPSIS, &[(&["--recursive", "--recursive"], "--recursive given twice")]);
        assert_eq!(
            reported(&os(&["--recursive"])),
            expected("no command given", &[COMMAND_LIST], SYNOPSIS)
        );
        assert_eq!(
            reported(&os(&["--recursive", "shwo"])),
            expected("unknown command 'shwo'", &[COMMAND_LIST], SYNOPSIS)
        );
    }

    #[test]
    fn usage_errors_in_the_options_and_operands_of_search() {
        let needs_query = "search needs a query";
        check_errors(
            SEARCH_SYNOPSIS,
            &[
                (&["search"], needs_query),
                (&["search", "  "], needs_query),
                (&["search", "--json"], needs_query),
                (&["search", "--"], needs_query),
                (&["search", "--json", "--json", "foo"], "--json given twice"),
                (&["search", "--json=1", "foo"], "--json takes no value"),
            ],
        );
    }

    #[test]
    fn non_utf8_argument_is_named_by_position_with_its_command_synopsis() {
        let bad = || OsString::from_vec(vec![b'x', 0xff]);
        let with = |words: &[&str], at: usize| {
            let mut args = os(words);
            args.insert(at, bad());
            args
        };
        let table = [
            (with(&["show", "--json"], 2), 3, SHOW_SYNOPSIS),
            (with(&["--json", "show"], 2), 3, SHOW_SYNOPSIS),
            (with(&["show"], 1), 2, SHOW_SYNOPSIS),
            (with(&["ls"], 1), 2, LS_SYNOPSIS),
            (with(&["refs", "--json"], 2), 3, REFS_SYNOPSIS),
            (with(&["--recursive", "ls"], 2), 3, LS_SYNOPSIS),
            (with(&["search", "foo"], 2), 3, SEARCH_SYNOPSIS),
            (with(&["--json"], 1), 2, SYNOPSIS),
            (with(&["show"], 0), 1, SYNOPSIS),
            (with(&["ls"], 0), 1, SYNOPSIS),
            (with(&[], 0), 1, SYNOPSIS),
        ];
        for (args, position, synopsis) in table {
            assert_eq!(
                reported(&args),
                expected(&format!("argument {position} is not valid UTF-8"), &[], synopsis),
                "{args:?}"
            );
        }
    }
}
