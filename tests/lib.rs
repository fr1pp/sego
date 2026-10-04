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
