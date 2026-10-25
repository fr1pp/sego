use std::ffi::OsString;
use std::fmt::Write as _;
use std::os::unix::ffi::OsStringExt;

use super::{
    COMMANDS, Command, DIFF_SYNOPSIS, LOG_SYNOPSIS, LS_SYNOPSIS,
    REFS_SYNOPSIS, SEARCH_SYNOPSIS, SHOW_SYNOPSIS, SYNOPSIS, help, parse,
};
use crate::address::{self, End, LineRange};
use crate::error::{Kind, report, usage_block};

const SHA: &str = "0123456789abcdef0123456789abcdef01234567";
const COMMAND_LIST: &str = "commands: search, show, ls, refs, log, diff";
const LIMIT: &str = "sego has no --limit; count: is a query filter";
const NEEDS_SHOW_ADDRESS: &str = "show needs an address, like REPO[@REV]:PATH";
const NEEDS_FILE_PATH: &str = "show needs a file's path, like REPO[@REV]:PATH";
const RANGE_ONLY_IN_HISTORY: &str =
    "a range is only valid in log (A..B) and diff (A...B)";

fn os(args: &[&str]) -> Vec<OsString> {
    args.iter().map(OsString::from).collect()
}

fn address(token: &str) -> address::Address {
    address::parse(token).unwrap_or_else(|e| panic!("{token}: {e}"))
}

fn lines(start: u32, end: u32) -> LineRange {
    LineRange { start, end: End::Line(end) }
}

fn search(query: &str, json: bool) -> Command {
    Command::Search { query: query.to_owned(), json }
}

fn show(token: &str, lines: Option<LineRange>, json: bool) -> Command {
    Command::Show { address: address(token), lines, json }
}

fn commit(token: &str, json: bool) -> Command {
    Command::ShowCommit { address: address(token), json }
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

fn check_parses(table: Vec<(Vec<&str>, Command)>) {
    for (args, command) in table {
        let parsed =
            parse(&os(&args)).unwrap_or_else(|e| panic!("{args:?}: {e}"));
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
    let mut out = Vec::new();
    report(&error, &mut out).unwrap_or_else(|e| panic!("{e}"));
    String::from_utf8(out).unwrap_or_else(|e| panic!("{e}"))
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
        assert_eq!(
            reported(&os(args)),
            expected(message, &[], synopsis),
            "{args:?}"
        );
    }
}

fn check_errors_with_fact(synopsis: &str, table: &[(&[&str], &str, &str)]) {
    for (args, message, fact) in table {
        assert_eq!(
            reported(&os(args)),
            expected(message, &[fact], synopsis),
            "{args:?}"
        );
    }
}

#[test]
fn show_parses_a_file_address_with_lines_and_options() {
    let blob = "https://github.com/x/y/blob/main/a.go#L3-L9";
    check_parses(vec![
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
            show(
                "x/y:a.go",
                Some(LineRange { start: 4, end: End::Open }),
                false,
            ),
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
    ]);
}

#[test]
fn show_of_a_revision_is_a_commit() {
    let blob = format!("https://github.com/o/r/commit/{SHA}");
    let sourcegraph =
        format!("sourcegraph.com/github.com/o/r@v1/-/commit/{SHA}");
    check_parses(vec![
        (vec!["show", "x/y@v1"], commit("x/y@v1", false)),
        (vec!["show", "--json", "x/y@v1"], commit("x/y@v1", true)),
        (vec!["show", "x/y@v1", "--json"], commit("x/y@v1", true)),
        (
            vec!["show", "github.com/x/y@HEAD~2"],
            commit("github.com/x/y@HEAD~2", false),
        ),
        (vec!["show", &blob], commit(&blob, false)),
        (vec!["show", &sourcegraph], commit(&sourcegraph, false)),
    ]);
}

#[test]
fn help_and_version_parse_with_or_without_a_command() {
    check_parses(vec![
        (vec!["--help"], Command::Help(None)),
        (vec!["-h"], Command::Help(None)),
        (vec!["show", "--help"], Command::Help(Some("show"))),
        (vec!["-h", "show"], Command::Help(Some("show"))),
        (vec!["show", "-h", "--json"], Command::Help(Some("show"))),
        (vec!["log", "--help"], Command::Help(Some("log"))),
        (vec!["-h", "diff"], Command::Help(Some("diff"))),
        (vec!["diff", "x/y@v1", "-h"], Command::Help(Some("diff"))),
        (vec!["ls", "--help"], Command::Help(Some("ls"))),
        (vec!["-h", "ls"], Command::Help(Some("ls"))),
        (vec!["ls", "--recursive", "--help"], Command::Help(Some("ls"))),
        (vec!["refs", "--help"], Command::Help(Some("refs"))),
        (vec!["refs", "x/y", "-h"], Command::Help(Some("refs"))),
        (vec!["search", "foo", "-h"], Command::Help(Some("search"))),
        (vec!["search", "--help"], Command::Help(Some("search"))),
        (vec!["-h", "search"], Command::Help(Some("search"))),
        (vec!["--version"], Command::Version),
        (vec!["show", "--version"], Command::Version),
        (vec!["ls", "--version"], Command::Version),
        (vec!["search", "--version"], Command::Version),
    ]);
}

#[test]
fn search_joins_its_operands_into_a_query() {
    check_parses(vec![
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
    ]);
}

#[test]
fn ls_parses_an_address_with_recursive_and_json() {
    let tree = "https://github.com/x/y/tree/main/src";
    check_parses(vec![
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
    ]);
}

#[test]
fn refs_parses_a_repository_and_text() {
    check_parses(vec![
        (vec!["refs", "x/y"], refs("x/y", None, false)),
        (vec!["refs", "--json", "x/y"], refs("x/y", None, true)),
        (vec!["refs", "x/y", "v1"], refs("x/y", Some("v1"), false)),
        (
            vec!["refs", "https://github.com/x/y"],
            refs("https://github.com/x/y", None, false),
        ),
        (vec!["refs", "x/y", "a b "], refs("x/y", Some("a b "), false)),
    ]);
}

#[test]
fn log_parses_an_address_and_one_path() {
    let blob = format!("https://github.com/o/r/blob/{SHA}/x.go");
    let tree = format!("https://github.com/o/r/tree/{SHA}/src");
    let root = format!("https://github.com/o/r/tree/{SHA}");
    let commit = format!("https://github.com/o/r/commit/{SHA}");
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
        (vec!["log", &commit], log(&commit, None, false)),
        (vec!["log", compare2], log(compare2, None, false)),
        (vec!["log", compare3], log(compare2, None, false)),
        (vec!["log", "--", "x/y"], log("x/y", None, false)),
    ]);
}

