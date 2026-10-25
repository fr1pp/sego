use std::cell::RefCell;
use std::fs;
use std::path::Path;
use std::rc::Rc;
use std::time::Instant;

use serde_json::Value;
use tempfile::TempDir;

use sego::{Reply, Transport, TransportError};

use super::check;
use crate::case::Case;
use crate::replay::reply;

/// A server that answers each request with the next reply and keeps what it
/// was asked.
#[derive(Clone, Default)]
struct Server {
    replies: Rc<RefCell<Vec<Reply>>>,
    asked: Rc<RefCell<Vec<String>>>,
    endpoints: Rc<RefCell<Vec<String>>>,
}

impl Server {
    fn answering(replies: Vec<Reply>) -> Server {
        let server = Server::default();
        *server.replies.borrow_mut() = replies;
        server.replies.borrow_mut().reverse();
        server
    }

    fn serve(&self, what: String) -> Result<Reply, TransportError> {
        self.asked.borrow_mut().push(what);
        self.replies
            .borrow_mut()
            .pop()
            .ok_or(TransportError::Network("no reply left"))
    }

    fn asked(&self) -> Vec<String> {
        self.asked.borrow().clone()
    }

    fn check(&self, case: &Case) -> Vec<String> {
        let server = self.clone();
        check(case, &move |endpoint| {
            server.endpoints.borrow_mut().push(endpoint.to_owned());
            Ok(Box::new(server.clone()))
        })
    }
}

impl Transport for Server {
    fn post(
        &self,
        path: &str,
        body: &[u8],
        _: Instant,
    ) -> Result<Reply, TransportError> {
        self.serve(format!("POST {path} {}", String::from_utf8_lossy(body)))
    }

    fn get(&self, path: &str, _: Instant) -> Result<Reply, TransportError> {
        self.serve(format!("GET {path}"))
    }
}

const STAMP: &str = "recorded 2026-10-07 https://sourcegraph.com";

fn graphql(response_line: &str, body: &str) -> String {
    format!(
        "--- request Refs {{\"repo0\":\"example.com/a\"}}\n--- query\nquery \
         Refs($repo0: String!) {{ r0 {{ name }} }}\n--- response \
         {response_line}\n{body}\n"
    )
}

fn parse(exchanges: &[String]) -> Case {
    let text = format!("args: x\nexit: 0\n{}", exchanges.concat());
    Case::parse(&text, Path::new("x.case")).unwrap_or_else(|e| panic!("{e}"))
}

