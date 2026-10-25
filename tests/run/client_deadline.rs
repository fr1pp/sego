use std::ffi::OsStr;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use crate::envelope::{SHOW_BIG, json_reply, over_the_content_limit};
use crate::transport::{Server, reply, sego};

const CLIENT_DEADLINE: Duration = Duration::from_secs(90);

#[test]
fn the_client_deadline_starts_once_for_the_whole_invocation() {
    let server = Server::new(vec![
        Ok(json_reply(&over_the_content_limit(2_000_000))),
        Ok(reply(200, "text/plain", b"1\n2\n3\n4\n5\n")),
    ]);
    let before = Instant::now();
    let ran = sego(&SHOW_BIG, OsStr::new("off"), &server);
    let after = Instant::now();

    assert_eq!(ran.stderr, "");
    assert_eq!(ran.code, ExitCode::SUCCESS);
    let calls = server.calls();
    assert_eq!(server.methods(), ["POST", "GET"]);
    for call in &calls {
        assert!(call.deadline >= before + CLIENT_DEADLINE);
        assert!(call.deadline <= after + CLIENT_DEADLINE);
    }
    assert_eq!(calls[1].deadline, calls[0].deadline);
}
