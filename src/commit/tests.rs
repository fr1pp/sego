use std::time::Instant;

use serde_json::{Value, json};

use crate::address;
use crate::error::reported;
use crate::output::Notes;
use crate::upstream::DEADLINE;
use crate::upstream::script::Script;

use super::{Committed, run};

const REPO: &str = "example.com/acme/widget";
const OID: &str = "affe2b9b7c608f4e354d7e83d0583ed35e845650";
const PARENT: &str = "6b668aa526afdc623c1a9ed1d6ae920e04a717ad";
const SECRET: &str = "git command [git show --secret] failed";

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
    let server = Script::graphql(envelope);
    let address =
        address::parse(&format!("{REPO}@v1")).expect("a well-formed address");
    let deadline = Instant::now() + DEADLINE;
    run(&address, &server, deadline, &mut Notes::new())
        .map_err(|error| reported(&error))
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
fn a_missing_author_fails_the_revision_lookup() {
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
fn a_missing_committer_is_read_as_none() {
    let mut body = envelope();
    commit_mut(&mut body)["committer"] = Value::Null;
    let committed = committed(&body).expect("the commit reads");
    assert_eq!(committed.committer, None);
    assert_eq!(committed.committer_date, None);
}
