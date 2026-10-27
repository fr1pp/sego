//! The cache across two runs of sego on one directory.

use std::ffi::OsStr;

use serde_json::{Value, json};
use tempfile::TempDir;

use sego::{Response, TransportError};

use crate::support::replay::{Answers, response_of};
use crate::support::{Ran, sego};

const SHOW_BIG: [&str; 3] = ["show", "example.com/x/y@main:big.c", "1-5"];

const SHA: &str = "d81fd54f1a2b3c4d5e6f708192a3b4c5d6e7f809";

/// The response to `sego show` of a file of `byte_size` bytes that the
/// server refused to send because it is over the content limit.
fn over_the_content_limit(byte_size: usize) -> Value {
    json!({"data": {"r0": {
        "name": "example.com/x/y",
        "defaultBranch": {"displayName": "main"},
        "mirrorInfo": null,
        "commit": {
            "oid": SHA,
            "meta": {
                "__typename": "GitBlob", "path": "big.c",
                "byteSize": byte_size, "totalLines": 5, "binary": false,
                "rawURL": format!("/r/example.com/x/y@{SHA}/-/raw/big.c"),
                "lfs": null, "submodule": null
            },
            "body": null,
            "gm": null
        }
    }}, "errors": [
        {"message": "file exceeded size limit", "path": ["r0", "commit", "body", "content"]},
        {"message": "file exceeded size limit", "path": ["r0", "commit", "meta", "lfs"]}
    ]})
}

fn json_response(value: &Value) -> Response {
    response_of(200, Some("application/json"), value.to_string())
}

const FILE: &[u8] = b"1\n2\n3\n4\n5\n";

type Script = Vec<Result<Response, TransportError>>;

/// The responses to reading `big.c` through the raw route, whose size the
/// response gives as `byte_size`.
fn raw_read(byte_size: usize) -> Script {
    vec![
        Ok(json_response(&over_the_content_limit(byte_size))),
        Ok(response_of(200, Some("text/plain"), FILE)),
    ]
}

fn show_big(cache_dir: &OsStr, script: Script) -> (Ran, Answers) {
    let server = Answers::new(script);
    (sego(&SHOW_BIG, cache_dir, &server), server)
}

#[test]
fn a_second_read_of_a_large_file_uses_the_cache() {
    let dir = TempDir::new().unwrap();
    let (first, server) = show_big(dir.path().as_os_str(), raw_read(FILE.len()));
    assert_eq!(first.stderr, "");
    assert_eq!(first.code, 0);
    assert_eq!(server.methods(), ["POST", "GET"]);
    assert!(dir.path().join("trim.txt").exists());

    let (second, server) = show_big(
        dir.path().as_os_str(),
        vec![Ok(json_response(&over_the_content_limit(FILE.len())))],
    );
    assert_eq!(second.stderr, "");
    assert_eq!(second.code, 0);
    assert_eq!(server.methods(), ["POST"]);
    assert_eq!(second.stdout, first.stdout);
    assert!(second.stdout.ends_with("5:5\n"), "{}", second.stdout);
}
