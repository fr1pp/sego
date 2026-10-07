use std::ffi::OsString;
use std::fmt::Write as _;
use std::os::unix::ffi::OsStringExt;

use sego::internal::cli::{Command, SEARCH_SYNOPSIS, help, parse};
use sego::internal::error::{Kind, report};

const GENERAL: &str = "sego COMMAND [--json] ...";

fn os(args: &[&str]) -> Vec<OsString> {
    args.iter().map(OsString::from).collect()
}

fn search(query: &str, json: bool) -> Command {
    Command::Search { query: query.to_owned(), json }
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
    let _ = writeln!(text, "usage: {synopsis}");
    text
}

#[test]
fn queries_parse() {
    let table: Vec<(Vec<&str>, Command)> = vec![
        (
            vec!["search", "repo:x", "foo", "bar", "--json"],
            search("repo:x foo bar", true),
        ),
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
        (vec!["search", "foo", "-h"], Command::Help(Some("search"))),
        (vec!["search", "--help"], Command::Help(Some("search"))),
        (vec!["-h", "search"], Command::Help(Some("search"))),
        (vec!["search", "--version"], Command::Version),
    ];
    for (args, command) in table {
        let parsed =
            parse(&os(&args)).unwrap_or_else(|e| panic!("{args:?}: {e}"));
        assert_eq!(parsed, command, "{args:?}");
    }
}

#[test]
fn search_errors_carry_the_search_synopsis() {
    let plain = |message: &str| expected(message, &[], SEARCH_SYNOPSIS);
    let table: Vec<(Vec<&str>, String)> = vec![
        (vec!["search"], plain("search needs a query")),
        (vec!["search", "  "], plain("search needs a query")),
        (vec!["search", "--json"], plain("search needs a query")),
        (vec!["search", "--"], plain("search needs a query")),
        (
            vec!["search", "--limit", "5", "foo"],
            expected(
                "unknown option --limit",
                &["  sego has no --limit; count: is a query filter"],
                SEARCH_SYNOPSIS,
            ),
        ),
        (
            vec!["search", "--json", "--json", "foo"],
            plain("--json given twice"),
        ),
        (vec!["search", "--json=1", "foo"], plain("--json takes no value")),
        (
            vec!["search", "--recursive", "foo"],
            plain("search takes no --recursive"),
        ),
    ];
    for (args, text) in table {
        assert_eq!(reported(&os(&args)), text, "{args:?}");
    }
}

#[test]
fn single_dash_is_an_option_before_the_command() {
    assert_eq!(
        reported(&os(&["-x", "search", "foo"])),
        expected("unknown option -x", &[], GENERAL)
    );
}

#[test]
fn non_utf8_after_search_names_the_position() {
    let mut args = os(&["search", "foo"]);
    args.push(OsString::from_vec(vec![b'x', 0xff]));
    assert_eq!(
        reported(&args),
        expected("argument 3 is not valid UTF-8", &[], SEARCH_SYNOPSIS)
    );
}

const SEARCH_HELP: &str = "\
usage: sego search [--json] QUERY...

QUERY... is joined with single spaces and sent as one Sourcegraph query,
with count:100 appended when the query has no count:.

query:
  -term          literal text, not an option (-h is still help)
  and, or, not   operators, in any case
  count:, repo:, lang:, file:
                 query filters
  -- TEXT        text that is one of sego's options, such as --json

options:
  --json       one JSON document on stdout
  -h, --help   this help, or a command's help after its name
  --version    sego's version

Search prints one line of context around each match.
Text prints the first 100 matches; --json carries every result fetched.
";

#[test]
fn help_texts_are_exact() {
    assert_eq!(help(Some("search")), SEARCH_HELP);
    assert!(help(None).contains(
        "commands:\n  search QUERY...                code matching a \
         Sourcegraph query\n  show REPO[@REV]:PATH [LINES]   lines of a \
         file\n  ls REPO[@REV][:PATH]           entries of a directory\n  \
         refs REPO [TEXT]               branches and tags\n"
    ));
}

#[test]
fn search_help_fits_79_columns() {
    for line in help(Some("search")).lines() {
        assert!(line.chars().count() <= 79, "{line}");
    }
}
