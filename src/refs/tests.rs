use std::time::Instant;

use serde_json::{Value, json};

use crate::address::Repo;
use crate::error::reported;
use crate::output::{Answer, Notes};
use crate::upstream::DEADLINE;
use crate::upstream::script::Script;

use super::run;

const REPO: &str = "example.com/acme/widget";

/// What `sego refs [--json] REPO [TEXT]` makes of `envelope`: its text, or the
/// error as written to stderr.
fn refs(
    text: Option<&str>,
    json: bool,
    envelope: &Value,
) -> Result<String, String> {
    let server = Script::graphql(envelope);
    let repo = Repo::Exact(REPO.to_owned());
    let deadline = Instant::now() + DEADLINE;
    run(&repo, text, json, &server, deadline, &mut Notes::new())
        .map(|listed| listed.text())
        .map_err(|error| reported(&error))
}

fn failure(text: Option<&str>, json: bool, envelope: &Value) -> String {
    refs(text, json, envelope).expect_err("the refs fail")
}

fn error_line(message: &str) -> String {
    format!("sego: error: {message}\n")
}

fn node(name: &str, n: usize) -> Value {
    json!({"displayName": name, "target": {"oid": format!("{n:040x}"),
        "commit": {"committer": {"date": "2026-01-02T03:04:05Z"}}}})
}

fn list(total: usize, nodes: &[Value]) -> Value {
    json!({"totalCount": total, "nodes": nodes})
}

fn root(branches: &Value, tags: &Value) -> Value {
    json!({
        "name": REPO,
        "defaultBranch": {"displayName": "main"},
        "branches": branches,
        "tags": tags,
    })
}

fn body(root: &Value) -> Value {
    json!({"data": {"r0": root}})
}

fn located(part: &str) -> Value {
    json!({"errors": [{
        "message": "panic occurred: runtime error: slice bounds out of range",
        "path": ["r0", part, "nodes"]
    }], "data": {"r0": null}})
}

#[test]
fn a_located_error_in_any_list_names_that_list() {
    for (part, text, json, message) in [
        ("tags", None, false, "the tag list"),
        ("branches", None, false, "the branch list"),
        ("all_tags", Some("x"), false, "the tag list"),
        ("all_branches", Some("x"), false, "the branch list"),
        ("tags", None, true, "the tag list"),
    ] {
        assert_eq!(
            failure(text, json, &located(part)),
            error_line(&format!("Sourcegraph failed to answer {message}")),
            "{part}"
        );
    }
}

#[test]
fn a_list_that_errors_beside_data_fails_that_list() {
    let mut tags_gone =
        body(&root(&list(1, &[node("main", 1)]), &Value::Null));
    tags_gone["errors"] = json!([{"message": "boom", "path": ["r0", "tags"]}]);
    assert_eq!(
        failure(None, false, &tags_gone),
        error_line("Sourcegraph failed to answer the tag list")
    );

    let mut branches_gone = body(&root(&Value::Null, &list(0, &[])));
    branches_gone["errors"] =
        json!([{"message": "boom", "path": ["r0", "branches", "nodes"]}]);
    assert_eq!(
        failure(None, false, &branches_gone),
        error_line("Sourcegraph failed to answer the branch list")
    );
}

#[test]
fn a_failed_default_branch_fails_the_repository_lookup() {
    let mut gone = body(&root(&list(0, &[]), &list(0, &[])));
    gone["data"]["r0"]["defaultBranch"] = Value::Null;
    gone["errors"] =
        json!([{"message": "boom", "path": ["r0", "defaultBranch"]}]);
    assert_eq!(
        failure(None, false, &gone),
        error_line("Sourcegraph failed to answer the repository lookup")
    );
}

#[test]
fn a_repository_with_null_lists_and_no_error_fails_the_branch_list() {
    assert_eq!(
        failure(None, false, &body(&root(&Value::Null, &Value::Null))),
        error_line("Sourcegraph failed to answer the branch list")
    );
}

#[test]
fn a_filter_without_the_unfiltered_totals_fails_the_branch_list() {
    let unfiltered = body(&root(&list(0, &[]), &list(0, &[])));
    assert_eq!(
        failure(Some("x"), false, &unfiltered),
        error_line("Sourcegraph failed to answer the branch list")
    );
}
