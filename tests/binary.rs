//! One smoke test of the built binary, for what `main` adds to `run`: the
//! process environment, the real streams and the exit status. None of it
//! connects to a server.

use std::io;
use std::process::{Command, Output, Stdio};

fn sego() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_sego"));
    command.env_remove("SEGO_ENDPOINT").env_remove("SEGO_CACHE_DIR");
    command
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn the_built_binary_reports_its_version_reads_its_environment_and_survives_a_closed_stdout()
-> io::Result<()> {
    let version = sego().arg("--version").output()?;
    assert_eq!(version.status.code(), Some(0));
    assert_eq!(
        text(&version.stdout),
        concat!("sego ", env!("CARGO_PKG_VERSION"), "\n")
    );
    assert_eq!(text(&version.stderr), "");

    let Output { status, stdout, stderr } = sego()
        .args(["show", "example.com/x/y:a.go"])
        .env("SEGO_ENDPOINT", "ftp://x")
        .output()?;
    assert_eq!(status.code(), Some(2));
    assert_eq!(text(&stdout), "");
    assert!(
        text(&stderr).starts_with(
            "sego: error: SEGO_ENDPOINT must be an http:// or https:// URL"
        ),
        "{}",
        text(&stderr)
    );

    let (reader, writer) = io::pipe()?;
    drop(reader);
    let closed = sego().arg("--help").stdout(Stdio::from(writer)).output()?;
    assert_eq!(closed.status.code(), Some(0));
    assert_eq!(text(&closed.stderr), "");
    Ok(())
}
