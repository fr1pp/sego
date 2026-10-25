use std::cell::Cell;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::address;
use crate::error::report;
use crate::output::{Answer, Notes};
use crate::upstream::{Reply, Transport, TransportError};

use super::run;

const REPO: &str = "example.com/acme/widget";
const OID_A: &str = "4ad40c3a28bd2acabbb50eb3ff63b08c2c55f96c";
const OID_B: &str = "1e23553eec2392753c5420d348e63a63f517cd45";
const A12: &str = "4ad40c3a28bd";
const B12: &str = "1e23553eec23";

/// Answers the one GraphQL request with `reply`.
struct Server {
    reply: Cell<Option<Reply>>,
}

impl Transport for Server {
    fn post(
        &self,
        _: &str,
        _: &[u8],
        _: Instant,
    ) -> Result<Reply, TransportError> {
        Ok(self.reply.take().expect("one GraphQL request"))
    }

    fn get(&self, _: &str, _: Instant) -> Result<Reply, TransportError> {
        Err(TransportError::Network("no raw route"))
    }
}

/// What `sego log ADDRESS [PATH]` makes of `envelope`: its text, or the error
/// as written to stderr.
fn log(
    address: &str,
    path: Option<&str>,
    envelope: &Value,
) -> Result<String, String> {
    let server = Server {
        reply: Cell::new(Some(Reply {
            status: 200,
            content_type: Some("application/json".to_owned()),
            location: None,
            retry_after: None,
            cf_mitigated: None,
            body: envelope.to_string().into_bytes(),
        })),
    };
    let address = address::parse(address).expect("a well-formed address");
    let deadline = Instant::now() + Duration::from_secs(90);
    match run(&address, path, false, &server, deadline, &mut Notes::new()) {
        Ok(logged) => Ok(logged.text()),
        Err(error) => {
            let mut written = Vec::new();
            report(&error, &mut written).expect("a vector takes a write");
            Err(String::from_utf8_lossy(&written).into_owned())
        }
    }
}

fn failure(address: &str, path: Option<&str>, envelope: &Value) -> String {
    log(address, path, envelope).expect_err("the log fails")
}

fn answer(address: &str, path: Option<&str>, envelope: &Value) -> String {
    log(address, path, envelope).unwrap_or_else(|written| panic!("{written}"))
}

fn error_line(message: &str) -> String {
    format!("sego: error: {message}\n")
}

fn root(fields: &[(&str, Value)]) -> Value {
    let mut root = json!({
        "name": REPO,
        "defaultBranch": {"displayName": "main"},
    });
    for (key, value) in fields {
        root[*key] = value.clone();
    }
    root
}

fn oid(n: usize) -> String {
    format!("{n:040x}")
}

fn node(n: usize) -> Value {
    json!({
        "oid": oid(n),
        "subject": format!("change {n}"),
        "author": {
            "date": "2026-09-14T10:19:56Z",
            "person": {"displayName": "Ada Example"},
        },
        "parents": [{"oid": oid(n + 1)}],
    })
}

fn nodes(count: usize) -> Vec<Value> {
    (1..=count).map(node).collect()
}

fn page(nodes: &[Value]) -> Value {
    json!({"nodes": nodes, "pageInfo": {"hasNextPage": false}})
}

fn no_tags() -> Value {
    root(&[("gitRefs", json!({"totalCount": 0, "nodes": []}))])
}

fn rev_body(nodes: &[Value]) -> Value {
    json!({"data": {
        "r0": root(&[("commit", json!({
            "oid": oid(1),
            "ancestors": page(nodes),
        }))]),
        "t0": no_tags(),
    }})
}

fn range_body(ahead: Option<u64>, nodes: &[Value]) -> Value {
    let mut data = json!({
        "a0": root(&[("commit", json!({"oid": OID_A}))]),
        "b0": root(&[("commit", json!({"oid": OID_B}))]),
        "c0": root(&[("comparison", json!({"commits": page(nodes)}))]),
        "t0": no_tags(),
    });
    if let Some(ahead) = ahead {
        data["n0"] =
            root(&[("commit", json!({"behindAhead": {"ahead": ahead}}))]);
    }
    json!({"data": data})
}

