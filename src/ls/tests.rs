use std::time::Instant;

use serde_json::{Value, json};

use crate::address;
use crate::error::reported;
use crate::output::{Answer, Notes};
use crate::upstream::DEADLINE;
use crate::upstream::script::Script;

use super::{Listed, run};

const REPO: &str = "example.com/acme/widget";
const OID: &str = "98cb6bf42e086f6af920b965c38cacc07402d51b";
const SECRET: &str = "rpc error: code = Internal desc = git command failed";

fn entry(path: &str, is_dir: bool) -> Value {
    json!({"path": path, "isDirectory": is_dir, "submodule": null})
}

fn envelope(mut commit: Value) -> Value {
    commit["oid"] = json!(OID);
    json!({"data": {"r0": {
        "name": REPO,
        "defaultBranch": {"displayName": "main"},
        "c0": commit,
    }}})
}

fn listed(token: &str, body: &Value) -> Result<Listed, String> {
    let server = Script::graphql(body);
    let address = address::parse(token).expect("a well-formed address");
    let deadline = Instant::now() + DEADLINE;
    run(&address, false, &server, deadline, &mut Notes::new())
        .map_err(|error| reported(&error))
}

fn failed(token: &str, body: &Value) {
    let written = listed(token, body).err().expect("the listing fails");
    assert_eq!(
        written,
        "sego: error: Sourcegraph failed to answer the directory listing\n"
    );
}

fn paths(token: &str, entries: &[Value]) -> Vec<String> {
    let body = envelope(json!({
        "tree": {"entries": entries},
        "file": null,
        "parent": null,
    }));
    let listed = listed(token, &body).expect("the listing reads");
    let mut json = Vec::new();
    listed.write_json(&mut json).expect("a vector takes a write");
    let json: Value = serde_json::from_slice(&json).expect("JSON");
    json["entries"]
        .as_array()
        .expect("entries")
        .iter()
        .map(|entry| entry["path"].as_str().expect("a path").to_owned())
        .collect()
}

#[test]
fn entries_are_sorted_by_path_segment() {
    let entries = [
        entry("pkg/foo.go", false),
        entry("pkg/foo/sub/x.go", false),
        entry("pkg/foo_x.go", false),
        entry("pkg/foo", true),
        entry("pkg/foo-bar.go", false),
        entry("pkg/foo/bar.go", false),
        entry("pkg/foo/sub", true),
    ];
    assert_eq!(
        paths(&format!("{REPO}@v1:pkg"), &entries),
        [
            "pkg/foo",
            "pkg/foo/bar.go",
            "pkg/foo/sub",
            "pkg/foo/sub/x.go",
            "pkg/foo-bar.go",
            "pkg/foo.go",
            "pkg/foo_x.go",
        ]
    );
}

#[test]
fn an_error_beneath_the_tree_fails_the_listing() {
    let at =
        |part: &str| json!({"message": SECRET, "path": ["r0", "c0", part]});
    let parent = json!({"entries": [entry("broken", false)]});
    let token = format!("{REPO}@v1:broken");

    let mut tree_only = envelope(json!({
        "tree": null,
        "file": null,
        "parent": parent,
    }));
    tree_only["errors"] = json!([at("tree")]);
    failed(&token, &tree_only);

    let mut file_and_tree = tree_only.clone();
    file_and_tree["errors"] = json!([at("file"), at("tree")]);
    failed(&token, &file_and_tree);

    let mut all_three = tree_only.clone();
    all_three["errors"] = json!([at("file"), at("tree"), at("parent")]);
    failed(&token, &all_three);

    let mut at_the_root =
        envelope(json!({"tree": null, "file": null, "parent": null}));
    at_the_root["errors"] = json!([at("tree")]);
    failed(REPO, &at_the_root);
}
