use std::time::{Duration, Instant};

use serde_json::{Map, Value, json};

use super::{Envelope, Seg, classify, exchange};
use crate::error::{Error, Kind, report};
use crate::upstream::script::{Script, reply};
use crate::upstream::{Reply, Request, TransportError};

const NOT_GRAPHQL: &str = "sourcegraph.com's answer isn't a GraphQL response";

const HOST: &str = "sourcegraph.com";

const JSON: Option<&str> = Some("application/json");

fn json_reply(status: u16, body: &Value) -> Reply {
    reply(status, JSON, &body.to_string())
}

fn rendered(error: &Error) -> String {
    let mut out = Vec::new();
    report(error, &mut out).unwrap_or_else(|e| panic!("{e}"));
    String::from_utf8(out).unwrap_or_else(|e| panic!("{e}"))
}

fn refused(reply: Reply) -> Error {
    match classify(reply, HOST) {
        Ok(envelope) => panic!("accepted {envelope:?}"),
        Err(error) => error,
    }
}

fn partial_reply() -> Reply {
    let failure = json!({
        "message": "git blob failed",
        "path": ["r0", "c0", "meta", "lfs"],
    });
    json_reply(
        200,
        &json!({
            "data": {"r0": {"c0": {"meta": {"lfs": null, "size": 3}}}},
            "errors": [
                failure,
                failure,
                {"message": "other", "path": ["r0", "c0", "meta", "lfs"]},
            ],
        }),
    )
}

fn partial() -> Envelope {
    classify(partial_reply(), HOST).unwrap_or_else(|e| panic!("{e}"))
}

