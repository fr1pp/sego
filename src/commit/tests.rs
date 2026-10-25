use std::cell::Cell;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::address;
use crate::error::report;
use crate::output::Notes;
use crate::upstream::{Reply, Transport, TransportError};

use super::{Committed, run};

const REPO: &str = "example.com/acme/widget";
const OID: &str = "affe2b9b7c608f4e354d7e83d0583ed35e845650";
const PARENT: &str = "6b668aa526afdc623c1a9ed1d6ae920e04a717ad";
const SECRET: &str = "git command [git show --secret] failed";

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

fn commit() -> Value {
    json!({
        "oid": OID,
        "subject": "m",
        "message": "m",
        "author": {
            "date": "2026-09-09T20:14:26Z",
            "person": {"displayName": "Jonathan Davies"},
        },
        "committer": {
            "date": "2026-09-10T01:02:03Z",
            "person": {"displayName": "GitHub"},
        },
        "parents": [{"oid": PARENT}],
    })
}

fn comparison() -> Value {
    json!({
        "all": {"totalCount": 1, "diffStat": {"added": 1, "deleted": 1}},
        "page": {"nodes": [{
            "oldPath": "a.go",
            "newPath": "a.go",
            "stat": {"added": 1, "deleted": 1},
        }]},
    })
}

fn envelope() -> Value {
    json!({"data": {
        "m0": {
            "name": REPO,
            "defaultBranch": {"displayName": "main"},
            "commit": commit(),
        },
        "d0": {
            "name": REPO,
            "defaultBranch": {"displayName": "main"},
            "comparison": comparison(),
        },
    }})
}

fn errors(path: &Value) -> Value {
    json!([{"message": SECRET, "path": path}])
}

fn commit_mut(envelope: &mut Value) -> &mut Value {
    &mut envelope["data"]["m0"]["commit"]
}

fn committed(envelope: &Value) -> Result<Committed, String> {
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
    let address =
        address::parse(&format!("{REPO}@v1")).expect("a well-formed address");
    let deadline = Instant::now() + Duration::from_secs(90);
    run(&address, &server, deadline, &mut Notes::new()).map_err(|error| {
        let mut written = Vec::new();
        report(&error, &mut written).expect("a vector takes a write");
        String::from_utf8_lossy(&written).into_owned()
    })
}

fn failed(envelope: &Value, part: &str) {
    let written = committed(envelope).err().expect("the commit fails");
    assert_eq!(
        written,
        format!("sego: error: Sourcegraph failed to answer {part}\n")
    );
}

#[test]
fn an_error_at_the_revision_hides_the_server_text() {
    for path in [
        json!(["m0", "commit"]),
        json!(["m0", "commit", "message"]),
        json!(["m0", "commit", "author", "date"]),
        json!(["m0", "commit", "parents"]),
    ] {
        let mut body = envelope();
        body["errors"] = errors(&path);
        if path.as_array().map(Vec::len) == Some(2) {
            *commit_mut(&mut body) = Value::Null;
        }
        failed(&body, "the revision lookup");
    }
}

#[test]
fn an_error_at_the_revision_outranks_one_at_the_file_list() {
    let mut body = envelope();
    *commit_mut(&mut body) = Value::Null;
    body["data"]["d0"] = Value::Null;
    body["errors"] = json!([
        {"message": "x", "path": ["d0", "comparison"]},
        {"message": SECRET, "path": ["m0", "commit"]},
    ]);
    failed(&body, "the revision lookup");
}

#[test]
fn an_error_in_the_file_list_hides_the_server_text() {
    for (path, nulls) in [
        (json!(["d0"]), true),
        (json!(["d0", "comparison"]), true),
        (json!(["d0", "comparison", "page", "nodes"]), false),
        (json!(["d0", "comparison", "all", "totalCount"]), false),
    ] {
        let mut body = envelope();
        body["errors"] = errors(&path);
        if nulls {
            body["data"]["d0"] = Value::Null;
        }
        failed(&body, "the file list");
    }
}

#[test]
fn the_author_is_required() {
    for path in [
        &["author"][..],
        &["author", "date"],
        &["author", "person"],
        &["author", "person", "displayName"],
    ] {
        let mut body = envelope();
        let mut at = commit_mut(&mut body);
        for key in path {
            at = &mut at[*key];
        }
        *at = Value::Null;
        failed(&body, "the revision lookup");
    }
}

#[test]
fn the_committer_is_optional() {
    let mut body = envelope();
    commit_mut(&mut body)["committer"] = Value::Null;
    let committed = committed(&body).expect("the commit reads");
    assert_eq!(committed.committer, None);
    assert_eq!(committed.committer_date, None);
}
