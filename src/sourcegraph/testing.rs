//! Captures: real upstream replies to sego's own plans, stored as
//! `tests/<dir>/<case>.json` for the response tests and the mock server;
//! and the helpers those response tests share.

use std::fmt::Debug;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::diagnostic::{Diagnostic, report};

use super::envelope::{Envelope, decode};
use super::transport::Transport;
use super::{Plan, Reply};

#[derive(Serialize, Deserialize)]
struct Capture {
    request: serde_json::Value,
    response: Response,
}

#[derive(Serialize, Deserialize)]
struct Response {
    status: u16,
    body: serde_json::Value,
}

/// The envelope of a capture's reply.
pub fn envelope(capture: &str) -> Envelope {
    let capture: Capture =
        serde_json::from_str(capture).expect("a capture is JSON");
    let Response { status, body } = capture.response;
    let body = serde_json::to_vec(&body).expect("JSON serialises");
    decode(Reply { status, body, trace: None, location: None })
        .expect("a capture's reply is a GraphQL response")
}

/// The envelope of an HTTP 200 reply whose body is `body`.
pub fn decoded(body: impl Into<Vec<u8>>) -> Envelope {
    let body = body.into();
    decode(Reply { status: 200, body, trace: None, location: None })
        .expect("the body is a GraphQL response")
}

/// A failure's code, outcome and report.
pub fn failure<T: Debug>(result: Result<T, Box<dyn Diagnostic>>) -> String {
    let diagnostic = match result {
        Ok(answer) => panic!("an answer: {answer:?}"),
        Err(diagnostic) => diagnostic,
    };
    format!(
        "[{} {:?}]\n{}",
        diagnostic.code(),
        diagnostic.outcome(),
        report(&*diagnostic)
    )
}

/// Sends `plan` to the real endpoint and rewrites
/// `tests/<dir>/<case>.json` with the request and the reply.
pub fn capture(dir: &str, case: &str, plan: &Plan) {
    let transport = crate::endpoint()
        .and_then(|endpoint| Transport::new(&endpoint))
        .expect("a valid SEGO_ENDPOINT");
    let reply = transport.fetch(plan).expect("the upstream answers");
    let body = serde_json::from_slice(&reply.body).unwrap_or_else(|error| {
        panic!("{case}: the reply ({}) isn't JSON: {error}", reply.status)
    });
    let capture = Capture {
        request: plan.request(),
        response: Response { status: reply.status, body },
    };
    let mut text =
        serde_json::to_string_pretty(&capture).expect("JSON serialises");
    text.push('\n');
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join(dir)
        .join(format!("{case}.json"));
    fs::create_dir_all(path.parent().expect("a file has a parent"))
        .expect("the capture directory can be created");
    fs::write(&path, text).expect("the capture can be written");
}