fn range_address() -> String {
    format!("{REPO}@v1..v2")
}

#[test]
fn a_range_with_a_missing_side_names_the_revisions_not_found() {
    let both_gone = |a: bool, b: bool| {
        let mut body = range_body(Some(0), &[]);
        if !a {
            body["data"]["a0"]["commit"] = Value::Null;
        }
        if !b {
            body["data"]["b0"]["commit"] = Value::Null;
        }
        body["data"]["c0"] = Value::Null;
        body["data"]["n0"]["commit"] = Value::Null;
        let named = if b { "v1" } else { "v2" };
        let mut errors = vec![json!({
            "message": format!("revision not found: repo 1700 @{named}^0"),
            "path": ["c0", "comparison"],
        })];
        if !a && b {
            errors.push(json!({
                "message": "revision not found",
                "path": ["n0", "commit", "behindAhead"],
            }));
        }
        body["errors"] = Value::Array(errors);
        failure(&range_address(), None, &body)
    };
    assert_eq!(
        both_gone(false, true),
        error_line(&format!("base revision v1 not found in {REPO}"))
    );
    assert_eq!(
        both_gone(true, false),
        error_line(&format!("head revision v2 not found in {REPO}"))
    );
    assert_eq!(
        both_gone(false, false),
        error_line(&format!("revisions v1 and v2 not found in {REPO}"))
    );
}

#[test]
fn a_missing_revision_is_named_and_a_missing_head_is_named_head() {
    let mut body = rev_body(&[]);
    body["data"]["r0"]["commit"] = Value::Null;
    assert_eq!(
        failure(&format!("{REPO}@v9"), None, &body),
        error_line(&format!("revision v9 not found in {REPO}"))
    );
    assert_eq!(
        failure(REPO, None, &body),
        error_line(&format!("revision HEAD not found in {REPO}"))
    );
}

#[test]
fn a_plain_path_with_no_commits_at_a_revision_is_not_found() {
    let body = rev_body(&[]);
    assert_eq!(
        failure(&format!("{REPO}@v1"), Some("nope.go"), &body),
        error_line(&format!("nope.go not found in {REPO}@{}", &oid(1)[..12]))
    );
}

#[test]
fn a_glob_path_with_no_commits_is_an_empty_answer() {
    let body = rev_body(&[]);
    for path in ["*.nope", ":!nope"] {
        let text = answer(&format!("{REPO}@v1"), Some(path), &body);
        assert!(text.ends_with("0 commits  rev v1\n"), "{path}: {text}");
    }
}

#[test]
fn a_plain_path_in_a_range_is_not_found_only_where_it_exists_at_neither_end() {
    let found = |at_a: bool, at_b: bool| {
        let mut body = range_body(None, &[]);
        let blob = json!({"__typename": "GitBlob"});
        body["data"]["a0"]["commit"]["x"] =
            if at_a { blob.clone() } else { Value::Null };
        body["data"]["b0"]["commit"]["x"] =
            if at_b { blob } else { Value::Null };
        log(&range_address(), Some("nope.go"), &body)
    };
    assert_eq!(
        found(false, false),
        Err(error_line(&format!(
            "nope.go not found in {REPO}@{A12} or @{B12}"
        )))
    );
    for (at_a, at_b) in [(true, false), (false, true), (true, true)] {
        let text = found(at_a, at_b).unwrap_or_else(|e| panic!("{e}"));
        assert!(text.contains("  0 commits  "), "{at_a} {at_b}: {text}");
    }
}

#[test]
fn a_failed_comparison_beats_a_path_not_found() {
    let mut body = range_body(None, &[]);
    body["data"]["c0"] = Value::Null;
    body["errors"] = json!([{"message": "x", "path": ["c0", "comparison"]}]);
    for alias in ["a0", "b0"] {
        body["data"][alias]["commit"]["x"] = Value::Null;
    }
    assert_eq!(
        failure(&range_address(), Some("nope.go"), &body),
        error_line("Sourcegraph failed to answer the comparison")
    );
}

