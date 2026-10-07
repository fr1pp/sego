use std::ffi::OsString;
use std::fmt::Write as _;
use std::os::unix::ffi::OsStringExt;

use sego::internal::address::{self, End, LineRange};
use sego::internal::cli::{
    COMMANDS as TABLE, Command, SHOW_SYNOPSIS, help, parse,
};
use sego::internal::error::{Kind, report, usage_block};

const GENERAL: &str = "sego COMMAND [--json] ...";
const COMMANDS: &str = "  commands: search, show, ls, refs, log, diff";
const LIMIT: &str = "sego has no --limit; count: is a query filter";

fn os(args: &[&str]) -> Vec<OsString> {
    args.iter().map(OsString::from).collect()
}

fn address(token: &str) -> address::Address {
    address::parse(token).unwrap_or_else(|e| panic!("{token}: {e}"))
}

fn show(token: &str, lines: Option<LineRange>, json: bool) -> Command {
    Command::Show { address: address(token), lines, json }
}

fn lines(start: u32, end: u32) -> LineRange {
    LineRange { start, end: End::Line(end) }
}

fn reported(args: &[OsString]) -> String {
    let error = match parse(args) {
        Ok(command) => panic!("{args:?} parsed as {command:?}"),
        Err(error) => error,
    };
    assert_eq!(error.kind(), Kind::Usage, "{args:?}");
    assert_eq!(error.exit_code(), 2);
    let mut out = Vec::new();
    report(&error, &mut out).unwrap_or_else(|e| panic!("{e}"));
    String::from_utf8(out).unwrap_or_else(|e| panic!("{e}"))
}

fn expected(message: &str, facts: &[&str], synopsis: &str) -> String {
    let mut text = format!("sego: error: {message}\n");
    for fact in facts {
        let _ = writeln!(text, "{fact}");
    }
    for (i, line) in synopsis.lines().enumerate() {
        let lead = if i == 0 { "usage: " } else { "       " };
        let _ = writeln!(text, "{lead}{line}");
    }
    text
}

fn check(table: Vec<(Vec<&str>, String)>) {
    for (args, text) in table {
        assert_eq!(reported(&os(&args)), text, "{args:?}");
    }
}

#[test]
fn commands_parse() {
    let blob = "https://github.com/x/y/blob/main/a.go#L3-L9";
    let table: Vec<(Vec<&str>, Command)> = vec![
        (
            vec!["show", "github.com/x/y:a.go"],
            show("github.com/x/y:a.go", None, false),
        ),
        (vec!["show", "--json", "x/y:a.go"], show("x/y:a.go", None, true)),
        (vec!["show", "x/y:a.go", "--json"], show("x/y:a.go", None, true)),
        (vec!["--json", "show", "x/y:a.go"], show("x/y:a.go", None, true)),
        (
            vec!["show", "x/y@v1:a.go", "3-9"],
            show("x/y@v1:a.go", Some(lines(3, 9)), false),
        ),
        (
            vec!["show", "x/y:a.go", "4-"],
            Command::Show {
                address: address("x/y:a.go"),
                lines: Some(LineRange { start: 4, end: End::Open }),
                json: false,
            },
        ),
        (
            vec!["show", "x/y:a.go", "3,9"],
            show("x/y:a.go", Some(lines(3, 9)), false),
        ),
        (vec!["show", "--", "x/y:a.go"], show("x/y:a.go", None, false)),
        (
            vec!["show", "https://github.com/x/y/tree/main/src"],
            show("https://github.com/x/y/tree/main/src", None, false),
        ),
        (vec!["show", blob], show(blob, Some(lines(3, 9)), false)),
        (vec!["show", blob, "3-9"], show(blob, Some(lines(3, 9)), false)),
        (vec!["--help"], Command::Help(None)),
        (vec!["-h"], Command::Help(None)),
        (vec!["show", "--help"], Command::Help(Some("show"))),
        (vec!["-h", "show"], Command::Help(Some("show"))),
        (vec!["show", "-h", "--json"], Command::Help(Some("show"))),
        (vec!["--version"], Command::Version),
        (vec!["show", "--version"], Command::Version),
    ];
    for (args, command) in table {
        let parsed =
            parse(&os(&args)).unwrap_or_else(|e| panic!("{args:?}: {e}"));
        assert_eq!(parsed, command, "{args:?}");
    }
}

