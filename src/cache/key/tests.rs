use std::path::Path;

use super::{Key, entry_path};

#[test]
fn a_known_key_has_a_known_name() {
    let key = Key {
        endpoint: "https://sourcegraph.com",
        repo: "github.com/gorilla/mux",
        commit: "98cb6bf42e086f6af920b965c38cacc07402d51b",
        path: "mux.go",
    };
    assert_eq!(
        key.name(),
        "9c2f74df4e930a4eb06a7bb20d828db6dc4eed3fa759e256183cf59c00a91fb6"
    );
    assert_eq!(
        entry_path(Path::new("/c"), &key.name()),
        Path::new(
            "/c/raw/9c/2f74df4e930a4eb06a7bb20d828db6dc4eed3fa759e256183cf59c00a91fb6"
        )
    );
}

#[test]
fn names_differ_where_keys_differ() {
    // case only; parts that would run together without length prefixes;
    // endpoint scheme; prefix
    let base = Key {
        endpoint: "https://x",
        repo: "r",
        commit: "c",
        path: "xt_CONNMARK.h",
    };
    let names = [
        base.name(),
        Key { path: "xt_connmark.h", ..base }.name(),
        Key { repo: "rc", commit: "", ..base }.name(),
        Key { endpoint: "http://x", ..base }.name(),
        Key { endpoint: "https://x/sg", ..base }.name(),
    ];
    let distinct: std::collections::HashSet<_> = names.iter().collect();
    assert_eq!(distinct.len(), names.len());
}
