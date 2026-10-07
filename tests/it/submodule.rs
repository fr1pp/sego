use serde_json::{Value, json};

use sego::internal::submodule::{find, from_entry};

const PARENT: &str = "github.com/akinomyoga/ble.sh";
const COMMIT: &str = "d2109203480a7dfe1dead5f5f8f9f15a9146c90d";

fn entry(path: &str, url: &str, commit: &str) -> Value {
    json!({"path": path, "submodule": {"url": url, "commit": commit}})
}

#[test]
fn submodule_lookup() {
    let entries = vec![
        json!({"path": "make", "submodule": null}),
        entry("contrib", "../blesh-contrib", COMMIT),
        entry("local", "file:///srv/b.git", COMMIT),
        entry("blank", "", COMMIT),
    ];
    let shown = |path: &str| {
        find(&entries, path, PARENT)
            .unwrap_or_else(|| panic!("{path}: no submodule"))
    };

    let contrib = shown("contrib");
    assert_eq!(contrib.url, "../blesh-contrib");
    assert_eq!(
        contrib.target.as_deref(),
        Some("github.com/akinomyoga/blesh-contrib@d2109203480a")
    );
    assert_eq!(contrib.commit, COMMIT);
    assert_eq!(
        contrib.shown(),
        "github.com/akinomyoga/blesh-contrib@d2109203480a"
    );

    let local = shown("local");
    assert_eq!(local.target, None);
    assert_eq!(local.shown(), "file:///srv/b.git");

    let blank = shown("blank");
    assert_eq!(blank.target, None);
    assert_eq!(blank.shown(), "?");

    assert!(find(&entries, "make", PARENT).is_none());
    assert!(find(&entries, "absent", PARENT).is_none());
    assert!(find(&[], "contrib", PARENT).is_none());
}

#[test]
fn an_entry_without_a_commit_has_a_bare_target() {
    let bare = from_entry(&entry("c", "../blesh-contrib", ""), PARENT)
        .expect("a submodule");
    assert_eq!(bare.shown(), "github.com/akinomyoga/blesh-contrib");
    assert!(from_entry(&json!({"path": "x"}), PARENT).is_none());
}
