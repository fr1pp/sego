use std::cell::RefCell;
use std::collections::VecDeque;
use std::fs;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant, UNIX_EPOCH};

use serde_json::json;
use tempfile::TempDir;

use sego::{Reply, Transport, TransportError};

use super::{Recorder, date, record_case, write_recorded};
use crate::case::{Case, Headers, Request, Served, Stamp};
use crate::replay::Replay;
use crate::run::run_case;

type Calls = Rc<RefCell<Vec<String>>>;

/// An upstream that answers with the replies it was given, in order, and
/// logs `METHOD path` for each request.
struct Upstream {
    replies: RefCell<VecDeque<Reply>>,
    calls: Calls,
}

impl Transport for Upstream {
    fn post(
        &self,
        path: &str,
        _body: &[u8],
        _deadline: Instant,
    ) -> Result<Reply, TransportError> {
        self.answer(format!("POST {path}"))
    }

    fn get(
        &self,
        path: &str,
        _deadline: Instant,
    ) -> Result<Reply, TransportError> {
        self.answer(format!("GET {path}"))
    }
}

impl Upstream {
    fn answer(&self, call: String) -> Result<Reply, TransportError> {
        self.calls.borrow_mut().push(call);
        self.replies
            .borrow_mut()
            .pop_front()
            .ok_or(TransportError::Network("no reply left"))
    }
}

fn reply(status: u16, body: &str) -> Reply {
    Reply {
        status,
        content_type: Some("application/json".to_owned()),
        location: None,
        retry_after: None,
        cf_mitigated: None,
        body: body.as_bytes().to_vec(),
    }
}

fn recorder(replies: Vec<Reply>) -> (Recorder, Calls) {
    let calls = Calls::default();
    let upstream = Upstream {
        replies: RefCell::new(replies.into()),
        calls: calls.clone(),
    };
    let stamp = Stamp {
        date: "2026-10-07".to_owned(),
        endpoint: "https://sourcegraph.com".to_owned(),
        live_values: false,
    };
    (Recorder::new(Box::new(upstream), stamp), calls)
}

const STAMP: &str = "recorded 2026-10-07 https://sourcegraph.com";

const SERVER_JSON: &str = r#"{
  "data": {"r0": null, "list": [{"requestId": "r", "k": 2}]},
  "extensions": {"trace": "t"},
  "requestId": "q"
}"#;

const RECORDED_JSON: &str = "{\n  \"data\": {\n    \"list\": [\n      {\n        \
                             \"k\": 2\n      }\n    ],\n    \"r0\": null\n  }\n}\n";

struct Written {
    _dir: TempDir,
    path: PathBuf,
}

impl Written {
    fn new(text: &str) -> Written {
        let dir = TempDir::new().unwrap_or_else(|e| panic!("{e}"));
        let path = dir.path().join("x.case");
        fs::write(&path, text).unwrap_or_else(|e| panic!("{e}"));
        Written { _dir: dir, path }
    }

    fn text(&self) -> String {
        fs::read_to_string(&self.path).unwrap_or_else(|e| panic!("{e}"))
    }

    fn case(&self) -> Case {
        Case::load(&self.path).unwrap_or_else(|e| panic!("{e}"))
    }
}

const NEW_CASE: &str = "\
# Not found.
args: refs example.com/a/b
exit: 1
--- stderr
sego: error: example.com/a/b not found
";

fn record(
    written: &Written,
    replies: Vec<Reply>,
) -> (Result<(), String>, Calls) {
    let (recorder, calls) = recorder(replies);
    (record_case(&mut written.case(), recorder), calls)
}

#[test]
fn a_request_with_no_exchange_is_forwarded_and_written_with_its_stamp() {
    let written = Written::new(NEW_CASE);
    let (result, calls) = record(&written, vec![reply(200, SERVER_JSON)]);
    result.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(*calls.borrow(), ["POST /.api/graphql"]);
    let text = written.text();
    let tail = text
        .strip_prefix(NEW_CASE)
        .unwrap_or_else(|| panic!("the case changed:\n{text}"));
    let (request, rest) = tail
        .split_once("--- query\n")
        .unwrap_or_else(|| panic!("no query:\n{tail}"));
    assert_eq!(
        request,
        "--- request Refs \
         {\"first\":101,\"query\":null,\"repo0\":\"example.com/a/b\"}\n"
    );
    let (query, response) = rest
        .split_once("--- response")
        .unwrap_or_else(|| panic!("no response:\n{rest}"));
    assert!(query.starts_with("query Refs("), "{query}");
    assert_eq!(
        format!("--- response{response}"),
        format!("--- response 200 application/json {STAMP}\n{RECORDED_JSON}")
    );
}

