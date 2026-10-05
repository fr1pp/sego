//! Tests against the real endpoint: `SEGO_ENDPOINT` if set, else the
//! default. Run by `just live`.

use crate::Command;

/// sego with the caller's environment, proxy included; `SEGO_ENDPOINT` is
/// the caller's, or the default when unset.
fn live() -> Command {
    Command::bin(env!("CARGO_BIN_EXE_sego"))
}

#[test]
#[ignore = "network: run by just live"]
fn read_live() {
    let run = live().args([
        "read",
        "github.com/gorilla/mux@v1.7.0",
        "mux.go",
        "1-5",
    ]);
    let run = run.run();
    assert_eq!(run.code, 0, "{}", run.transcript());
}

#[test]
#[ignore = "network: run by just live"]
fn blame_live() {
    let run = live().args([
        "blame",
        "github.com/gorilla/mux@v1.7.0",
        "mux.go",
        "214-221",
    ]);
    let run = run.run();
    assert_eq!(run.code, 0, "{}", run.transcript());
}

#[test]
#[ignore = "network: run by just live"]
fn list_live() {
    let run = live().args(["list", "github.com/gorilla/mux@v1.7.0"]).run();
    assert_eq!(run.code, 0, "{}", run.transcript());
}

#[test]
#[ignore = "network: run by just live"]
fn search_live() {
    let run = live().args([
        "search",
        r"repo:^github\.com/gorilla/mux$@v1.7.0",
        "NewRouter",
        "case:yes",
    ]);
    let run = run.run();
    assert_eq!(run.code, 0, "{}", run.transcript());
}