#[test]
fn a_path_not_found_beats_a_failed_tag_list() {
    let mut body = range_body(None, &[]);
    body["errors"] = json!([{"message": "x", "path": ["t0", "gitRefs"]}]);
    body["data"]["t0"]["gitRefs"] = Value::Null;
    for alias in ["a0", "b0"] {
        body["data"][alias]["commit"]["x"] = Value::Null;
    }
    assert_eq!(
        failure(&range_address(), Some("nope.go"), &body),
        error_line(&format!("nope.go not found in {REPO}@{A12} or @{B12}"))
    );
}

#[test]
fn a_failed_commit_list_hides_the_server_text() {
    let mut body = rev_body(&nodes(1));
    body["errors"] = json!([{
        "message": "git command [git log --secret] failed",
        "path": ["r0", "commit", "ancestors", "nodes"],
    }]);
    body["data"]["r0"]["commit"] = Value::Null;
    let written = failure(&format!("{REPO}@v1:src"), None, &body);
    assert_eq!(
        written,
        error_line("Sourcegraph failed to answer the commit list")
    );
}

#[test]
fn a_failed_revision_lookup_is_named_whichever_root_it_was_at() {
    let mut commit = rev_body(&nodes(1));
    commit["errors"] = json!([{"message": "x", "path": ["r0", "commit"]}]);
    commit["data"]["r0"]["commit"] = Value::Null;
    assert_eq!(
        failure(&format!("{REPO}@v1"), None, &commit),
        error_line("Sourcegraph failed to answer the revision lookup")
    );

    for (path, side) in
        [(json!(["a0", "commit"]), "a0"), (json!(["b0", "commit", "x"]), "b0")]
    {
        let mut body = range_body(None, &nodes(1));
        body["errors"] = json!([{"message": "x", "path": path}]);
        if side == "a0" {
            body["data"]["a0"]["commit"] = Value::Null;
        } else {
            body["data"]["b0"]["commit"]["x"] = Value::Null;
        }
        assert_eq!(
            failure(&range_address(), Some("a.go"), &body),
            error_line("Sourcegraph failed to answer the revision lookup"),
            "{side}"
        );
    }
}

#[test]
fn a_failed_part_of_a_range_is_named_and_hides_the_server_text() {
    let cases = [
        ("c0 root", json!(["c0"]), "c0", "the comparison"),
        (
            "commits",
            json!(["c0", "comparison", "commits", "nodes"]),
            "c0",
            "the comparison",
        ),
        (
            "ahead",
            json!(["n0", "commit", "behindAhead"]),
            "n0",
            "the commit count",
        ),
        ("n0 root", json!(["n0"]), "n0", "the commit count"),
        ("tags", json!(["t0", "gitRefs"]), "t0", "the tag list"),
        ("t0 root", json!(["t0"]), "t0", "the tag list"),
    ];
    for (name, path, alias, part) in cases {
        let mut body = range_body(Some(1), &nodes(1));
        body["errors"] = json!([{"message": "secret internal", "path": path}]);
        if path.as_array().map(Vec::len) == Some(1) {
            body["data"][alias] = Value::Null;
        }
        assert_eq!(
            failure(&range_address(), None, &body),
            error_line(&format!("Sourcegraph failed to answer {part}")),
            "{name}"
        );
    }
}

#[test]
fn a_failed_tag_list_fails_a_revision() {
    let mut body = rev_body(&nodes(1));
    body["errors"] = json!([{"message": "secret", "path": ["t0", "gitRefs"]}]);
    body["data"]["t0"]["gitRefs"] = Value::Null;
    assert_eq!(
        failure(&format!("{REPO}@v1"), None, &body),
        error_line("Sourcegraph failed to answer the tag list")
    );
}

#[test]
fn a_range_without_a_commit_count_fails() {
    let mut body = range_body(None, &nodes(1));
    body["data"]["n0"] = Value::Null;
    assert_eq!(
        failure(&range_address(), None, &body),
        error_line("Sourcegraph failed to answer the commit count")
    );
}