#[test]
fn errors_before_a_command() {
    check(vec![
        (vec![], expected("no command given", &[COMMANDS], GENERAL)),
        (vec!["--json"], expected("no command given", &[COMMANDS], GENERAL)),
        (
            vec!["shwo"],
            expected("unknown command 'shwo'", &[COMMANDS], GENERAL),
        ),
        (
            vec!["list", "x/y"],
            expected("unknown command 'list'", &[COMMANDS], GENERAL),
        ),
        (
            vec!["list", "x/y:a.go", "3-9"],
            expected("unknown command 'list'", &[COMMANDS], GENERAL),
        ),
        (
            vec!["find", "x"],
            expected("unknown command 'find'", &[COMMANDS], GENERAL),
        ),
        (
            vec!["-C", "search", "x"],
            expected("unknown option -C", &[], GENERAL),
        ),
        (
            vec!["--json", "--json"],
            expected("--json given twice", &[], GENERAL),
        ),
        (vec!["--json=1"], expected("--json takes no value", &[], GENERAL)),
        (
            vec!["--limit", "5", "show"],
            expected(
                "unknown option --limit",
                &[&format!("  {LIMIT}")],
                GENERAL,
            ),
        ),
    ]);
}

#[test]
fn option_errors_in_show() {
    let with_fact = |option: &str, fact: &str| {
        expected(
            &format!("unknown option {option}"),
            &[&format!("  {fact}")],
            SHOW_SYNOPSIS,
        )
    };
    let plain = |message: &str| expected(message, &[], SHOW_SYNOPSIS);
    check(vec![
        (
            vec!["show", "--json", "--json", "x/y:a"],
            plain("--json given twice"),
        ),
        (
            vec!["show", "--json", "x/y:a", "--json"],
            plain("--json given twice"),
        ),
        (vec!["show", "--json=1", "x/y:a"], plain("--json takes no value")),
        (vec!["show", "--help=1"], plain("--help takes no value")),
        (vec!["show", "-x", "x/y:a"], plain("unknown option -x")),
        (vec!["show", "-json", "x/y:a"], plain("unknown option -json")),
        (
            vec!["show", "--recursive", "x/y:a"],
            plain("show takes no --recursive"),
        ),
        (vec!["show", "x/y:a", "--limit"], with_fact("--limit", LIMIT)),
        (vec!["show", "x/y:a", "--limit=5"], with_fact("--limit", LIMIT)),
        (
            vec!["show", "--count", "x/y:a"],
            with_fact(
                "--count",
                "sego has no --count; count: is a query filter",
            ),
        ),
        (
            vec!["show", "--context", "x/y:a"],
            with_fact(
                "--context",
                "sego has no --context; search prints one line of context, \
                 show prints any range",
            ),
        ),
        (
            vec!["show", "--repo", "x/y:a"],
            with_fact("--repo", "sego has no --repo; repo: is a query filter"),
        ),
        (
            vec!["show", "--lang", "x/y:a"],
            with_fact("--lang", "sego has no --lang; lang: is a query filter"),
        ),
        (
            vec!["show", "--file", "x/y:a"],
            with_fact("--file", "sego has no --file; file: is a query filter"),
        ),
    ]);
}

#[test]
fn show_operand_errors() {
    let plain = |message: &str| expected(message, &[], SHOW_SYNOPSIS);
    let needs_address = "show needs an address, like REPO[@REV]:PATH";
    let needs_path = "show needs a file's path, like REPO[@REV]:PATH";
    check(vec![
        (vec!["show"], plain(needs_address)),
        (vec!["show", "--json"], plain(needs_address)),
        (
            vec!["show", "x/y:a", "1", "2"],
            plain("show takes an address and at most one LINES operand"),
        ),
        (
            vec!["show", "github.com/x/y"],
            plain(
                "show needs a file's path or a revision, like \
                 REPO[@REV]:PATH or REPO@REV",
            ),
        ),
        (vec!["show", "x/y:"], plain(needs_path)),
        (vec!["show", "x/y@v1:"], plain(needs_path)),
        (vec!["show", "x/y:."], plain(needs_path)),
        (
            vec!["show", "x/y@a..b:f"],
            plain(
                "a range is only valid in log (A..B) and diff (A...B): a..b",
            ),
        ),
        (
            vec!["show", "x/y@a...b:f"],
            plain(
                "a range is only valid in log (A..B) and diff (A...B): a...b",
            ),
        ),
        (
            vec!["show", "x/y:f", "0"],
            plain("invalid line range 0; lines are numbered from 1"),
        ),
        (
            vec!["show", "x/y:f", "5-2"],
            plain("invalid line range 5-2; the end is before the start"),
        ),
        (
            vec!["show", "x/y@:f"],
            plain("the address has an empty revision: x/y@:f"),
        ),
        (
            vec!["show", "https://github.com/x/y/blob/main/a.go#L3", "5-9"],
            plain("the URL's lines 3 and the LINES operand 5-9 disagree"),
        ),
    ]);
}

