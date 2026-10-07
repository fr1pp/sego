use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use sego::internal::upstream::{
    Endpoint, GRAPHQL_PATH, Reply, Transport, TransportError, Ureq,
};

use crate::support::fixture::{Fault, Fixture, load_all};
use crate::support::http::{Response, Server};
use crate::support::replay::{Mode, Replay};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("fixtures-{name}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("{e}"));
    dir
}

fn client(url: &str) -> Ureq {
    let endpoint = Endpoint::from_env(Some(OsStr::new(url)))
        .unwrap_or_else(|e| panic!("{e}"));
    Ureq::without_proxy(endpoint)
}

fn soon() -> Instant {
    Instant::now() + Duration::from_secs(30)
}

fn graphql(
    client: &Ureq,
    operation: &str,
    variables: &Value,
    query: &str,
) -> Result<Reply, TransportError> {
    let body = json!({
        "query": query,
        "operationName": operation,
        "variables": variables,
    });
    client.post(GRAPHQL_PATH, body.to_string().as_bytes(), soon())
}

fn lines(bytes: &[u8]) -> usize {
    bytes.split(|&b| b == b'\n').count() - 1
}

fn text(reply: &Reply) -> String {
    String::from_utf8_lossy(&reply.body).into_owned()
}

const ECHO: &str = "query SelfTest($x: Int!) { echo(x: $x) }";

fn find(fixtures: &[Fixture], name: &str) -> Fixture {
    fixtures
        .iter()
        .find(|f| f.file.ends_with(name))
        .unwrap_or_else(|| panic!("no fixture {name}"))
        .clone()
}

fn fault(
    n: u32,
    fault: Fault,
    status: u16,
    headers: &[(&str, &str)],
) -> Fixture {
    Fixture {
        operation: Some("Fault".to_owned()),
        variables: Some(json!({ "n": n })),
        fault: Some(fault),
        status,
        headers: headers
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect(),
        ..Fixture::default()
    }
}

#[test]
fn loads_every_fixture() {
    let fixtures = load_all(&root());
    for name in ["echo.yaml", "big.yaml", "raw.yaml", "stale.yaml"] {
        let fixture = find(&fixtures, &format!("selftest/{name}"));
        assert!(fixture.file.is_file(), "{name}");
        assert!(fixture.handwritten, "{name}");
    }
    let echo = find(&fixtures, "selftest/echo.yaml");
    assert_eq!(echo.operation.as_deref(), Some("SelfTest"));
    assert_eq!(echo.variables, Some(json!({ "x": 1 })));
    assert_eq!(echo.status, 200);
}