#[test]
fn a_recorded_request_is_sent_again_as_written() {
    let case = parse(&[graphql(&format!("200 {STAMP}"), r#"{"data":{}}"#)]);
    let server = Server::answering(vec![reply(200, None, br#"{"data":{}}"#)]);
    assert_eq!(server.check(&case), Vec::<String>::new());
    let sent = server.asked();
    assert_eq!(sent.len(), 1);
    let body = sent[0].strip_prefix("POST /.api/graphql ").unwrap_or_default();
    let body: Value =
        serde_json::from_str(body).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(body["operationName"], "Refs");
    assert_eq!(body["variables"]["repo0"], "example.com/a");
    assert_eq!(body["query"], "query Refs($repo0: String!) { r0 { name } }\n");
    assert_eq!(*server.endpoints.borrow(), ["https://sourcegraph.com"]);
}

#[test]
fn a_response_with_no_recording_is_never_sent_again() {
    let case = parse(&[
        graphql("200", r#"{"data":{}}"#),
        graphql("502", "{}"),
        "--- request GET /raw\n--- response network error\n".to_owned(),
    ]);
    let server = Server::answering(Vec::new());
    assert_eq!(server.check(&case), Vec::<String>::new());
    assert_eq!(server.asked(), Vec::<String>::new());
}

#[test]
fn each_recorded_exchange_is_sent_and_the_rest_are_not() {
    let case = parse(&[
        graphql("200", r#"{"data":{}}"#),
        graphql(&format!("200 {STAMP}"), r#"{"data":{}}"#),
        "--- request GET /raw\n--- response 200 text/plain recorded \
         2026-10-07 https://sourcegraph.com\n"
            .to_owned(),
    ]);
    let server = Server::answering(vec![
        reply(200, None, br#"{"data":{}}"#),
        reply(200, None, b"raw"),
    ]);
    assert_eq!(server.check(&case), Vec::<String>::new());
    assert_eq!(server.asked().len(), 2);
    assert_eq!(server.asked()[1], "GET /raw");
}

#[test]
fn a_changed_shape_fails_naming_the_exchange() {
    let case = parse(&[graphql(
        &format!("200 {STAMP}"),
        r#"{"data":{"r0":{"name":"a"}}}"#,
    )]);
    let server =
        Server::answering(vec![reply(200, None, br#"{"data":{"r0":null}}"#)]);
    let failures = server.check(&case);
    assert_eq!(failures.len(), 1);
    assert!(failures[0].starts_with("x.case: exchange 1 Refs: shape:"));
}

#[test]
fn a_difference_the_second_answer_does_not_repeat_is_not_drift() {
    let case = parse(&[graphql(
        &format!("200 {STAMP}"),
        r#"{"data":{"r0":{"name":"a"}}}"#,
    )]);
    let server = Server::answering(vec![
        reply(200, None, br#"{"data":{"r0":null}}"#),
        reply(200, None, br#"{"data":{"r0":{"name":"a"}}}"#),
    ]);
    assert_eq!(server.check(&case), Vec::<String>::new());
    assert_eq!(server.asked().len(), 2);
}

#[test]
fn a_difference_the_second_answer_repeats_fails_once() {
    let case = parse(&[graphql(
        &format!("200 {STAMP}"),
        r#"{"data":{"r0":{"name":"a"}}}"#,
    )]);
    let server = Server::answering(vec![
        reply(200, None, br#"{"data":{"r0":null}}"#),
        reply(200, None, br#"{"data":{"r0":null}}"#),
    ]);
    let failures = server.check(&case);
    assert_eq!(failures.len(), 1);
    assert!(failures[0].starts_with("x.case: exchange 1 Refs: shape:"));
    assert_eq!(server.asked().len(), 2);
}

#[test]
fn values_are_compared_only_when_the_response_is_marked() {
    let recorded = r#"{"data":{"r0":{"name":"a"}}}"#;
    let case = parse(&[graphql(&format!("200 {STAMP}"), recorded)]);
    let server = Server::answering(vec![reply(
        200,
        None,
        br#"{"data":{"r0":{"name":"b"}}}"#,
    )]);
    assert_eq!(server.check(&case), Vec::<String>::new());

    let case =
        parse(&[graphql(&format!("200 {STAMP} live: values"), recorded)]);
    let server = Server::answering(vec![reply(
        200,
        None,
        br#"{"data":{"r0":{"name":"b"}}}"#,
    )]);
    let failures = server.check(&case);
    assert_eq!(failures.len(), 1);
    assert!(failures[0].contains("value: data.r0.name changed"));
}

#[test]
fn a_different_status_fails() {
    let case = parse(&[graphql(&format!("200 {STAMP}"), "{}")]);
    let server = Server::answering(vec![reply(400, None, b"{}")]);
    let failures = server.check(&case);
    assert_eq!(failures.len(), 1);
    assert!(failures[0].ends_with("status 400, was 200"));
}

#[test]
fn a_body_that_is_not_json_fails() {
    let case = parse(&[graphql(&format!("200 {STAMP}"), "{}")]);
    let server = Server::answering(vec![reply(200, None, b"<html>")]);
    let failures = server.check(&case);
    assert_eq!(failures.len(), 1);
    assert!(failures[0].contains("body is not JSON"));
}

#[test]
fn a_transport_error_fails() {
    let case = parse(&[graphql(&format!("200 {STAMP}"), "{}")]);
    let server = Server::answering(Vec::new());
    let failures = server.check(&case);
    assert_eq!(failures.len(), 1);
    assert!(failures[0].contains("transport: Network"));
}

#[test]
fn a_recorded_response_with_no_request_fails_without_sending() {
    let case = parse(&[format!("--- response 200 {STAMP}\n{{}}\n")]);
    let server = Server::answering(Vec::new());
    let failures = server.check(&case);
    assert_eq!(failures.len(), 1);
    assert!(failures[0].contains("has no request to send again"));
    assert_eq!(server.asked(), Vec::<String>::new());
}

#[test]
fn a_raw_read_compares_its_body_only_when_marked() {
    let dir = TempDir::new().unwrap_or_else(|e| panic!("{e}"));
    let path = dir.path().join("raw.case");
    fs::write(path.with_extension("body"), b"recorded")
        .unwrap_or_else(|e| panic!("{e}"));
    let load = |marker: &str| {
        let text = format!(
            "args: x\nexit: 0\n--- request GET /raw\n--- response 200 \
             text/plain {STAMP}{marker}\n"
        );
        Case::parse(&text, &path).unwrap_or_else(|e| panic!("{e}"))
    };

    let server = Server::answering(vec![reply(200, None, b"moved")]);
    assert_eq!(server.check(&load("")), Vec::<String>::new());

    let server = Server::answering(vec![reply(200, None, b"recorded")]);
    assert_eq!(server.check(&load(" live: values")), Vec::<String>::new());

    let server = Server::answering(vec![reply(200, None, b"moved")]);
    let failures = server.check(&load(" live: values"));
    assert_eq!(failures.len(), 1);
    assert!(
        failures[0].contains("raw.case: exchange 1 GET /raw: body 5 bytes")
    );
}
