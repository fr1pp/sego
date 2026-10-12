use std::time::{Duration, Instant};

use sego::internal::error::Kind;
use sego::internal::upstream::{RawError, Reply, TransportError, read};

use crate::support::fake::Fake;

fn reply(status: u16, content_type: &str, body: &[u8]) -> Reply {
    Reply {
        status,
        content_type: Some(content_type.to_owned()),
        location: None,
        retry_after: None,
        cf_mitigated: None,
        body: body.to_vec(),
    }
}

fn read_with(
    answer: Result<Reply, TransportError>,
) -> Result<Vec<u8>, RawError> {
    let fake = Fake::scripted(vec![answer]);
    let deadline = Instant::now() + Duration::from_secs(90);
    let result = read(&fake, "/r/x/-/raw/a", deadline);
    let calls = fake.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(
        (calls[0].method, calls[0].path.as_str()),
        ("GET", "/r/x/-/raw/a")
    );
    assert_eq!(calls[0].deadline, deadline);
    result
}

fn failure(answer: Result<Reply, TransportError>) -> (Kind, String) {
    match read_with(answer) {
        Err(RawError::Failed(error)) => (error.kind(), error.to_string()),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_200_answer_is_the_body() {
    let body = read_with(Ok(reply(200, "text/plain", b"a\nb\n")));
    assert_eq!(body.unwrap_or_default(), b"a\nb\n");
}

#[test]
fn a_404_is_not_found() {
    let result = read_with(Ok(reply(404, "text/html", b"<html>")));
    assert!(matches!(result, Err(RawError::NotFound)), "{result:?}");
}

#[test]
fn failures_name_the_transports_host() {
    let fake = Fake::scripted(vec![
        Ok(reply(500, "text/plain", b"bad")),
        Err(TransportError::Network("connection refused")),
    ])
    .named("h.example:7");
    let deadline = Instant::now() + Duration::from_secs(90);
    let messages: Vec<String> = (0..2)
        .map(|_| match read(&fake, "/r/x/-/raw/a", deadline) {
            Err(RawError::Failed(error)) => error.to_string(),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(
        messages,
        [
            "h.example:7 answered HTTP 500",
            "could not reach h.example:7: connection refused"
        ]
    );
}

#[test]
fn everything_else_is_a_failure() {
    assert_eq!(
        failure(Ok(reply(200, "text/html; charset=utf-8", b"<html>"))),
        (
            Kind::Upstream,
            "sourcegraph.com answered HTTP 200 text/html".to_owned()
        )
    );
    assert_eq!(
        failure(Ok(reply(502, "text/plain", b"bad"))),
        (Kind::Upstream, "sourcegraph.com answered HTTP 502".to_owned())
    );
    assert_eq!(failure(Err(TransportError::Deadline)).0, Kind::Deadline);
}
