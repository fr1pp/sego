#![allow(clippy::unwrap_used, clippy::expect_used)]

#[macro_use]
mod command;
mod blame;
mod compare;
mod list;
mod live;
mod read;
mod search;
mod server;

use command::Command;
use expect_test::expect;
use server::Server;

/// The variables sego's HTTP client reads a proxy from.
const PROXY_VARIABLES: &[&str] = &[
    "ALL_PROXY",
    "all_proxy",
    "HTTPS_PROXY",
    "https_proxy",
    "HTTP_PROXY",
    "http_proxy",
];

/// sego, pointed at a port nothing listens on and with no proxy, so no test
/// reaches the network or depends on the caller's `SEGO_ENDPOINT` or proxy.
#[must_use]
pub fn sego() -> Command {
    let sego = Command::bin(env!("CARGO_BIN_EXE_sego"));
    PROXY_VARIABLES
        .iter()
        .fold(sego, |sego, variable| sego.env_remove(variable))
        .env("SEGO_ENDPOINT", server::unreachable())
}

/// sego, pointed at `server`.
#[must_use]
pub fn sego_at(server: &Server) -> Command {
    sego().env("SEGO_ENDPOINT", server.url())
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
          sego search  [--json] <query>...
          sego list    [--json] [--depth N] <repo>[@<rev>] [<path>]
          sego read    [--json] <repo>[@<rev>] <path> [<lines>]
          sego blame   [--json] <repo>[@<rev>] <path> <lines>
          sego compare [--json] <repo>@[<base>...]<head> [<path>]
          sego <command> --help

        Commands:
          search   where is it? Matching code, paths, symbols, commits and repos
          list     what is here? The entries of a directory
          read     what does it say? The lines of a file
          blame    why, and when? The commits that last changed a file's lines
          compare  what changed? The commits and files changed in a range or a commit

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
        error: missing command; expected search, list, read, blame or compare
        usage: sego search  [--json] <query>...
               sego list    [--json] [--depth N] <repo>[@<rev>] [<path>]
               sego read    [--json] <repo>[@<rev>] <path> [<lines>]
               sego blame   [--json] <repo>[@<rev>] <path> <lines>
               sego compare [--json] <repo>@[<base>...]<head> [<path>]
        [EOF]
    "#]]
    );
}

#[test]
fn unknown_command_shows_every_synopsis() {
    check!(
        sego().args(["serch", "NewRouter"]),
        expect![[r#"
        exit 2
        --- stdout
        [EOF]
        --- stderr
        error: unknown command serch
        usage: sego search  [--json] <query>...
               sego list    [--json] [--depth N] <repo>[@<rev>] [<path>]
               sego read    [--json] <repo>[@<rev>] <path> [<lines>]
               sego blame   [--json] <repo>[@<rev>] <path> <lines>
               sego compare [--json] <repo>@[<base>...]<head> [<path>]
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
        {"version":1,"kind":"error","code":"usage","message":"unknown command serch","hints":[],"usage":["sego search  [--json] <query>...","sego list    [--json] [--depth N] <repo>[@<rev>] [<path>]","sego read    [--json] <repo>[@<rev>] <path> [<lines>]","sego blame   [--json] <repo>[@<rev>] <path> <lines>","sego compare [--json] <repo>@[<base>...]<head> [<path>]"]}
        [EOF]
        --- stderr
        error: unknown command serch
        usage: sego search  [--json] <query>...
               sego list    [--json] [--depth N] <repo>[@<rev>] [<path>]
               sego read    [--json] <repo>[@<rev>] <path> [<lines>]
               sego blame   [--json] <repo>[@<rev>] <path> <lines>
               sego compare [--json] <repo>@[<base>...]<head> [<path>]
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
        usage: sego search [--json] <query>...
        [EOF]
    "#]]
    );
}

#[test]
fn empty_operands_are_usage_errors() {
    check!(
        sego().args(["read", "", "mux.go"]),
        expect![[r#"
        exit 2
        --- stdout
        [EOF]
        --- stderr
        error: empty repository name
        usage: sego read [--json] <repo>[@<rev>] <path> [<lines>]
        [EOF]
    "#]]
    );
    check!(
        sego().args(["read", "github.com/gorilla/mux", ""]),
        expect![[r#"
        exit 2
        --- stdout
        [EOF]
        --- stderr
        error: empty path; read needs a file's path
        usage: sego read [--json] <repo>[@<rev>] <path> [<lines>]
        [EOF]
    "#]]
    );
}
