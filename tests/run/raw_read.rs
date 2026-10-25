use std::ffi::OsStr;
use std::process::ExitCode;

use sego::TransportError;

use crate::envelope::{SHOW_BIG, json_reply, over_the_content_limit};
use crate::transport::{Server, sego};

#[test]
fn a_raw_read_that_passes_the_client_deadline_writes_only_its_error() {
    let server = Server::new(vec![
        Ok(json_reply(&over_the_content_limit(2_000_000))),
        Err(TransportError::Deadline),
    ]);
    let ran = sego(&SHOW_BIG, OsStr::new("off"), &server);

    assert_eq!(ran.code, ExitCode::from(1));
    assert_eq!(ran.stdout, "");
    assert_eq!(
        ran.stderr,
        "sego: error: no answer from sourcegraph.com within 90 s\n"
    );
    let calls = server.calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[1].deadline, calls[0].deadline);
}