#[test]
fn a_recorded_case_replays_without_an_upstream() {
    let written = Written::new(NEW_CASE);
    let (result, _) = record(&written, vec![reply(200, SERVER_JSON)]);
    result.unwrap_or_else(|e| panic!("{e}"));
    let case = written.case();
    let outcome = run_case(&case).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(outcome.sent.len(), 1);
    assert!(
        outcome.sent[0].same_as(
            case.exchanges[0]
                .request
                .as_ref()
                .unwrap_or_else(|| panic!("no request"))
        )
    );
    assert!(outcome.problems.is_empty(), "{:?}", outcome.problems);
}

#[test]
fn a_request_that_has_an_exchange_is_not_forwarded() {
    let written = Written::new(NEW_CASE);
    let (result, _) = record(&written, vec![reply(200, SERVER_JSON)]);
    result.unwrap_or_else(|e| panic!("{e}"));
    let recorded = written.text();
    let (result, calls) = record(&written, vec![reply(200, "{}")]);
    result.unwrap_or_else(|e| panic!("{e}"));
    assert!(calls.borrow().is_empty());
    assert_eq!(written.text(), recorded);
}

#[test]
fn a_recorded_exchange_whose_request_changed_is_recorded_again() {
    let written = Written::new(NEW_CASE);
    let (result, _) = record(&written, vec![reply(200, SERVER_JSON)]);
    result.unwrap_or_else(|e| panic!("{e}"));
    let stale =
        written.text().replace("example.com/a/b\"}", "example.com/c/d\"}");
    fs::write(&written.path, &stale).unwrap_or_else(|e| panic!("{e}"));
    let (result, calls) = record(&written, vec![reply(200, SERVER_JSON)]);
    result.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(calls.borrow().len(), 1);
    assert_eq!(written.case().exchanges.len(), 1);
    let text = written.text();
    assert!(text.contains("\"repo0\":\"example.com/a/b\""), "{text}");
    assert!(!text.contains("example.com/c/d"), "{text}");
}

#[test]
fn recording_again_keeps_the_live_values_mark() {
    let written = Written::new(NEW_CASE);
    let (result, _) = record(&written, vec![reply(200, SERVER_JSON)]);
    result.unwrap_or_else(|e| panic!("{e}"));
    let marked = written
        .text()
        .replace(&format!("{STAMP}\n"), &format!("{STAMP} live: values\n"))
        .replace("example.com/a/b\"}", "example.com/c/d\"}");
    fs::write(&written.path, marked).unwrap_or_else(|e| panic!("{e}"));
    let (result, _) = record(&written, vec![reply(200, SERVER_JSON)]);
    result.unwrap_or_else(|e| panic!("{e}"));
    let text = written.text();
    assert!(
        text.contains(&format!("{STAMP} live: values\n{RECORDED_JSON}")),
        "{text}"
    );
}

#[test]
fn an_exchange_without_a_stamp_is_never_forwarded() {
    let written = Written::new(&format!(
        "{NEW_CASE}--- request Refs {{}}\n--- query\nstale\n--- response \
         200 application/json\n{{\"data\": null}}\n"
    ));
    let before = written.text();
    let (result, calls) = record(&written, vec![reply(200, "{}")]);
    result.unwrap_or_else(|e| panic!("{e}"));
    assert!(calls.borrow().is_empty());
    assert_eq!(written.text(), before);
}

#[test]
fn refused_replies_fail_with_the_status_and_write_nothing() {
    for (status, mitigated, message) in [
        (302, None, "HTTP 302"),
        (429, None, "HTTP 429"),
        (502, None, "HTTP 502"),
        (403, Some("challenge"), "HTTP 403 (cf-mitigated: challenge)"),
    ] {
        let written = Written::new(NEW_CASE);
        let mut refused = reply(status, "upstream says no");
        refused.cf_mitigated = mitigated.map(str::to_owned);
        let (result, calls) = record(&written, vec![refused]);
        let error = result.err().unwrap_or_else(|| panic!("{status} kept"));
        assert!(
            error.ends_with(&format!(
                "recording failed: upstream answered {message}"
            )),
            "{error}"
        );
        assert!(error.contains("x.case"), "{error}");
        assert_eq!(calls.borrow().len(), 1);
        assert_eq!(written.text(), NEW_CASE, "{status} was written");
    }
}

#[test]
fn a_bad_request_reply_is_kept() {
    let written = Written::new(NEW_CASE);
    let (result, _) = record(&written, vec![reply(400, "{\"errors\": []}")]);
    result.unwrap_or_else(|e| panic!("{e}"));
    let case = written.case();
    assert_eq!(
        case.exchanges[0].response.served,
        Served::Reply {
            status: 400,
            content_type: Some("application/json".to_owned()),
            headers: Headers::default(),
        }
    );
}

