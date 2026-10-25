use std::cell::Cell;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::address;
use crate::error::report;
use crate::output::{Answer, Notes};
use crate::upstream::{Reply, Transport, TransportError};

use super::{Listed, run};

const REPO: &str = "example.com/acme/widget";
const OID: &str = "98cb6bf42e086f6af920b965c38cacc07402d51b";
const SECRET: &str = "rpc error: code = Internal desc = git command failed";

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
    let server = Server {
        reply: Cell::new(Some(Reply {
            status: 200,
            content_type: Some("application/json".to_owned()),
            location: None,
            retry_after: None,
            cf_mitigated: None,
            body: body.to_string().into_bytes(),
        })),
    };
    let address = address::parse(token).expect("a well-formed address");
    let deadline = Instant::now() + Duration::from_secs(90);
    run(&address, false, &server, deadline, &mut Notes::new()).map_err(
        |error| {
            let mut written = Vec::new();
            report(&error, &mut written).expect("a vector takes a write");
            String::from_utf8_lossy(&written).into_owned()
        },
    )
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
fn entries_sort_by_path_segment() {
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
