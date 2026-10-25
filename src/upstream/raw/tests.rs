use std::time::{Duration, Instant};

use super::{RawError, read};
use crate::error::Kind;
use crate::upstream::script::{Script, reply};
use crate::upstream::{Reply, TransportError};

fn failure(answer: Result<Reply, TransportError>) -> (Kind, String) {
    let script = Script::new(vec![answer]);
    let deadline = Instant::now() + Duration::from_secs(90);
    match read(&script, "/r/x/-/raw/a", deadline) {
        Err(RawError::Failed(error)) => (error.kind(), error.to_string()),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_200_answer_is_the_body() {
    let script =
        Script::new(vec![Ok(reply(200, Some("text/plain"), "a\nb\n"))]);
    let deadline = Instant::now() + Duration::from_secs(90);
    let body = read(&script, "/r/x/-/raw/a", deadline);
    assert_eq!(body.unwrap_or_default(), b"a\nb\n");
    let calls = script.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(
        (calls[0].method, calls[0].path.as_str()),
        ("GET", "/r/x/-/raw/a")
    );
    assert_eq!(calls[0].deadline, deadline);
}

#[test]
fn a_404_is_not_found() {
    let script =
        Script::new(vec![Ok(reply(404, Some("text/html"), "<html>"))]);
    let deadline = Instant::now() + Duration::from_secs(90);
    let result = read(&script, "/r/x/-/raw/a", deadline);
    assert!(matches!(result, Err(RawError::NotFound)), "{result:?}");
}

#[test]
fn everything_else_is_a_failure() {
    assert_eq!(
        failure(Ok(reply(200, Some("text/html; charset=utf-8"), "<html>"))),
        (
            Kind::Upstream,
            "sourcegraph.com answered HTTP 200 text/html".to_owned()
        )
    );
    assert_eq!(
        failure(Ok(reply(502, Some("text/plain"), "bad"))),
        (Kind::Upstream, "sourcegraph.com answered HTTP 502".to_owned())
    );
    assert_eq!(failure(Err(TransportError::Deadline)).0, Kind::Deadline);
}
