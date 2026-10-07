use std::ffi::OsString;
use std::fmt::Write as _;
use std::os::unix::ffi::OsStringExt;

use sego::internal::address;
use sego::internal::cli::{
    Command, LS_SYNOPSIS, REFS_SYNOPSIS, SEARCH_SYNOPSIS, SHOW_SYNOPSIS, help,
    parse,
};
use sego::internal::error::{Kind, report};

const GENERAL: &str = "sego COMMAND [--json] ...";
const COMMANDS: &str = "  commands: search, show, ls, refs, log, diff";

fn os(args: &[&str]) -> Vec<OsString> {
    args.iter().map(OsString::from).collect()
}

fn address(token: &str) -> address::Address {
    address::parse(token).unwrap_or_else(|e| panic!("{token}: {e}"))
}

fn ls(token: &str, recursive: bool, json: bool) -> Command {
    Command::Ls { address: address(token), recursive, json }
}

fn refs(token: &str, text: Option<&str>, json: bool) -> Command {
    Command::Refs {
        repo: address(token).repo,
        text: text.map(str::to_owned),
        json,
    }
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
fn ls_and_refs_parse() {
    let tree = "https://github.com/x/y/tree/main/src";
    let table: Vec<(Vec<&str>, Command)> = vec![
        (vec!["ls", "x/y"], ls("x/y", false, false)),
        (vec!["ls", "x/y:src"], ls("x/y:src", false, false)),
        (vec!["ls", "x/y@v1:src"], ls("x/y@v1:src", false, false)),
        (vec!["ls", "x/y:"], ls("x/y:", false, false)),
        (vec!["ls", "x/y:."], ls("x/y:.", false, false)),
        (vec!["ls", tree], ls(tree, false, false)),
        (vec!["ls", "--json", "x/y"], ls("x/y", false, true)),
        (vec!["ls", "x/y", "--recursive"], ls("x/y", true, false)),
        (vec!["--recursive", "ls", "x/y"], ls("x/y", true, false)),
        (
            vec!["ls", "--recursive", "--json", "x/y:src"],
            ls("x/y:src", true, true),
        ),
        (vec!["ls", "--", "x/y"], ls("x/y", false, false)),
        (vec!["refs", "x/y"], refs("x/y", None, false)),
        (vec!["refs", "--json", "x/y"], refs("x/y", None, true)),
        (vec!["refs", "x/y", "v1"], refs("x/y", Some("v1"), false)),
        (
            vec!["refs", "https://github.com/x/y"],
            refs("https://github.com/x/y", None, false),
        ),
        (vec!["refs", "x/y", "a b "], refs("x/y", Some("a b "), false)),
        (vec!["ls", "--help"], Command::Help(Some("ls"))),
        (vec!["-h", "ls"], Command::Help(Some("ls"))),
        (vec!["refs", "--help"], Command::Help(Some("refs"))),
        (vec!["refs", "x/y", "-h"], Command::Help(Some("refs"))),
        (vec!["ls", "--version"], Command::Version),
        (vec!["ls", "--recursive", "--help"], Command::Help(Some("ls"))),
    ];
    for (args, command) in table {
        let parsed =
            parse(&os(&args)).unwrap_or_else(|e| panic!("{args:?}: {e}"));
        assert_eq!(parsed, command, "{args:?}");
    }
}

#[test]
fn ls_operand_errors() {
    let plain = |message: &str| expected(message, &[], LS_SYNOPSIS);
    check(vec![
        (vec!["ls"], plain("ls needs an address, like REPO[@REV][:PATH]")),
        (
            vec!["ls", "--json", "--recursive"],
            plain("ls needs an address, like REPO[@REV][:PATH]"),
        ),
        (vec!["ls", "x/y", "x/z"], plain("ls takes one address")),
        (vec!["ls", "x/y", "src", "a"], plain("ls takes one address")),
        (
            vec!["ls", "x/y@a..b"],
            plain(
                "a range is only valid in log (A..B) and diff (A...B): a..b",
            ),
        ),
        (
            vec!["ls", "x/y@a...b:src"],
            plain(
                "a range is only valid in log (A..B) and diff (A...B): a...b",
            ),
        ),
        (
            vec!["ls", "https://github.com/x/y/blob/main/a.go#L3"],
            plain("ls takes no LINES"),
        ),
        (
            vec!["ls", "x/y@:src"],
            plain("the address has an empty revision: x/y@:src"),
        ),
    ]);
}

#[test]
fn refs_operand_errors() {
    let plain = |message: &str| expected(message, &[], REFS_SYNOPSIS);
    check(vec![
        (vec!["refs"], plain("refs needs a repository, like REPO")),
        (vec!["refs", "--json"], plain("refs needs a repository, like REPO")),
        (
            vec!["refs", "x/y", "a", "b"],
            plain("refs takes a repository and at most one TEXT"),
        ),
        (
            vec!["refs", "x/y@v1"],
            plain("refs names a repository, not a revision or path: x/y@v1"),
        ),
        (
            vec!["refs", "github.com/x/y@v1"],
            plain(
                "refs names a repository, not a revision or path: \
                 github.com/x/y@v1",
            ),
        ),
        (
            vec!["refs", "x/y:src"],
            plain("refs names a repository, not a revision or path: x/y:src"),
        ),
        (
            vec!["refs", "x/y@v1:src", "t"],
            plain(
                "refs names a repository, not a revision or path: \
                 x/y@v1:src",
            ),
        ),
        (
            vec!["refs", "https://github.com/x/y/tree/main/src"],
            plain(
                "refs names a repository, not a revision or path: \
                 https://github.com/x/y/tree/main/src",
            ),
        ),
        (vec!["refs", "x/y", ""], plain("the TEXT operand is empty")),
        (vec!["refs", "x/y", "  "], plain("the TEXT operand is empty")),
        (
            vec!["refs", "x/y@:"],
            plain("the address has an empty revision: x/y@:"),
        ),
    ]);
}

#[test]
fn recursive_belongs_to_ls() {
    let takes_none = |command: &str, synopsis: &str| {
        expected(&format!("{command} takes no --recursive"), &[], synopsis)
    };
    check(vec![
        (
            vec!["--recursive"],
            expected("no command given", &[COMMANDS], GENERAL),
        ),
        (
            vec!["--recursive", "shwo"],
            expected("unknown command 'shwo'", &[COMMANDS], GENERAL),
        ),
        (
            vec!["show", "x/y:a", "--recursive"],
            takes_none("show", SHOW_SYNOPSIS),
        ),
        (
            vec!["--recursive", "show", "x/y:a"],
            takes_none("show", SHOW_SYNOPSIS),
        ),
        (
            vec!["search", "--recursive", "foo"],
            takes_none("search", SEARCH_SYNOPSIS),
        ),
        (
            vec!["refs", "x/y", "--recursive"],
            takes_none("refs", REFS_SYNOPSIS),
        ),
        (
            vec!["ls", "--recursive", "x/y", "--recursive"],
            expected("--recursive given twice", &[], LS_SYNOPSIS),
        ),
        (
            vec!["--recursive", "--recursive"],
            expected("--recursive given twice", &[], GENERAL),
        ),
        (
            vec!["ls", "--recursive=1", "x/y"],
            expected("--recursive takes no value", &[], LS_SYNOPSIS),
        ),
        (
            vec!["ls", "-r", "x/y"],
            expected("unknown option -r", &[], LS_SYNOPSIS),
        ),
        (
            vec!["refs", "--json", "--json", "x/y"],
            expected("--json given twice", &[], REFS_SYNOPSIS),
        ),
    ]);
}

#[test]
fn non_utf8_after_ls_or_refs_carries_that_synopsis() {
    let bad = || OsString::from_vec(vec![b'x', 0xff]);
    let with = |words: &[&str], at: usize| -> Vec<OsString> {
        let mut args = os(words);
        args.insert(at, bad());
        args
    };
    let table = [
        (with(&["ls"], 1), 2, LS_SYNOPSIS),
        (with(&["refs", "--json"], 2), 3, REFS_SYNOPSIS),
        (with(&["--recursive", "ls"], 2), 3, LS_SYNOPSIS),
        (with(&["ls"], 0), 1, GENERAL),
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

#[test]
fn help_texts_are_exact() {
    assert_eq!(help(Some("ls")), LS_HELP);
    assert_eq!(help(Some("refs")), REFS_HELP);
}
