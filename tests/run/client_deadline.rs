use std::ffi::OsStr;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use sego::TransportError;

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

const POINTED: &str = "sego: error: no answer from sourcegraph.com within 90 s\n  repo:, lang: or file: narrows what the search reads\n";

#[test]
fn a_search_past_the_client_deadline_points_to_a_narrower_scope() {
    let queries = [
        "lang:python foo count:all timeout:60s",
        "foo count:100",
        "foo timeout:30s",
        "foo",
    ];
    for query in queries {
        let server = Server::new(vec![Err(TransportError::Deadline)]);
        let ran = sego(&["search", query], OsStr::new("off"), &server);
        assert_eq!(ran.code, ExitCode::from(1), "{query}");
        assert_eq!(ran.stdout, "", "{query}");
        assert_eq!(ran.stderr, POINTED, "{query}");
    }
}

#[test]
fn a_search_that_fails_to_connect_has_no_pointer_to_a_narrower_scope() {
    let server =
        Server::new(vec![Err(TransportError::Network("connection refused"))]);
    let ran = sego(&["search", "foo count:100"], OsStr::new("off"), &server);
    assert_eq!(ran.code, ExitCode::from(1));
    assert_eq!(
        ran.stderr,
        "sego: error: could not reach sourcegraph.com: connection refused\n"
    );
}
