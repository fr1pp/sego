//! Tests against the real endpoint: `SEGO_ENDPOINT` if set, else the
//! default. Run by `just live`.

use crate::Command;
use crate::sego;

/// sego with the caller's `SEGO_ENDPOINT`, or the default when unset.
fn live() -> Command {
    sego().env(
        "SEGO_ENDPOINT",
        std::env::var_os("SEGO_ENDPOINT").unwrap_or_default(),
    )
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
