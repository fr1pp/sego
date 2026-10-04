#![allow(clippy::unwrap_used, clippy::expect_used)]

#[macro_use]
mod command;

use command::Command;
use expect_test::expect;

#[must_use]
pub fn sego() -> Command {
    Command::bin(env!("CARGO_BIN_EXE_sego"))
}

#[test]
fn version() {
    check!(
        sego().arg("--version"),
        expect![[r#"
            exit 0
            --- stdout
            sego 0.1.0
            [EOF]
            --- stderr
            [EOF]
        "#]]
    );
}

#[test]
fn help() {
    check!(
        sego().arg("--help"),
        expect![[r#"
        exit 0
        --- stdout
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
        [EOF]
        --- stderr
        [EOF]
    "#]]
    );
}

#[test]
fn help_fits_79_columns() {
    let run = sego().arg("--help").run();
    assert_eq!(run.code, 0);
    for line in run.stdout.lines() {
        assert!(line.chars().count() <= 79, "too wide: {line}");
    }
}

#[test]
fn no_command() {
    check!(
        sego(),
        expect![[r#"
        exit 2
        --- stdout
        [EOF]
        --- stderr
        error: missing command; expected search, list or read
        hint: sego --help
        [EOF]
    "#]]
    );
}

#[test]
fn unknown_command_suggests_one() {
    check!(
        sego().args(["serch", "NewRouter"]),
        expect![[r#"
        exit 2
        --- stdout
        [EOF]
        --- stderr
        error: unknown command serch
        hint: sego search NewRouter
        [EOF]
    "#]]
    );
}

#[test]
fn json_usage_errors_go_to_both_streams() {
    check!(
        sego().args(["--json", "serch", "x"]),
        expect![[r#"
        exit 2
        --- stdout
        {"version":1,"kind":"error","code":"usage","message":"unknown command serch","hints":["sego search x --json"]}
        [EOF]
        --- stderr
        error: unknown command serch
        hint: sego search x --json
        [EOF]
    "#]]
    );
}

#[test]
fn depth_only_applies_to_list() {
    check!(
        sego().args(["search", "x", "--depth", "2"]),
        expect![[r#"
        exit 2
        --- stdout
        [EOF]
        --- stderr
        error: --depth applies only to list
        hint: sego search x
        [EOF]
    "#]]
    );
}

#[test]
fn empty_operands_are_usage_errors() {
    check!(
        sego().args(["read", ""]),
        expect![[r#"
        exit 2
        --- stdout
        [EOF]
        --- stderr
        error: read is not implemented yet
        [EOF]
    "#]]
    );
}