#[test]
fn large_fixture_loads() {
    let big = find(&load_all(&root()), "selftest/big.yaml");
    let body = big.body();
    assert_eq!(lines(&body), 300_000);

    let dir = scratch("large");
    let items = "    - x\n".repeat(300_000);
    fs::write(
        dir.join("many.yaml"),
        format!(
            "operation: Many\nvariables: {{}}\nresponse:\n  data:\n{items}"
        ),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    let loaded = load_all(&dir);
    let Some(Value::Object(response)) = &loaded[0].response else {
        panic!("{:?}", loaded[0].response);
    };
    assert_eq!(response["data"].as_array().map(Vec::len), Some(300_000));
}

#[test]
fn replays_by_operation_and_variables() {
    let fake = Replay::start(load_all(&root()), Mode::Replay);
    let client = client(&fake.url());
    let reply = graphql(&client, "SelfTest", &json!({ "x": 1 }), ECHO)
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(reply.status, 200);
    assert_eq!(reply.content_type.as_deref(), Some("application/json"));
    let body: Value =
        serde_json::from_slice(&reply.body).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(body, json!({ "data": { "echo": 1 } }));
    assert_eq!(fake.take_requests(), 1);
    assert_eq!(fake.take_requests(), 0);
}

#[test]
fn query_whitespace_is_not_a_change() {
    let fake = Replay::start(load_all(&root()), Mode::Replay);
    let reply = graphql(
        &client(&fake.url()),
        "SelfTest",
        &json!({ "x": 1 }),
        "  query SelfTest($x: Int!) {\n\n\techo(x: $x)\n}  ",
    )
    .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(reply.status, 200);
}

#[test]
fn raw_get_replays() {
    let fake = Replay::start(load_all(&root()), Mode::Replay);
    let client = client(&fake.url());
    let reply = client.get("/r/x", soon()).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(reply.status, 200);
    assert_eq!(reply.content_type.as_deref(), Some("text/plain"));
    assert_eq!(text(&reply), "raw body\n");
    let big = client.get("/r/big", soon()).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(lines(&big.body), 300_000);
}

#[test]
fn stale_query_fails_with_both_texts() {
    let fixtures = load_all(&root());
    let file = find(&fixtures, "selftest/stale.yaml").file;
    let fake = Replay::start(fixtures, Mode::Replay);
    let reply = graphql(
        &client(&fake.url()),
        "SelfTestStale",
        &json!({ "x": 2 }),
        "query SelfTestStale($x: Int!) { stored(x: $x) other }",
    )
    .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(reply.status, 500);
    assert_eq!(
        text(&reply),
        format!(
            "stale fixture {}: query changed\n--- fixture\nquery \
             SelfTestStale($x: Int!) {{ stored(x: $x) }}\n--- sent\nquery \
             SelfTestStale($x: Int!) {{ stored(x: $x) other }}",
            file.display()
        )
    );
}

#[test]
fn unknown_request_lists_candidates() {
    let fixtures = load_all(&root());
    let file = find(&fixtures, "selftest/echo.yaml").file;
    let fake = Replay::start(fixtures, Mode::Replay);
    let client = client(&fake.url());

    let reply = graphql(&client, "SelfTest", &json!({ "x": 3 }), ECHO)
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(reply.status, 500);
    let message = text(&reply);
    assert!(
        message.starts_with(r#"no fixture for SelfTest {"x":3}"#),
        "{message}"
    );
    assert!(message.contains(&file.display().to_string()), "{message}");
    assert!(message.contains(r#"{"x":1}"#), "{message}");

    let reply = graphql(&client, "Nope", &json!({}), "query Nope { a }")
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(reply.status, 500);
    assert!(text(&reply).starts_with("no fixture for Nope {}"));

    let reply =
        client.get("/r/missing", soon()).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(reply.status, 500);
    assert!(text(&reply).starts_with("no fixture for GET /r/missing"));
}

#[test]
fn faults() {
    let fake = Replay::start(
        vec![
            fault(1, Fault::Network, 200, &[]),
            fault(2, Fault::HtmlChallenge, 200, &[]),
            fault(3, Fault::Redirect, 200, &[("location", "/elsewhere")]),
            fault(4, Fault::Status, 503, &[("retry-after", "7")]),
        ],
        Mode::Replay,
    );
    let client = client(&fake.url());
    let send = |n: u32| graphql(&client, "Fault", &json!({ "n": n }), "q");

    assert!(matches!(send(1), Err(TransportError::Network(_))));

    let challenge = send(2).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(challenge.status, 429);
    assert_eq!(challenge.content_type.as_deref(), Some("text/html"));
    assert_eq!(challenge.cf_mitigated.as_deref(), Some("challenge"));
    assert_eq!(challenge.body.len(), 5 * 1024);

    let redirect = send(3).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(redirect.status, 302);
    assert_eq!(redirect.location.as_deref(), Some("/elsewhere"));

    let status = send(4).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(status.status, 503);
    assert_eq!(status.retry_after.as_deref(), Some("7"));
    assert_eq!(status.body, b"");
}

#[test]
fn write_round_trips() {
    let file = PathBuf::from("round.yaml");
    for (content, block) in [("a\nb\n", true), ("a\r\nb\r\n", false)] {
        let fixture = Fixture {
            operation: Some("Round".to_owned()),
            variables: Some(json!({ "a": [1, "two"] })),
            query: Some("query Round {\n  a\n}\n".to_owned()),
            headers: [(
                "content-type".to_owned(),
                "application/json".to_owned(),
            )]
            .into(),
            pinned: true,
            response: Some(json!({ "data": { "content": content } })),
            file: file.clone(),
            ..Fixture::default()
        };
        let yaml = fixture.to_yaml();
        assert_eq!(yaml.contains("content: |"), block, "{yaml}");
        assert!(yaml.contains("query: |"), "{yaml}");
        if !block {
            assert!(yaml.contains(r"\r\n"), "{yaml}");
        }
        assert_eq!(Fixture::parse(&yaml, &file), Ok(fixture), "{yaml}");
    }
}

#[test]
fn misspelt_keys_are_an_error() {
    let result =
        Fixture::parse("operation: A\nvaraibles: {}\n", Path::new("a.yaml"));
    assert!(result.is_err());
}

fn upstream() -> Server {
    Server::start(|request| {
        if request.path == "/r/big" {
            return Response::new(200)
                .header("content-type", "text/plain")
                .filler(b'a', 1_000_001);
        }
        Response::new(200).header("content-type", "application/json").body(
            json!({
                "data": { "x": 1, "list": [{ "requestId": "r", "k": 2 }] },
                "extensions": { "trace": "t" },
            })
            .to_string(),
        )
    })
}

fn recording(mode: Mode, root: &Path, upstream: &Server) -> Replay {
    Replay::start_recording(
        Vec::new(),
        mode,
        root.to_path_buf(),
        Box::new(client(&upstream.url())),
    )
}

#[test]
fn recording_writes_and_then_serves() {
    let dir = scratch("record");
    let upstream = upstream();
    let fake = recording(Mode::RecordMissing, &dir, &upstream);
    let client = client(&fake.url());
    let variables = json!({
        "repo0": "github.com/Gorilla/mux",
        "rev0": "v1.8.0",
        "path_0": "mux.go",
    });
    let expected = json!({ "data": { "x": 1, "list": [{ "k": 2 }] } });

    let reply = graphql(&client, "Rec", &variables, "query Rec { x }")
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(reply.status, 200);
    let body: Value =
        serde_json::from_slice(&reply.body).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(body, expected);
    let name = "github_com_gorilla_mux--v1_8_0--mux_go";
    let file = dir.join("Rec").join(format!("{name}.yaml"));
    let written = Fixture::parse(
        &fs::read_to_string(&file).unwrap_or_else(|e| panic!("{e}")),
        &file,
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(written.operation.as_deref(), Some("Rec"));
    assert_eq!(written.query.as_deref(), Some("query Rec { x }"));
    assert_eq!(written.variables, Some(variables.clone()));
    assert_eq!(written.response, Some(expected.clone()));
    assert!(!written.handwritten);
    assert_eq!(upstream.requests().len(), 1);
    assert_eq!(upstream.requests()[0].path, GRAPHQL_PATH);

    graphql(&client, "Rec", &variables, "query Rec { x }")
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(upstream.requests().len(), 1, "second hit is replayed");

    let mut clash = variables.clone();
    clash["other"] = json!(true);
    graphql(&client, "Rec", &clash, "query Rec { x }")
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert!(dir.join("Rec").join(format!("{name}-2.yaml")).is_file());
    assert_eq!(load_all(&dir).len(), 2);
}

#[test]
fn recording_stores_a_large_body_beside_the_fixture() {
    let dir = scratch("large-body");
    let upstream = upstream();
    let fake = recording(Mode::RecordMissing, &dir, &upstream);
    let reply = client(&fake.url())
        .get("/r/big", soon())
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(reply.body.len(), 1_000_001);
    let body = dir.join("raw/r_big.body");
    assert_eq!(
        fs::metadata(&body).unwrap_or_else(|e| panic!("{e}")).len(),
        1_000_001
    );
    let loaded = load_all(&dir);
    assert_eq!(loaded[0].body_file.as_deref(), Some("r_big.body"));
    assert_eq!(loaded[0].body().len(), 1_000_001);
}

#[test]
fn replay_mode_does_not_forward() {
    let dir = scratch("no-forward");
    let upstream = upstream();
    let fake = recording(Mode::Replay, &dir, &upstream);
    let reply = graphql(&client(&fake.url()), "Rec", &json!({}), "q")
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(reply.status, 500);
    assert!(text(&reply).starts_with("no fixture for Rec {}"));
    assert!(upstream.requests().is_empty());
}

#[test]
fn refresh_forwards_every_request_and_rewrites_the_file() {
    let dir = scratch("refresh");
    let upstream = upstream();
    let stored = Fixture {
        operation: Some("Rec".to_owned()),
        variables: Some(json!({})),
        pinned: true,
        response: Some(json!({ "old": true })),
        file: dir.join("Rec/old.yaml"),
        ..Fixture::default()
    };
    fs::create_dir_all(dir.join("Rec")).unwrap_or_else(|e| panic!("{e}"));
    fs::write(&stored.file, stored.to_yaml())
        .unwrap_or_else(|e| panic!("{e}"));
    let fake = Replay::start_recording(
        vec![stored],
        Mode::Refresh,
        dir.clone(),
        Box::new(client(&upstream.url())),
    );
    let client = client(&fake.url());
    for _ in 0..2 {
        graphql(&client, "Rec", &json!({}), "query Rec { x }")
            .unwrap_or_else(|e| panic!("{e:?}"));
    }
    assert_eq!(upstream.requests().len(), 2);
    let loaded = load_all(&dir);
    assert_eq!(loaded.len(), 1);
    assert!(loaded[0].file.ends_with("Rec/old.yaml"));
    assert!(loaded[0].pinned);
    assert_eq!(
        loaded[0].response.as_ref().map(|r| r["data"]["x"].clone()),
        Some(json!(1))
    );
}

fn failing_upstream(
    status: u16,
    cf_mitigated: Option<&'static str>,
) -> Server {
    Server::start(move |_| {
        let mut response = Response::new(status).body("upstream says no");
        if let Some(value) = cf_mitigated {
            response = response.header("cf-mitigated", value);
        }
        response
    })
}

#[test]
fn recording_refuses_error_replies() {
    let cases = [
        (502, None, "recording failed: upstream answered HTTP 502"),
        (302, None, "recording failed: upstream answered HTTP 302"),
        (429, None, "recording failed: upstream answered HTTP 429"),
        (
            403,
            Some("challenge"),
            "recording failed: upstream answered HTTP 403 (cf-mitigated: \
             challenge)",
        ),
    ];
    for (status, mitigated, expected) in cases {
        let dir = scratch(&format!("refuse-{status}"));
        let upstream = failing_upstream(status, mitigated);
        let fake = recording(Mode::RecordMissing, &dir, &upstream);
        let reply = graphql(
            &client(&fake.url()),
            "Rec",
            &json!({}),
            "query Rec { x }",
        )
        .unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(reply.status, 500, "{status}");
        assert_eq!(text(&reply), expected);
        assert_eq!(upstream.requests().len(), 1);
        assert!(load_all(&dir).is_empty(), "{status} was written");
    }
}

#[test]
fn recording_keeps_a_bad_request_reply() {
    let dir = scratch("keep-400");
    let upstream = failing_upstream(400, None);
    let fake = recording(Mode::RecordMissing, &dir, &upstream);
    let reply =
        graphql(&client(&fake.url()), "Rec", &json!({}), "query Rec { x }")
            .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(reply.status, 400);
    assert_eq!(load_all(&dir).len(), 1);
}