type Row = (&'static str, Reply, Kind, String);

/// Replies refused by their status and headers.
fn refusal_rows() -> Vec<Row> {
    let challenge = "sourcegraph.com's edge refused the request (HTTP 403, \
                     Cloudflare challenge)";
    vec![
        (
            "challenge",
            Reply {
                cf_mitigated: Some("challenge".to_owned()),
                ..reply(
                    403,
                    Some("text/html; charset=UTF-8"),
                    "<html><title>Just a moment...</title></html>",
                )
            },
            Kind::Edge,
            challenge.into(),
        ),
        (
            "redirect",
            Reply {
                location: Some(
                    "https://sourcegraph.com/.api/graphql/".to_owned(),
                ),
                ..reply(301, None, "")
            },
            Kind::Upstream,
            "sourcegraph.com answered HTTP 301 with Location \
             https://sourcegraph.com/.api/graphql/"
                .into(),
        ),
        (
            "complexity",
            json_reply(
                400,
                &json!({"errors": [{
                    "message": "query complexity limit exceeded (cost 5001, \
                                limit 5000)",
                    "extensions": {"code": "ErrQueryComplexityLimitExceeded"},
                }]}),
            ),
            Kind::Upstream,
            "Sourcegraph rejected the request: query complexity limit \
             exceeded (cost 5001, limit 5000)"
                .into(),
        ),
        (
            "rate",
            json_reply(429, &json!({"error": "rate limited"})),
            Kind::Upstream,
            "sourcegraph.com answered HTTP 429".into(),
        ),
        (
            "bad-gateway",
            reply(502, Some("text/plain"), "Bad Gateway"),
            Kind::Upstream,
            "sourcegraph.com answered HTTP 502".into(),
        ),
        (
            "html-404",
            reply(
                404,
                Some("text/html; charset=utf-8"),
                "<html><body>Not found</body></html>",
            ),
            Kind::Upstream,
            "sourcegraph.com answered HTTP 404 text/html".into(),
        ),
        (
            "redirect-no-location",
            reply(302, None, ""),
            Kind::Upstream,
            "sourcegraph.com answered HTTP 302 with no Location".into(),
        ),
        (
            "html-403",
            reply(
                403,
                Some("Text/HTML; charset=utf-8"),
                "<html><body>Forbidden</body></html>",
            ),
            Kind::Edge,
            challenge.into(),
        ),
        (
            "challenge-text",
            Reply {
                cf_mitigated: Some("challenge".to_owned()),
                ..reply(403, Some("text/plain"), "Forbidden")
            },
            Kind::Edge,
            challenge.into(),
        ),
    ]
}

/// Replies refused by their content type or body.
fn body_rows() -> Vec<Row> {
    vec![
        (
            "text-plain-envelope",
            reply(200, Some("text/plain"), r#"{"data":{"r0":{"name":"x"}}}"#),
            Kind::Upstream,
            format!("{NOT_GRAPHQL}: not JSON"),
        ),
        (
            "no-content-type",
            reply(200, None, r#"{"data":{"r0":{"name":"x"}}}"#),
            Kind::Upstream,
            format!("{NOT_GRAPHQL}: not JSON"),
        ),
        (
            "null-no-errors",
            json_reply(200, &json!({"data": null, "errors": []})),
            Kind::Upstream,
            format!("{NOT_GRAPHQL}: empty"),
        ),
        (
            "neither",
            json_reply(200, &json!({"hello": "world"})),
            Kind::Upstream,
            format!("{NOT_GRAPHQL}: empty"),
        ),
        (
            "bad-request",
            json_reply(
                400,
                &json!({"errors": [{
                    "message": "Syntax Error",
                    "extensions": {"code": "ErrOther"},
                }]}),
            ),
            Kind::Upstream,
            "Sourcegraph rejected sego's request: Syntax Error".into(),
        ),
        (
            "no-message",
            json_reply(
                200,
                &json!({"data": null, "errors": [{"extensions": {"code": "x"}}]}),
            ),
            Kind::Upstream,
            "Sourcegraph rejected sego's request".into(),
        ),
        (
            "not-json",
            reply(200, Some("text/plain"), "this is not json"),
            Kind::Upstream,
            format!("{NOT_GRAPHQL}: not JSON"),
        ),
        (
            "truncated",
            reply(200, JSON, r#"{"data":{"r0":{"c0":{"meta":"#),
            Kind::Upstream,
            format!("{NOT_GRAPHQL}: truncated"),
        ),
        (
            "empty",
            json_reply(200, &json!({"data": {}})),
            Kind::Upstream,
            format!("{NOT_GRAPHQL}: empty"),
        ),
        (
            "pathless",
            json_reply(
                200,
                &json!({"data": null, "errors": [
                    {"message": "Unknown field \"nope\" on type \"Query\""},
                    {"message": "second"},
                ]}),
            ),
            Kind::Upstream,
            "Sourcegraph rejected sego's request: Unknown field \"nope\" on \
             type \"Query\""
                .into(),
        ),
    ]
}

#[test]
fn classification() {
    let rows = refusal_rows().into_iter().chain(body_rows());
    for (name, reply, kind, message) in rows {
        let error = refused(reply);
        assert_eq!(error.kind(), kind, "{name}");
        assert_eq!(error.to_string(), message, "{name}");
    }
    assert!(classify(partial_reply(), HOST).is_ok());
}

#[test]
fn messages_name_the_host_given() {
    let Err(error) =
        classify(reply(500, Some("text/plain"), "down"), "127.0.0.1:9")
    else {
        panic!("a 500 was accepted");
    };
    assert_eq!(error.to_string(), "127.0.0.1:9 answered HTTP 500");

    let envelope = classify(partial_reply(), "127.0.0.1:9")
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(envelope.host, "127.0.0.1:9");
}

#[test]
fn exchange_names_the_transports_host() {
    let request = Request {
        operation: "Op".to_owned(),
        document: "query Op { x }".to_owned(),
        variables: Map::new(),
    };
    let deadline = Instant::now() + Duration::from_secs(5);

    let script = Script::new(vec![Ok(reply(500, Some("text/plain"), "down"))])
        .named("h.example:7");
    let Err(error) = exchange(&script, &request, deadline) else {
        panic!("a 500 was accepted");
    };
    assert_eq!(error.to_string(), "h.example:7 answered HTTP 500");

    let script =
        Script::new(vec![Err(TransportError::Deadline)]).named("h.example:7");
    let Err(error) = exchange(&script, &request, deadline) else {
        panic!("a deadline was accepted");
    };
    assert_eq!(error.to_string(), "no answer from h.example:7 within 90 s");
}

#[test]
fn rate_limit_carries_retry_after() {
    let limited = Reply {
        retry_after: Some("30".to_owned()),
        ..json_reply(429, &json!({"error": "rate limited"}))
    };
    assert_eq!(
        rendered(&refused(limited)),
        "sego: error: sourcegraph.com answered HTTP 429\n  Retry-After: 30\n"
    );
    let gateway = reply(502, Some("text/plain"), "Bad Gateway");
    assert!(!rendered(&refused(gateway)).contains("Retry-After"));
}

#[test]
fn error_coverage() {
    let envelope = partial();
    assert!(envelope.get(&["r0", "c0", "meta"]).is_some());
    assert!(envelope.get(&["r0", "c0", "meta", "lfs"]).is_none());
    assert!(envelope.get(&["r0", "c0", "meta", "size"]).is_some());
    assert!(
        envelope.error_at(&["r0", "c0", "meta", "lfs", "byteSize"]).is_some()
    );
    assert!(envelope.error_at(&["r0", "c0", "meta"]).is_none());
    assert!(envelope.error_at(&["r0", "c0", "meta", "lfs"]).is_some());
    let messages: Vec<&str> =
        envelope.errors.iter().map(|e| e.message.as_str()).collect();
    assert_eq!(messages, ["git blob failed", "other"]);
    let path: Vec<Seg> = ["r0", "c0", "meta", "lfs"]
        .into_iter()
        .map(|k| Seg::Key(k.to_owned()))
        .collect();
    assert_eq!(envelope.errors[0].path, path);
}

#[test]
fn errors_beneath_a_path() {
    let envelope = partial();
    let beneath = |path: &[&str]| envelope.error_beneath(path).is_some();
    assert!(beneath(&["r0", "c0", "meta", "lfs"]));
    assert!(beneath(&["r0", "c0", "meta"]));
    assert!(beneath(&["r0"]));
    assert!(beneath(&[]));
    assert!(!beneath(&["r0", "c0", "meta", "lfs", "byteSize"]));
    assert!(!beneath(&["r0", "c0", "body"]));
    assert!(!beneath(&["r1"]));
    assert!(envelope.error_at(&["r0", "c0", "meta"]).is_none());
    let first = envelope.error_beneath(&["r0", "c0", "meta"]);
    assert_eq!(first.map(|e| e.message.as_str()), Some("git blob failed"));
}

#[test]
fn odd_paths_stay_located() {
    let odd = json_reply(
        200,
        &json!({
            "data": {"r0": {"name": "x"}},
            "errors": [
                {"message": "null element", "path": [null]},
                {"message": "negative index", "path": ["r0", -1, "x"]},
            ],
        }),
    );
    let envelope = classify(odd, HOST).unwrap_or_else(|e| panic!("{e}"));
    let key = |k: &str| Seg::Key(k.to_owned());
    assert_eq!(envelope.errors.len(), 2);
    assert_eq!(envelope.errors[0].path, [Seg::Invalid]);
    assert_eq!(envelope.errors[1].path, [key("r0"), Seg::Invalid, key("x")]);
    assert!(envelope.error_at(&["r0", "x"]).is_none());
    assert!(envelope.get(&["r0", "name"]).is_some());
}