#[test]
fn a_reply_that_is_not_json_is_kept_as_text() {
    let written = Written::new(NEW_CASE);
    let mut text = reply(400, "Bad Request");
    text.content_type = Some("text/plain".to_owned());
    let (result, _) = record(&written, vec![text]);
    result.unwrap_or_else(|e| panic!("{e}"));
    assert!(written.text().ends_with(&format!(
        "--- response 400 text/plain {STAMP}\nBad Request\n"
    )));
}

#[test]
fn a_reply_with_a_line_that_starts_a_section_is_refused() {
    let written = Written::new(NEW_CASE);
    let (result, _) = record(&written, vec![reply(400, "oops\n--- stdout\n")]);
    assert!(result.is_err());
    assert_eq!(written.text(), NEW_CASE);
}

#[test]
fn a_failed_connection_fails_and_writes_nothing() {
    let written = Written::new(NEW_CASE);
    let (result, _) = record(&written, vec![]);
    let error = result.err().unwrap_or_else(|| panic!("recorded"));
    assert!(error.ends_with("recording failed: no reply left"), "{error}");
    assert_eq!(written.text(), NEW_CASE);
}

const SOON: Duration = Duration::from_secs(5);

fn replay_of(written: &Written, replies: Vec<Reply>) -> (Replay, Calls) {
    let (recorder, calls) = recorder(replies);
    (Replay::recording(&written.case(), recorder), calls)
}

#[test]
fn a_raw_read_is_recorded_with_its_body_beside_the_case() {
    let written = Written::new("args: x\nexit: 0\n");
    let (replay, _) = replay_of(
        &written,
        vec![Reply {
            content_type: Some("text/plain".to_owned()),
            body: b"\xff\0bytes".to_vec(),
            ..reply(200, "")
        }],
    );
    replay
        .get("/r/x/-/raw/f", Instant::now() + SOON)
        .unwrap_or_else(|e| panic!("{e:?}"));
    write_recorded(&mut written.case(), &replay)
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        written.text(),
        format!(
            "args: x\nexit: 0\n--- request GET /r/x/-/raw/f\n--- response 200 \
             text/plain {STAMP}\n"
        )
    );
    assert_eq!(
        fs::read(written.path.with_extension("body"))
            .unwrap_or_else(|e| panic!("{e}")),
        b"\xff\0bytes"
    );
}

#[test]
fn nothing_is_written_when_a_later_request_is_refused() {
    let written = Written::new("args: x\nexit: 0\n");
    let (replay, calls) = replay_of(
        &written,
        vec![reply(200, SERVER_JSON), reply(502, "upstream says no")],
    );
    let body = json!({"operationName": "Op", "variables": {}, "query": "q"});
    replay
        .post(
            "/.api/graphql",
            body.to_string().as_bytes(),
            Instant::now() + SOON,
        )
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert!(replay.get("/r/x", Instant::now() + SOON).is_err());
    assert_eq!(calls.borrow().len(), 2);
    let error = write_recorded(&mut written.case(), &replay)
        .err()
        .unwrap_or_else(|| panic!("written"));
    assert!(error.contains("HTTP 502"), "{error}");
    assert_eq!(written.text(), "args: x\nexit: 0\n");
    assert!(!written.path.with_extension("body").exists());
}

#[test]
fn the_request_forwarded_is_the_one_run_sent() {
    let written = Written::new(NEW_CASE);
    let (replay, _) = replay_of(&written, vec![reply(200, SERVER_JSON)]);
    let body =
        json!({"operationName": "Op", "variables": {"a": 1}, "query": "q"});
    replay
        .post(
            "/.api/graphql",
            body.to_string().as_bytes(),
            Instant::now() + SOON,
        )
        .unwrap_or_else(|e| panic!("{e:?}"));
    let [(0, recorded)] = &replay.recorded()[..] else {
        panic!("{:?}", replay.recorded());
    };
    assert_eq!(
        recorded.exchange.request,
        Some(Request::Graphql {
            operation: "Op".to_owned(),
            variables: json!({"a": 1}),
            query: "q".to_owned(),
        })
    );
}

#[test]
fn dates_are_utc_and_count_leap_days() {
    let at = |days: u64| {
        date(UNIX_EPOCH + Duration::from_secs(days * 86_400 + 86_399))
    };
    assert_eq!(at(0), "1970-01-01");
    assert_eq!(at(10_956), "1999-12-31");
    assert_eq!(at(11_017), "2000-03-01");
    assert_eq!(at(19_782), "2024-02-29");
    assert_eq!(at(20_733), "2026-10-07");
}
