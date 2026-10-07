use std::ffi::OsString;
use std::fmt::Write as _;

use sego::internal::address::{self, Dots, Rev};
use sego::internal::cli::{
    Command, DIFF_SYNOPSIS, LOG_SYNOPSIS, SHOW_SYNOPSIS, help, parse,
};
use sego::internal::error::{Kind, report};

const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

fn os(args: &[&str]) -> Vec<OsString> {
    args.iter().map(OsString::from).collect()
}

fn address(token: &str) -> address::Address {
    address::parse(token).unwrap_or_else(|e| panic!("{token}: {e}"))
}

fn commit(token: &str, json: bool) -> Command {
    Command::ShowCommit { address: address(token), json }
}

fn log(token: &str, path: Option<&str>, json: bool) -> Command {
    Command::Log {
        address: address(token),
        path: path.map(str::to_owned),
        json,
    }
}

fn diff(token: &str, paths: &[&str], json: bool) -> Command {
    Command::Diff {
        address: address(token),
        paths: paths.iter().map(|p| (*p).to_owned()).collect(),
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

fn expected(message: &str, synopsis: &str) -> String {
    let mut text = format!("sego: error: {message}\n");
    for (i, line) in synopsis.lines().enumerate() {
        let lead = if i == 0 { "usage: " } else { "       " };
        let _ = writeln!(text, "{lead}{line}");
    }
    text
}

fn check_parses(table: Vec<(Vec<&str>, Command)>) {
    for (args, command) in table {
        let parsed =
            parse(&os(&args)).unwrap_or_else(|e| panic!("{args:?}: {e}"));
        assert_eq!(parsed, command, "{args:?}");
    }
}

fn check_errors(synopsis: &str, table: Vec<(Vec<&str>, &str)>) {
    for (args, message) in table {
        assert_eq!(
            reported(&os(&args)),
            expected(message, synopsis),
            "{args:?}"
        );
    }
}

#[test]
fn show_of_a_revision_is_a_commit() {
    let url = format!("https://github.com/o/r/commit/{SHA}");
    let sg = format!("sourcegraph.com/github.com/o/r@v1/-/commit/{SHA}");
    check_parses(vec![
        (vec!["show", "x/y@v1"], commit("x/y@v1", false)),
        (vec!["show", "--json", "x/y@v1"], commit("x/y@v1", true)),
        (vec!["show", "x/y@v1", "--json"], commit("x/y@v1", true)),
        (
            vec!["show", "github.com/x/y@HEAD~2"],
            commit("github.com/x/y@HEAD~2", false),
        ),
        (vec!["show", &url], commit(&url, false)),
        (vec!["show", &sg], commit(&sg, false)),
    ]);
}

#[test]
fn show_of_a_commit_is_checked() {
    let needs_either = "show needs a file's path or a revision, like \
                        REPO[@REV]:PATH or REPO@REV";
    let needs_path = "show needs a file's path, like REPO[@REV]:PATH";
    let range = |r: &str| {
        format!("a range is only valid in log (A..B) and diff (A...B): {r}")
    };
    let url = format!("https://github.com/o/r/commit/{SHA}#L3");
    let tree_dot = format!("https://github.com/o/r/tree/{SHA}/.");
    let blob_dot = format!("https://github.com/o/r/blob/{SHA}/.");
    let sg_dot = "sourcegraph.com/github.com/o/r@v1/-/tree/.";
    check_errors(
        SHOW_SYNOPSIS,
        vec![
            (vec!["show", "x/y"], needs_either),
            (vec!["show", "github.com/x/y"], needs_either),
            (vec!["show", "x/y:"], needs_path),
            (vec!["show", "x/y@v1:"], needs_path),
            (vec!["show", "x/y@v1:."], needs_path),
            (vec!["show", "x/y:."], needs_path),
            (vec!["show", &tree_dot], needs_path),
            (vec!["show", &blob_dot], needs_path),
            (vec!["show", sg_dot], needs_path),
            (vec!["show", "x/y@v1", "3"], "a commit takes no LINES"),
            (vec!["show", "x/y@v1", "abc"], "a commit takes no LINES"),
            (vec!["show", &url], "a commit takes no LINES"),
            (vec!["show", "x/y@a..b"], &range("a..b")),
            (vec!["show", "x/y@a...b"], &range("a...b")),
            (
                vec!["show", "x/y@v1", "1", "2"],
                "show takes an address and at most one LINES operand",
            ),
        ],
    );
}

#[test]
fn log_parses() {
    let blob = format!("https://github.com/o/r/blob/{SHA}/x.go");
    let tree = format!("https://github.com/o/r/tree/{SHA}/src");
    let root = format!("https://github.com/o/r/tree/{SHA}");
    let url = format!("https://github.com/o/r/commit/{SHA}");
    let compare2 = "https://github.com/o/r/compare/a..b";
    let compare3 = "https://github.com/o/r/compare/a...b";
    check_parses(vec![
        (vec!["log", "x/y"], log("x/y", None, false)),
        (vec!["log", "--json", "x/y"], log("x/y", None, true)),
        (vec!["log", "x/y", "--json"], log("x/y", None, true)),
        (vec!["log", "x/y@v1"], log("x/y@v1", None, false)),
        (vec!["log", "x/y@a..b"], log("x/y@a..b", None, false)),
        (vec!["log", "x/y:src"], log("x/y:src", Some("src"), false)),
        (vec!["log", "x/y@v1:src/"], log("x/y@v1:src/", Some("src"), false)),
        (vec!["log", "x/y", "src"], log("x/y", Some("src"), false)),
        (
            vec!["log", "x/y", "/src/a.go/"],
            log("x/y", Some("src/a.go"), false),
        ),
        (vec!["log", "x/y@a..b:s"], log("x/y@a..b:s", Some("s"), false)),
        (vec!["log", "x/y@a..b", "s"], log("x/y@a..b", Some("s"), false)),
        (vec!["log", "x/y@v1:."], log("x/y@v1:.", None, false)),
        (vec!["log", "x/y@v1:"], log("x/y@v1:", None, false)),
        (vec!["log", "x/y@v1", "."], log("x/y@v1", None, false)),
        (vec!["log", "x/y@v1", "/"], log("x/y@v1", None, false)),
        (vec!["log", "x/y:.", "src"], log("x/y:.", Some("src"), false)),
        (vec!["log", "x/y:src", "."], log("x/y:src", Some("src"), false)),
        (vec!["log", &blob], log(&blob, Some("x.go"), false)),
        (vec!["log", &tree], log(&tree, Some("src"), false)),
        (vec!["log", &root], log(&root, None, false)),
        (vec!["log", &url], log(&url, None, false)),
        (vec!["log", compare2], log(compare2, None, false)),
        (vec!["log", compare3], log(compare2, None, false)),
        (vec!["log", "--", "x/y"], log("x/y", None, false)),
    ]);
}

#[test]
fn log_reads_a_three_dot_compare_url_as_two_dots() {
    let compare3 = "https://github.com/o/r/compare/a...b";
    let Ok(Command::Log { address, .. }) = parse(&os(&["log", compare3]))
    else {
        panic!("log {compare3} did not parse as a log");
    };
    assert_eq!(
        address.rev,
        Rev::Range { base: "a".into(), head: "b".into(), dots: Dots::Two }
    );
}

#[test]
fn log_errors() {
    let blob_main = "https://github.com/o/r/blob/main/x.go";
    let tree_main = "https://github.com/o/r/tree/main/src";
    let split = "log can't split a branch or tag from the path in a tree \
                 or blob URL";
    let lines = format!("https://github.com/o/r/blob/{SHA}/x.go#L3");
    check_errors(
        LOG_SYNOPSIS,
        vec![
            (vec!["log"], "log needs an address, like REPO[@REV][:PATH]"),
            (
                vec!["log", "--json"],
                "log needs an address, like REPO[@REV][:PATH]",
            ),
            (
                vec!["log", "x/y", "a", "b"],
                "log takes an address and at most one PATH",
            ),
            (vec!["log", "x/y@a...b"], "log takes A..B, not A...B"),
            (vec!["log", "x/y@a...b:src"], "log takes A..B, not A...B"),
            (vec!["log", "x/y", ""], "log takes no empty path"),
            (vec!["log", "x/y@v1:", ""], "log takes no empty path"),
            (vec!["log", "x/y@a..b", ""], "log takes no empty path"),
            (vec!["log", "x/y:src", ""], "log takes no empty path"),
            (vec!["log", "x/y:src", "lib"], "log takes one path"),
            (vec!["log", "x/y@a..b:src", "lib"], "log takes one path"),
            (vec!["log", blob_main], split),
            (vec!["log", tree_main], split),
            (vec!["log", &lines], "log takes no LINES"),
            (
                vec!["log", "x/y@:src"],
                "the address has an empty revision: x/y@:src",
            ),
        ],
    );
}

#[test]
fn diff_parses() {
    let blob = format!("https://github.com/o/r/blob/{SHA}/x.go");
    let url = format!("https://github.com/o/r/commit/{SHA}");
    let compare = "https://github.com/o/r/compare/a...b";
    check_parses(vec![
        (vec!["diff", "x/y@v1"], diff("x/y@v1", &[], false)),
        (vec!["diff", "--json", "x/y@v1"], diff("x/y@v1", &[], true)),
        (vec!["diff", "x/y@a...b"], diff("x/y@a...b", &[], false)),
        (
            vec!["diff", "x/y@a...b:src"],
            diff("x/y@a...b:src", &["src"], false),
        ),
        (
            vec!["diff", "x/y@a...b", "src", "lib/"],
            diff("x/y@a...b", &["src", "lib"], false),
        ),
        (
            vec!["diff", "x/y@a...b:src", "/lib"],
            diff("x/y@a...b:src", &["src", "lib"], false),
        ),
        (
            vec!["diff", "x/y@v1", "//a/b//", "c"],
            diff("x/y@v1", &["a/b", "c"], false),
        ),
        (vec!["diff", "x/y@v1...v2", "."], diff("x/y@v1...v2", &[], false)),
        (vec!["diff", "x/y@v1...v2", "/"], diff("x/y@v1...v2", &[], false)),
        (vec!["diff", "x/y@v1...v2:."], diff("x/y@v1...v2:.", &[], false)),
        (
            vec!["diff", "x/y@v1...v2", ".", "src", "/"],
            diff("x/y@v1...v2", &["src"], false),
        ),
        (vec!["diff", "x/y@v1:"], diff("x/y@v1:", &[], false)),
        (vec!["diff", &blob], diff(&blob, &["x.go"], false)),
        (vec!["diff", &url], diff(&url, &[], false)),
        (vec!["diff", compare, "x.go"], diff(compare, &["x.go"], false)),
    ]);
}

#[test]
fn diff_errors() {
    let blob_main = "https://github.com/o/r/blob/main/x.go";
    let split = "diff can't split a branch or tag from the path in a tree \
                 or blob URL";
    let lines = format!("https://github.com/o/r/blob/{SHA}/x.go#L3");
    let needs_rev = "diff needs a revision or a range, like REPO@REV or \
                     REPO@A...B";
    let two_dots =
        "diff takes A...B (Sourcegraph compares from the merge base)";
    check_errors(
        DIFF_SYNOPSIS,
        vec![
            (
                vec!["diff"],
                "diff needs an address, like REPO@A...B or REPO@REV",
            ),
            (vec!["diff", "x/y"], needs_rev),
            (vec!["diff", "x/y:src"], needs_rev),
            (vec!["diff", "x/y@a..b"], two_dots),
            (vec!["diff", "x/y@a..b:src", "lib"], two_dots),
            (vec!["diff", "x/y@v1", ""], "diff takes no empty path"),
            (vec!["diff", "x/y@v1", "src", ""], "diff takes no empty path"),
            (vec!["diff", blob_main], split),
            (vec!["diff", &lines], "diff takes no LINES"),
            (
                vec!["diff", "x/y@a....b"],
                "invalid range a....b; a range is A..B or A...B",
            ),
        ],
    );
}

#[test]
fn log_and_diff_take_no_recursive() {
    check_errors(
        LOG_SYNOPSIS,
        vec![(vec!["log", "--recursive", "x/y"], "log takes no --recursive")],
    );
    check_errors(
        DIFF_SYNOPSIS,
        vec![(
            vec!["diff", "x/y@v1", "--recursive"],
            "diff takes no --recursive",
        )],
    );
}

#[test]
fn help_topics_parse() {
    check_parses(vec![
        (vec!["log", "--help"], Command::Help(Some("log"))),
        (vec!["-h", "diff"], Command::Help(Some("diff"))),
        (vec!["diff", "x/y@v1", "-h"], Command::Help(Some("diff"))),
    ]);
}

const LOG_HELP: &str = "\
usage: sego log [--json] REPO[@REV][:PATH] [PATH]
       sego log [--json] REPO@A..B[:PATH] [PATH]

operands:
  REPO       host/owner/name, owner/name, or a GitHub, GitLab or
             Sourcegraph URL
  REPO@REV   commits reachable from REV, newest first; the default
             branch without @REV
  REPO@A..B  commits reachable from B and not from A, as git log A..B
  PATH       only commits that changed PATH (one path)

options:
  --json       one JSON document on stdout
  -h, --help   this help, or a command's help after its name
  --version    sego's version

Text and JSON list the newest 150 commits of a REV; a range lists them all
in JSON. Tags that point at a listed commit are marked.
";

const DIFF_HELP: &str = "\
usage: sego diff [--json] REPO@A...B[:PATH] [PATH...]
       sego diff [--json] REPO@REV[:PATH] [PATH...]

operands:
  REPO        host/owner/name, owner/name, or a GitHub, GitLab or
              Sourcegraph URL
  REPO@A...B  changes from the merge base of A and B to B, as git
              diff A...B
  REPO@REV    the change REV made, against its first parent
  PATH...     paths or directory prefixes; the patch comes only for these

options:
  --json       one JSON document on stdout
  -h, --help   this help, or a command's help after its name
  --version    sego's version

Text prints 200 file rows; --json carries every file.
With PATH operands, a rename shows as R only when both its paths are named.
";

const SHOW_HELP: &str = "\
usage: sego show [--json] REPO[@REV]:PATH [LINES]
       sego show [--json] REPO@REV

addresses:
  REPO     host/owner/name, owner/name, or a GitHub, GitLab or
           Sourcegraph URL
  @REV     a branch, tag or commit; the default branch without it
  @REV     with no :PATH, the commit: its message and changed files
  :PATH    a path from the repository root
  LINES    N, N-M, N,M or N- (lines are numbered from 1)

options:
  --json       one JSON document on stdout
  -h, --help   this help, or a command's help after its name
  --version    sego's version

A file over 1.5 MiB is read through Sourcegraph's raw route, a second request.
Text shows lines 1-500 when LINES is not given; --json carries the same lines.
";

#[test]
fn history_help_texts_are_exact() {
    assert_eq!(help(Some("log")), LOG_HELP);
    assert_eq!(help(Some("diff")), DIFF_HELP);
    assert_eq!(help(Some("show")), SHOW_HELP);
}