#[test]
fn diff_parses_a_revision_or_three_dot_range_and_paths() {
    let blob = format!("https://github.com/o/r/blob/{SHA}/x.go");
    let commit = format!("https://github.com/o/r/commit/{SHA}");
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
        (vec!["diff", &commit], diff(&commit, &[], false)),
        (vec!["diff", compare, "x.go"], diff(compare, &["x.go"], false)),
    ]);
}

#[test]
fn usage_errors_before_a_command_carry_the_general_synopsis() {
    let no_command = expected("no command given", &[COMMAND_LIST], SYNOPSIS);
    let unknown = |word: &str| {
        expected(
            &format!("unknown command '{word}'"),
            &[COMMAND_LIST],
            SYNOPSIS,
        )
    };
    let table = [
        (vec![], no_command.clone()),
        (vec!["--json"], no_command),
        (vec!["shwo"], unknown("shwo")),
        (vec!["list", "x/y"], unknown("list")),
        (vec!["list", "x/y:a.go", "3-9"], unknown("list")),
        (vec!["find", "x"], unknown("find")),
        (vec!["shwo", "--help"], unknown("shwo")),
        (vec!["shwo", "--version"], unknown("shwo")),
        (
            vec!["-C", "search", "x"],
            expected("unknown option -C", &[], SYNOPSIS),
        ),
        (
            vec!["-x", "search", "foo"],
            expected("unknown option -x", &[], SYNOPSIS),
        ),
        (
            vec!["--json", "--json"],
            expected("--json given twice", &[], SYNOPSIS),
        ),
        (vec!["--json=1"], expected("--json takes no value", &[], SYNOPSIS)),
        (
            vec!["--limit", "5", "show"],
            expected("unknown option --limit", &[LIMIT], SYNOPSIS),
        ),
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
    check_errors_with_fact(
        SHOW_SYNOPSIS,
        &[
            (&["show", "x/y:a", "--limit"], "unknown option --limit", LIMIT),
            (&["show", "x/y:a", "--limit=5"], "unknown option --limit", LIMIT),
            (
                &["show", "--count", "x/y:a"],
                "unknown option --count",
                "sego has no --count; count: is a query filter",
            ),
            (
                &["show", "--context", "x/y:a"],
                "unknown option --context",
                "sego has no --context; search prints one line of context, \
                 show prints any range",
            ),
            (
                &["show", "--repo", "x/y:a"],
                "unknown option --repo",
                "sego has no --repo; repo: is a query filter",
            ),
            (
                &["show", "--lang", "x/y:a"],
                "unknown option --lang",
                "sego has no --lang; lang: is a query filter",
            ),
            (
                &["show", "--file", "x/y:a"],
                "unknown option --file",
                "sego has no --file; file: is a query filter",
            ),
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
            (&["show", "x/y:"], NEEDS_FILE_PATH),
            (&["show", "x/y@v1:"], NEEDS_FILE_PATH),
            (&["show", "x/y:."], NEEDS_FILE_PATH),
            (&["show", "x/y@a..b:f"], &range("a..b")),
            (&["show", "x/y@a...b:f"], &range("a...b")),
            (
                &["show", "x/y:f", "0"],
                "invalid line range 0; lines are numbered from 1",
            ),
            (
                &["show", "x/y:f", "5-2"],
                "invalid line range 5-2; the end is before the start",
            ),
            (&["show", "x/y@:f"], "the address has an empty revision: x/y@:f"),
            (
                &["show", "https://github.com/x/y/blob/main/a.go#L3", "5-9"],
                "the URL's lines 3 and the LINES operand 5-9 disagree",
            ),
        ],
    );
}

#[test]
fn usage_errors_in_a_commit_address_of_show() {
    let needs_either = "show needs a file's path or a revision, like \
                        REPO[@REV]:PATH or REPO@REV";
    let with_lines = format!("https://github.com/o/r/commit/{SHA}#L3");
    let tree_dot = format!("https://github.com/o/r/tree/{SHA}/.");
    let blob_dot = format!("https://github.com/o/r/blob/{SHA}/.");
    let sourcegraph_dot = "sourcegraph.com/github.com/o/r@v1/-/tree/.";
    check_errors(
        SHOW_SYNOPSIS,
        &[
            (&["show", "x/y"], needs_either),
            (&["show", "x/y@v1:."], NEEDS_FILE_PATH),
            (&["show", &tree_dot], NEEDS_FILE_PATH),
            (&["show", &blob_dot], NEEDS_FILE_PATH),
            (&["show", sourcegraph_dot], NEEDS_FILE_PATH),
            (&["show", "x/y@v1", "3"], "a commit takes no LINES"),
            (&["show", "x/y@v1", "abc"], "a commit takes no LINES"),
            (&["show", &with_lines], "a commit takes no LINES"),
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
            (
                &["ls", "https://github.com/x/y/blob/main/a.go#L3"],
                "ls takes no LINES",
            ),
            (
                &["ls", "x/y@:src"],
                "the address has an empty revision: x/y@:src",
            ),
        ],
    );
}

#[test]
fn usage_errors_in_the_operands_of_refs() {
    let needs_repo = "refs needs a repository, like REPO";
    let names_repo = |token: &str| {
        format!("refs names a repository, not a revision or path: {token}")
    };
    check_errors(
        REFS_SYNOPSIS,
        &[
            (&["refs"], needs_repo),
            (&["refs", "--json"], needs_repo),
            (
                &["refs", "x/y", "a", "b"],
                "refs takes a repository and at most one TEXT",
            ),
            (&["refs", "x/y@v1"], &names_repo("x/y@v1")),
            (&["refs", "github.com/x/y@v1"], &names_repo("github.com/x/y@v1")),
            (&["refs", "x/y:src"], &names_repo("x/y:src")),
            (&["refs", "x/y@v1:src", "t"], &names_repo("x/y@v1:src")),
            (
                &["refs", "https://github.com/x/y/tree/main/src"],
                &names_repo("https://github.com/x/y/tree/main/src"),
            ),
            (&["refs", "x/y", ""], "the TEXT operand is empty"),
            (&["refs", "x/y", "  "], "the TEXT operand is empty"),
            (&["refs", "x/y@:"], "the address has an empty revision: x/y@:"),
        ],
    );
}

#[test]
fn usage_errors_in_the_operands_of_log() {
    let needs_address = "log needs an address, like REPO[@REV][:PATH]";
    let blob_main = "https://github.com/o/r/blob/main/x.go";
    let tree_main = "https://github.com/o/r/tree/main/src";
    let split = "log can't split a branch or tag from the path in a tree \
                 or blob URL";
    let with_lines = format!("https://github.com/o/r/blob/{SHA}/x.go#L3");
    check_errors(
        LOG_SYNOPSIS,
        &[
            (&["log"], needs_address),
            (&["log", "--json"], needs_address),
            (
                &["log", "x/y", "a", "b"],
                "log takes an address and at most one PATH",
            ),
            (&["log", "x/y@a...b"], "log takes A..B, not A...B"),
            (&["log", "x/y@a...b:src"], "log takes A..B, not A...B"),
            (&["log", "x/y", ""], "log takes no empty path"),
            (&["log", "x/y@v1:", ""], "log takes no empty path"),
            (&["log", "x/y@a..b", ""], "log takes no empty path"),
            (&["log", "x/y:src", ""], "log takes no empty path"),
            (&["log", "x/y:src", "lib"], "log takes one path"),
            (&["log", "x/y@a..b:src", "lib"], "log takes one path"),
            (&["log", blob_main], split),
            (&["log", tree_main], split),
            (&["log", &with_lines], "log takes no LINES"),
            (
                &["log", "x/y@:src"],
                "the address has an empty revision: x/y@:src",
            ),
        ],
    );
}

#[test]
fn usage_errors_in_the_operands_of_diff() {
    let blob_main = "https://github.com/o/r/blob/main/x.go";
    let split = "diff can't split a branch or tag from the path in a tree \
                 or blob URL";
    let with_lines = format!("https://github.com/o/r/blob/{SHA}/x.go#L3");
    let needs_revision = "diff needs a revision or a range, like REPO@REV or \
                          REPO@A...B";
    let two_dots =
        "diff takes A...B (Sourcegraph compares from the merge base)";
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
            (&["diff", blob_main], split),
            (&["diff", &with_lines], "diff takes no LINES"),
            (
                &["diff", "x/y@a....b"],
                "invalid range a....b; a range is A..B or A...B",
            ),
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
    check_errors(
        LOG_SYNOPSIS,
        &[(&["log", "--recursive", "x/y"], &takes_none("log"))],
    );
    check_errors(
        DIFF_SYNOPSIS,
        &[(&["diff", "x/y@v1", "--recursive"], &takes_none("diff"))],
    );
    check_errors(
        LS_SYNOPSIS,
        &[
            (
                &["ls", "--recursive", "x/y", "--recursive"],
                "--recursive given twice",
            ),
            (&["ls", "--recursive=1", "x/y"], "--recursive takes no value"),
            (&["ls", "-r", "x/y"], "unknown option -r"),
        ],
    );
    check_errors(
        SYNOPSIS,
        &[(&["--recursive", "--recursive"], "--recursive given twice")],
    );
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
    check_errors_with_fact(
        SEARCH_SYNOPSIS,
        &[(
            &["search", "--limit", "5", "foo"],
            "unknown option --limit",
            LIMIT,
        )],
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
            expected(
                &format!("argument {position} is not valid UTF-8"),
                &[],
                synopsis
            ),
            "{args:?}"
        );
    }
}

