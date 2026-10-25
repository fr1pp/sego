use std::cell::Cell;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::address;
use crate::error::report;
use crate::output::Notes;
use crate::upstream::{Reply, Transport, TransportError};

use super::run;

const REPO: &str = "example.com/acme/widget";
const OID_A: &str = "0123456789abcdef0123456789abcdef01234567";
const OID_B: &str = "89abcdef0123456789abcdef0123456789abcdef";
const SECRET: &str = "internal failure in /srv/secret-host";

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

fn root(key: &str, value: Value) -> Value {
    let mut root = json!({
        "name": REPO,
        "defaultBranch": {"displayName": "main"},
    });
    root[key] = value;
    root
}

fn answer() -> Value {
    json!({"data": {
        "a0": root("commit", json!({"oid": OID_A})),
        "b0": root(
            "commit",
            json!({"oid": OID_B, "parents": [{"oid": OID_A}]}),
        ),
        "c0": root("comparison", json!({
            "all": {"totalCount": 1, "diffStat": {"added": 1, "deleted": 1}},
            "page": {"nodes": [{
                "oldPath": "a.go",
                "newPath": "a.go",
                "stat": {"added": 1, "deleted": 1},
                "oldFile": {"commit": {"oid": OID_A}},
            }]},
        })),
    }})
}

/// What `sego diff example.com/acme/widget@v1...v2 PATHS` writes to stderr as
/// an error on `envelope`.
fn failure(envelope: &Value, paths: &[&str]) -> String {
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
    let address = address::parse(&format!("{REPO}@v1...v2"))
        .expect("a well-formed address");
    let paths: Vec<String> = paths.iter().map(|p| (*p).to_owned()).collect();
    let deadline = Instant::now() + Duration::from_secs(90);
    let error =
        run(&address, &paths, false, &server, deadline, &mut Notes::new())
            .err()
            .expect("the diff fails");
    let mut written = Vec::new();
    report(&error, &mut written).expect("a vector takes a write");
    String::from_utf8_lossy(&written).into_owned()
}

#[test]
fn a_failed_revision_lookup_hides_the_server_text() {
    let mut commit = answer();
    commit["data"]["a0"]["commit"] = Value::Null;
    commit["errors"] = json!([{"message": SECRET, "path": ["a0", "commit"]}]);

    let mut probe = answer();
    probe["data"]["b0"]["commit"]["p0"] = Value::Null;
    probe["errors"] = json!([
        {"message": SECRET, "path": ["c0", "comparison"]},
        {"message": SECRET, "path": ["b0", "commit", "p0"]},
    ]);

    for (envelope, paths) in [(commit, &[][..]), (probe, &["a.go"][..])] {
        assert_eq!(
            failure(&envelope, paths),
            "sego: error: Sourcegraph failed to answer the revision lookup\n"
        );
    }
}

#[test]
fn a_failed_comparison_hides_the_server_text() {
    let at = [
        (json!(["c0"]), true),
        (json!(["c0", "comparison"]), true),
        (json!(["c0", "comparison", "page", "nodes"]), false),
        (json!(["c0", "comparison", "all", "totalCount"]), false),
    ];
    for (path, null_root) in at {
        let mut envelope = answer();
        envelope["errors"] = json!([{"message": SECRET, "path": path}]);
        if null_root {
            envelope["data"]["c0"] = Value::Null;
        }
        assert_eq!(
            failure(&envelope, &[]),
            "sego: error: Sourcegraph failed to answer the comparison\n",
            "{path}"
        );
    }
}
