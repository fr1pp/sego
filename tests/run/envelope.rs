use serde_json::{Value, json};

use sego::Reply;

use crate::transport::reply;

pub const SHOW_BIG: [&str; 3] = ["show", "example.com/x/y@main:big.c", "1-5"];

const SHA: &str = "d81fd54f1a2b3c4d5e6f708192a3b4c5d6e7f809";

/// The response to `sego show` of a file of `byte_size` bytes that the
/// server refused to send because it is over the content limit.
pub fn over_the_content_limit(byte_size: usize) -> Value {
    json!({"data": {"r0": {
        "name": "example.com/x/y",
        "defaultBranch": {"displayName": "main"},
        "c0": {
            "oid": SHA,
            "meta": {
                "byteSize": byte_size, "totalLines": 5, "binary": false,
                "rawURL": format!("/r/example.com/x/y@{SHA}/-/raw/big.c"),
                "lfs": null
            },
            "body": null,
            "dir": null,
            "parent": {"entries": [{"path": "big.c", "submodule": null}]}
        }
    }}, "errors": [
        {"message": "not a directory", "path": ["r0", "c0", "dir"]},
        {"message": "file exceeded size limit", "path": ["r0", "c0", "body", "content"]},
        {"message": "file exceeded size limit", "path": ["r0", "c0", "meta", "lfs"]}
    ]})
}

/// The response to `sego show` of a file under the content limit.
pub fn under_the_content_limit() -> Value {
    json!({"data": {"r0": {
        "name": "example.com/x/y",
        "defaultBranch": {"displayName": "main"},
        "c0": {
            "oid": SHA,
            "meta": {
                "byteSize": 6, "totalLines": 3, "binary": false,
                "rawURL": format!("/r/example.com/x/y@{SHA}/-/raw/a.go"),
                "lfs": null
            },
            "body": {"content": "a\nb\nc\n"},
            "dir": null,
            "parent": {"entries": [{"path": "a.go", "submodule": null}]}
        }
    }}, "errors": [{"message": "not a directory", "path": ["r0", "c0", "dir"]}]})
}

pub fn json_reply(value: &Value) -> Reply {
    reply(200, "application/json", value.to_string().as_bytes())
}
