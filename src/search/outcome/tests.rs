use serde_json::{Value, json};

use crate::output::Notes;
use crate::search::model::Searched;
use crate::search::outcome::{binary_size, judge};
use crate::search::query::prepare;
use crate::search::response::{Results, UnindexedFile};

const MIB: u64 = 1_048_576;
const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";
const PINNED_NOTE: &str = "sego: note: at a pinned revision the search \
    can't tell which files it skipped; files over 1 MiB are never searched\n";
const UNLISTED_NOTE: &str = "sego: note: the search can't tell which files \
    it skipped; files over 1 MiB and files with too many distinct trigrams \
    are never searched\n";

fn results(overrides: &Value) -> Results {
    let mut base = json!({
        "matchCount": 0,
        "approximateResultCount": "0",
        "limitHit": false,
        "timedout": [],
        "alert": null,
        "elapsedMilliseconds": 100,
        "dynamicFilters": [],
        "results": [],
    });
    if let (Some(map), Some(target)) =
        (overrides.as_object(), base.as_object_mut())
    {
        target.extend(map.clone());
    }
    match serde_json::from_value(base) {
        Ok(results) => results,
        Err(e) => panic!("bad response: {e}"),
    }
}

fn repo(name: &str) -> Value {
    json!({"__typename": "Repository", "name": name})
}

fn one_result() -> Value {
    json!({"matchCount": 1, "results": [repo("x")]})
}

fn file(repo: &str, path: &str, bytes: u64) -> UnindexedFile {
    UnindexedFile {
        repo: repo.into(),
        commit: COMMIT.into(),
        path: path.into(),
        bytes,
    }
}

/// The notes `judge` writes for the response to `query`, given the files
/// the side query listed, which is `None` when it wasn't sent.
fn notes_for(
    query: &str,
    overrides: &Value,
    unindexed: Option<Vec<UnindexedFile>>,
) -> String {
    let mut searched = Searched::new(results(overrides), &prepare(query));
    searched.unindexed = unindexed;
    let mut notes = Notes::new();
    assert!(judge(&searched, &mut notes).is_ok(), "{query}");
    let mut printed = Vec::new();
    assert!(notes.write(&mut printed).is_ok());
    String::from_utf8_lossy(&printed).into_owned()
}

#[test]
fn binary_sizes_are_written_in_the_largest_whole_unit() {
    assert_eq!(binary_size(4_273_688), "4.1 MiB");
    assert_eq!(binary_size(263_448), "257.3 KiB");
    assert_eq!(binary_size(12), "12 B");
    assert_eq!(binary_size(1023), "1023 B");
    assert_eq!(binary_size(1024), "1.0 KiB");
    assert_eq!(binary_size(MIB), "1.0 MiB");
}

#[test]
fn a_content_search_that_cant_list_its_unindexed_files_says_so() {
    for query in [
        "repo:x a or b",
        r"repo:^github\.com/sharkdp/bat$ (syntax_request.md or zzqqxx)",
        "repo:x foo",
        "foo",
        "repo:x type:symbol foo",
        "type:symbol foo",
    ] {
        for overrides in [json!({}), one_result()] {
            assert_eq!(
                notes_for(query, &overrides, None),
                UNLISTED_NOTE,
                "{query}"
            );
        }
    }
}

#[test]
fn a_search_that_reads_no_contents_has_no_unlisted_note() {
    for query in [
        "repo:x",
        r"repo:x file:\.go$",
        "repo:x type:path foo",
        "repo:x type:bogus foo",
        "repo:x@v1 type:bogus foo",
        "repo:x type:commit foo",
        "repo:x type:diff foo",
        "repo:x type:repo foo",
    ] {
        assert_eq!(notes_for(query, &json!({}), None), "", "{query}");
    }
}

#[test]
fn a_pinned_content_search_has_the_pinned_note() {
    for query in [
        "repo:x@v1 a or b",
        r"repo:^github\.com/gorilla/mux$@v1.8.0 file:^mux\.go$ (StrictSlash or zzqqxx)",
        "repo:x@v1 index:no a or b",
        "repo:x@v1 type:symbol foo",
    ] {
        assert_eq!(notes_for(query, &json!({}), None), PINNED_NOTE, "{query}");
    }
}

#[test]
fn a_head_or_empty_revision_takes_the_default_branch_notes() {
    let listed = |path| {
        format!(
            "sego: note: the contents of 1 file in scope aren't in the \
             search index (1 over 1 MiB):\n  {path}  2.0 MiB\n"
        )
    };
    for query in [
        "repo:x@HEAD foo",
        "repo:x@ foo",
        "repo:x rev:HEAD foo",
        "repo:x revision:HEAD foo",
        "repo:x@HEAD a or b",
    ] {
        for overrides in [json!({}), one_result()] {
            assert_eq!(
                notes_for(query, &overrides, None),
                UNLISTED_NOTE,
                "{query}"
            );
        }
    }
    for query in ["repo:x@HEAD foo", "repo:x@ foo", "repo:x rev:HEAD foo"] {
        for (overrides, path) in
            [(json!({}), "x@0123456789ab:big"), (one_result(), "big")]
        {
            let files = Some(vec![file("x", "big", 2 * MIB)]);
            assert_eq!(
                notes_for(query, &overrides, files),
                listed(path),
                "{query}"
            );
        }
    }
    assert_eq!(notes_for("repo:x@HEAD:v1 foo", &json!({}), None), PINNED_NOTE);
    assert_eq!(notes_for("repo:x@HEAD:v1 foo", &one_result(), None), "");
}