#[test]
fn non_utf8_arguments_are_named_by_position() {
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
        (with(&["--json"], 1), 2, GENERAL),
        (with(&["show"], 0), 1, GENERAL),
        (with(&[], 0), 1, GENERAL),
    ];
    for (args, position, synopsis) in table {
        assert_eq!(
            reported(&args),
            expected(
                &format!("argument {position} is not valid UTF-8"),
                &[],
                synopsis
            ),
            "{args:?}"
        );
    }
}

#[test]
fn help_and_version_need_a_known_command() {
    for flag in ["--help", "--version"] {
        assert_eq!(
            reported(&os(&["shwo", flag])),
            expected("unknown command 'shwo'", &[COMMANDS], GENERAL)
        );
    }
}

const HELP: &str = "\
sego reads public source code at any revision through sourcegraph.com.

usage: sego COMMAND [--json] ...

commands:
  search QUERY...                code matching a Sourcegraph query
  show REPO[@REV]:PATH [LINES]   lines of a file
  ls REPO[@REV][:PATH]           entries of a directory
  refs REPO [TEXT]               branches and tags
  log REPO[@REV][:PATH] [PATH]   commits
  diff REPO@A...B [PATH...]      changed files, and the patch of named paths

addresses:
  REPO     host/owner/name, owner/name, or a GitHub, GitLab or
           Sourcegraph URL
  @REV     a branch, tag or commit; the default branch without it
  :PATH    a path from the repository root
  LINES    N, N-M, N,M or N- (lines are numbered from 1)

options:
  --json       one JSON document on stdout
  -h, --help   this help, or a command's help after its name
  --version    sego's version

environment:
  SEGO_ENDPOINT   the Sourcegraph instance (default
                  https://sourcegraph.com)

exit codes: 0 answered, 1 not answered, 2 the command line is wrong
";

#[test]
fn help_is_the_text_in_the_brief() {
    assert_eq!(help(None), HELP);
}

#[test]
fn show_help_has_the_synopsis_blocks_and_two_facts() {
    let text = help(Some("show"));
    assert!(text.starts_with(&format!(
        "{}\n\naddresses:\n",
        usage_block(SHOW_SYNOPSIS)
    )));
    assert!(text.contains("\noptions:\n  --json "));
    assert!(text.ends_with(
        "A file over 1.5 MiB is read through Sourcegraph's raw route, a \
         second request.\nText shows lines 1-500 when LINES is not given; \
         --json carries the same lines.\n"
    ));
    assert!(!text.contains("commands:"));
}

#[test]
fn every_command_has_its_own_help() {
    let general = help(None);
    for entry in TABLE {
        let text = help(Some(entry.name));
        assert!(
            text.starts_with(&format!("{}\n", usage_block(entry.synopsis))),
            "{}: {text}",
            entry.name
        );
        assert_ne!(text, general, "{}", entry.name);
        assert!(!text.contains("commands:"), "{}", entry.name);
    }
}

#[test]
fn help_fits_79_columns() {
    let topics = TABLE.iter().map(|entry| Some(entry.name));
    for topic in std::iter::once(None).chain(topics) {
        for line in help(topic).lines() {
            assert!(line.chars().count() <= 79, "{topic:?}: {line}");
        }
    }
}

#[test]
fn general_help_names_every_command() {
    let text = help(None);
    let block = text
        .split("commands:\n")
        .nth(1)
        .and_then(|rest| rest.split("\n\n").next())
        .unwrap_or_default();
    let named: Vec<&str> = block
        .lines()
        .filter_map(|line| line.trim_start().split(' ').next())
        .collect();
    let table: Vec<&str> = TABLE.iter().map(|entry| entry.name).collect();
    assert_eq!(named, table);
}