fn topics() -> impl Iterator<Item = Option<&'static str>> {
    std::iter::once(None).chain(COMMANDS.iter().map(|entry| Some(entry.name)))
}

#[test]
fn every_command_help_has_the_common_sections_in_order() {
    for entry in COMMANDS {
        let text = help(Some(entry.name));
        let operands = match entry.name {
            "search" => "\nquery:\n",
            "show" | "ls" => "\naddresses:\n",
            _ => "\noperands:\n",
        };
        let mut sections = vec!["usage: ", operands, "\noptions:\n"];
        if entry.name == "search" {
            sections.push("\ndefaults:\n");
        }
        sections.extend(["\ntext:\n", "\njson:\n", "\nexamples:\n"]);
        let at: Vec<Option<usize>> =
            sections.iter().map(|section| text.find(section)).collect();
        assert_eq!(at[0], Some(0), "{}", entry.name);
        assert!(at.iter().all(Option::is_some), "{}: {at:?}", entry.name);
        assert!(at.is_sorted(), "{}: {at:?}", entry.name);
    }
}

#[test]
fn every_command_has_its_own_help() {
    let general = help(None);
    for entry in COMMANDS {
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
fn general_help_fits_50_lines() {
    assert!(help(None).lines().count() <= 50);
}

#[test]
fn help_fits_79_columns() {
    for topic in topics() {
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
    let mut once = named.clone();
    once.dedup();
    let table: Vec<&str> = COMMANDS.iter().map(|entry| entry.name).collect();
    assert_eq!(once, table);
}
