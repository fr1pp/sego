//! `sego search`.

use insta::assert_snapshot;
use serde_json::json;

use crate::common::case::Case;
use crate::common::record::stopped;
use crate::common::{answered, sego};

/// An alert's title, a timed-out repository, filters and results with control characters are escaped, and so is the query, given as the argument.
#[test]
fn alert_and_results_with_control_characters() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": {
                        "description": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                        "kind": null,
                        "proposedQueries": [
                            {
                                "description": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                                "query": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                            },
                        ],
                        "title": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                    },
                    "approximateResultCount": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                    "dynamicFilters": [
                        {
                            "count": 1,
                            "kind": "lang",
                            "value": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                        },
                        {
                            "count": 2,
                            "kind": "utility",
                            "value": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                        },
                    ],
                    "elapsedMilliseconds": 100,
                    "limitHit": false,
                    "matchCount": 10,
                    "results": [
                        {
                            "__typename": "FileMatch",
                            "pathMatches": [],
                            "chunkMatches": [
                                {
                                    "content": "a\u{1b}b\nc\u{202e}d\ne\u{200b}f\ni\u{2028}j\nk\u{7f}l\nmid\rcr\ntab\tok\n",
                                    "contentStart": {"line": 2},
                                    "ranges": [
                                        {
                                            "end": {"character": 2, "line": 2},
                                            "start": {"character": 1, "line": 2},
                                        },
                                    ],
                                },
                            ],
                            "file": {
                                "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                "path": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                            },
                            "repository": {"name": "example.com/acme/widget"},
                            "revSpec": {"expr": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i"},
                            "symbols": [
                                {
                                    "containerName": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                                    "kind": "FUNCTION",
                                    "location": {"range": {"start": {"line": 2}}},
                                    "name": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                                },
                            ],
                        },
                        {
                            "__typename": "FileMatch",
                            "pathMatches": [],
                            "chunkMatches": [{"content": "a\0b\n", "contentStart": {"line": 0}, "ranges": []}],
                            "file": {
                                "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                "path": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i.bin",
                            },
                            "repository": {"name": "example.com/acme/widget"},
                            "revSpec": null,
                            "symbols": [],
                        },
                        {
                            "__typename": "CommitSearchResult",
                            "commit": {
                                "author": {
                                    "date": "2024-01-02T03:04:05Z",
                                    "person": {
                                        "displayName": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                                    },
                                },
                                "oid": "fedcba9876543210fedcba9876543210fedcba98",
                                "repository": {"name": "example.com/acme/widget"},
                                "subject": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                            },
                            "matches": [
                                {
                                    "body": {
                                        "text": "```\na\u{1b}b\nc\u{202e}d\ne\u{200b}f\ni\u{2028}j\nk\u{7f}l\nmid\rcr\ntab\tok\n```",
                                    },
                                    "highlights": [{"character": 1, "length": 2, "line": 2}],
                                },
                            ],
                        },
                        {"__typename": "Repository", "name": "example.com/acme/widget"},
                    ],
                    "timedout": [{"name": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i"}],
                },
            },
        },
    });
    assert_snapshot!(answered(
        &["search", "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i ok"],
        [response]
    ));
}

/// In JSON the alert, the filters and the results hold the characters themselves.
#[test]
fn alert_and_results_with_control_characters_json() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": {
                        "description": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                        "kind": null,
                        "proposedQueries": [
                            {
                                "description": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                                "query": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                            },
                        ],
                        "title": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                    },
                    "approximateResultCount": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                    "dynamicFilters": [
                        {
                            "count": 1,
                            "kind": "lang",
                            "value": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                        },
                        {
                            "count": 2,
                            "kind": "utility",
                            "value": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                        },
                    ],
                    "elapsedMilliseconds": 100,
                    "limitHit": false,
                    "matchCount": 10,
                    "results": [
                        {
                            "__typename": "FileMatch",
                            "pathMatches": [],
                            "chunkMatches": [
                                {
                                    "content": "a\u{1b}b\nc\u{202e}d\ne\u{200b}f\ni\u{2028}j\nk\u{7f}l\nmid\rcr\ntab\tok\n",
                                    "contentStart": {"line": 2},
                                    "ranges": [
                                        {
                                            "end": {"character": 2, "line": 2},
                                            "start": {"character": 1, "line": 2},
                                        },
                                    ],
                                },
                            ],
                            "file": {
                                "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                "path": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                            },
                            "repository": {"name": "example.com/acme/widget"},
                            "revSpec": {"expr": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i"},
                            "symbols": [
                                {
                                    "containerName": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                                    "kind": "FUNCTION",
                                    "location": {"range": {"start": {"line": 2}}},
                                    "name": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                                },
                            ],
                        },
                        {
                            "__typename": "FileMatch",
                            "pathMatches": [],
                            "chunkMatches": [{"content": "a\0b\n", "contentStart": {"line": 0}, "ranges": []}],
                            "file": {
                                "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                "path": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i.bin",
                            },
                            "repository": {"name": "example.com/acme/widget"},
                            "revSpec": null,
                            "symbols": [],
                        },
                        {
                            "__typename": "CommitSearchResult",
                            "commit": {
                                "author": {
                                    "date": "2024-01-02T03:04:05Z",
                                    "person": {
                                        "displayName": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                                    },
                                },
                                "oid": "fedcba9876543210fedcba9876543210fedcba98",
                                "repository": {"name": "example.com/acme/widget"},
                                "subject": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                            },
                            "matches": [
                                {
                                    "body": {
                                        "text": "```\na\u{1b}b\nc\u{202e}d\ne\u{200b}f\ni\u{2028}j\nk\u{7f}l\nmid\rcr\ntab\tok\n```",
                                    },
                                    "highlights": [{"character": 1, "length": 2, "line": 2}],
                                },
                            ],
                        },
                        {"__typename": "Repository", "name": "example.com/acme/widget"},
                    ],
                    "timedout": [{"name": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i"}],
                },
            },
        },
    });
    let ran = answered(
        &["search", "--json", "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i ok"],
        [response],
    );
    assert!(stopped(&ran.stdout, &ran.stderr));
    assert_eq!(
        ran.stdout,
        "{\"query\":\"timeout:60s a\\u001bb\\u202ec\\u200bd\\ne\\rf\\u0000g\\u2028h\\u007fi ok\",\"pattern_type\":\"keyword\",\"unit\":\"matches\",\"count\":10,\"at_least\":true,\"stopped\":true,\"elapsed_ms\":100,\"limit_ms\":60000,\"widen\":[{\"filter\":\"a\\u001bb\\u202ec\\u200bd\\ne\\rf\\u0000g\\u2028h\\u007fi\",\"repos\":2}],\"alert\":{\"title\":\"a\\u001bb\\u202ec\\u200bd\\ne\\rf\\u0000g\\u2028h\\u007fi\",\"description\":\"a\\u001bb\\u202ec\\u200bd\\ne\\rf\\u0000g\\u2028h\\u007fi\",\"kind\":null,\"proposed\":[{\"description\":\"a\\u001bb\\u202ec\\u200bd\\ne\\rf\\u0000g\\u2028h\\u007fi\",\"query\":\"a\\u001bb\\u202ec\\u200bd\\ne\\rf\\u0000g\\u2028h\\u007fi\"}]},\"results\":[{\"kind\":\"line\",\"repo\":\"example.com/acme/widget\",\"commit\":\"0123456789abcdef0123456789abcdef01234567\",\"rev\":\"a\\u001bb\\u202ec\\u200bd\\ne\\rf\\u0000g\\u2028h\\u007fi\",\"path\":\"a\\u001bb\\u202ec\\u200bd\\ne\\rf\\u0000g\\u2028h\\u007fi\",\"line\":3,\"text\":\"a\\u001bb\",\"match\":true,\"ranges\":[[1,2]],\"chunk\":1},{\"kind\":\"symbol\",\"repo\":\"example.com/acme/widget\",\"commit\":\"0123456789abcdef0123456789abcdef01234567\",\"rev\":\"a\\u001bb\\u202ec\\u200bd\\ne\\rf\\u0000g\\u2028h\\u007fi\",\"path\":\"a\\u001bb\\u202ec\\u200bd\\ne\\rf\\u0000g\\u2028h\\u007fi\",\"line\":3,\"name\":\"a\\u001bb\\u202ec\\u200bd\\ne\\rf\\u0000g\\u2028h\\u007fi\",\"symbol_kind\":\"function\",\"container\":\"a\\u001bb\\u202ec\\u200bd\\ne\\rf\\u0000g\\u2028h\\u007fi\"},{\"kind\":\"line\",\"repo\":\"example.com/acme/widget\",\"commit\":\"0123456789abcdef0123456789abcdef01234567\",\"rev\":\"a\\u001bb\\u202ec\\u200bd\\ne\\rf\\u0000g\\u2028h\\u007fi\",\"path\":\"a\\u001bb\\u202ec\\u200bd\\ne\\rf\\u0000g\\u2028h\\u007fi\",\"line\":4,\"text\":\"c\\u202ed\",\"match\":false,\"ranges\":[],\"chunk\":1},{\"kind\":\"line\",\"repo\":\"example.com/acme/widget\",\"commit\":\"0123456789abcdef0123456789abcdef01234567\",\"rev\":\"a\\u001bb\\u202ec\\u200bd\\ne\\rf\\u0000g\\u2028h\\u007fi\",\"path\":\"a\\u001bb\\u202ec\\u200bd\\ne\\rf\\u0000g\\u2028h\\u007fi\",\"line\":5,\"text\":\"e\\u200bf\",\"match\":false,\"ranges\":[],\"chunk\":1},{\"kind\":\"line\",\"repo\":\"example.com/acme/widget\",\"commit\":\"0123456789abcdef0123456789abcdef01234567\",\"rev\":\"a\\u001bb\\u202ec\\u200bd\\ne\\rf\\u0000g\\u2028h\\u007fi\",\"path\":\"a\\u001bb\\u202ec\\u200bd\\ne\\rf\\u0000g\\u2028h\\u007fi\",\"line\":6,\"text\":\"i\\u2028j\",\"match\":false,\"ranges\":[],\"chunk\":1},{\"kind\":\"line\",\"repo\":\"example.com/acme/widget\",\"commit\":\"0123456789abcdef0123456789abcdef01234567\",\"rev\":\"a\\u001bb\\u202ec\\u200bd\\ne\\rf\\u0000g\\u2028h\\u007fi\",\"path\":\"a\\u001bb\\u202ec\\u200bd\\ne\\rf\\u0000g\\u2028h\\u007fi\",\"line\":7,\"text\":\"k\\u007fl\",\"match\":false,\"ranges\":[],\"chunk\":1},{\"kind\":\"line\",\"repo\":\"example.com/acme/widget\",\"commit\":\"0123456789abcdef0123456789abcdef01234567\",\"rev\":\"a\\u001bb\\u202ec\\u200bd\\ne\\rf\\u0000g\\u2028h\\u007fi\",\"path\":\"a\\u001bb\\u202ec\\u200bd\\ne\\rf\\u0000g\\u2028h\\u007fi\",\"line\":8,\"text\":\"mid\\rcr\",\"match\":false,\"ranges\":[],\"chunk\":1},{\"kind\":\"line\",\"repo\":\"example.com/acme/widget\",\"commit\":\"0123456789abcdef0123456789abcdef01234567\",\"rev\":\"a\\u001bb\\u202ec\\u200bd\\ne\\rf\\u0000g\\u2028h\\u007fi\",\"path\":\"a\\u001bb\\u202ec\\u200bd\\ne\\rf\\u0000g\\u2028h\\u007fi\",\"line\":9,\"text\":\"tab\\tok\",\"match\":false,\"ranges\":[],\"chunk\":1},{\"kind\":\"binary\",\"repo\":\"example.com/acme/widget\",\"commit\":\"0123456789abcdef0123456789abcdef01234567\",\"rev\":null,\"path\":\"a\\u001bb\\u202ec\\u200bd\\ne\\rf\\u0000g\\u2028h\\u007fi.bin\"},{\"kind\":\"commit\",\"repo\":\"example.com/acme/widget\",\"commit\":\"fedcba9876543210fedcba9876543210fedcba98\",\"date\":\"2024-01-02\",\"author\":\"a\\u001bb\\u202ec\\u200bd\\ne\\rf\\u0000g\\u2028h\\u007fi\",\"subject\":\"a\\u001bb\\u202ec\\u200bd\\ne\\rf\\u0000g\\u2028h\\u007fi\",\"lines\":[\"a\\u001bb\",\"c\\u202ed\",\"e\\u200bf\",\"i\\u2028j\",\"k\\u007fl\",\"mid\\rcr\",\"tab\\tok\"],\"highlights\":[[1,1,3]]},{\"kind\":\"repo\",\"repo\":\"example.com/acme/widget\"}],\"server\":{\"match_count\":10,\"approximate_count\":\"a\\u001bb\\u202ec\\u200bd\\ne\\rf\\u0000g\\u2028h\\u007fi\",\"limit_hit\":false,\"timedout\":[\"a\\u001bb\\u202ec\\u200bd\\ne\\rf\\u0000g\\u2028h\\u007fi\"]},\"notes\":[\"a\\u001bb\\u202ec\\u200bd\\ne\\rf\\u0000g\\u2028h\\u007fi: a\\u001bb\\u202ec\\u200bd\\ne\\rf\\u0000g\\u2028h\\u007fi\",\"the server proposes: a\\u001bb\\u202ec\\u200bd\\ne\\rf\\u0000g\\u2028h\\u007fi\"]}\n"
    );
    assert_snapshot!(ran);
}

/// An alert with results is a note, and each proposed query is passed on.
#[test]
fn alert_with_results_passes_on_proposed_queries() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": {
                        "description": "Narrow the search.",
                        "kind": null,
                        "proposedQueries": [
                            {
                                "description": "only go",
                                "query": "repo:example.com/acme/widget foo lang:go",
                            },
                            {
                                "description": "only rust",
                                "query": "repo:example.com/acme/widget foo lang:rust",
                            },
                        ],
                        "title": "Too many results",
                    },
                    "approximateResultCount": "1",
                    "dynamicFilters": [],
                    "elapsedMilliseconds": 800,
                    "limitHit": false,
                    "matchCount": 1,
                    "results": [{"__typename": "Repository", "name": "example.com/acme/widget"}],
                    "timedout": [],
                },
            },
        },
    });
    assert_snapshot!(answered(&["search", "repo:example.com/acme/widget foo"], [response]));
}

/// An alert with no description is a note of its title.
#[test]
fn alert_without_description() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": {
                        "description": null,
                        "kind": null,
                        "proposedQueries": null,
                        "title": "Heads up",
                    },
                    "approximateResultCount": "1",
                    "dynamicFilters": [],
                    "elapsedMilliseconds": 100,
                    "limitHit": false,
                    "matchCount": 1,
                    "results": [{"__typename": "Repository", "name": "example.com/acme/widget"}],
                    "timedout": [],
                },
            },
        },
    });
    assert_snapshot!(answered(&["search", "repo:example.com/acme/widget type:repo"], [response]));
}

/// A clean answer that arrived at its deadline is stopped: line 1 says so.
#[test]
fn answer_at_its_deadline_is_stopped() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": null,
                    "approximateResultCount": "1",
                    "dynamicFilters": [],
                    "elapsedMilliseconds": 60009,
                    "limitHit": false,
                    "matchCount": 1,
                    "results": [{"__typename": "Repository", "name": "example.com/acme/widget"}],
                    "timedout": [],
                },
            },
        },
    });
    let ran = answered(&["search", "repo:example.com/acme/widget foo"], [response]);
    assert!(stopped(&ran.stdout, &ran.stderr));
    assert_snapshot!(ran);
}

/// A chunk holding a NUL is a binary file; its lines are not shown.
#[test]
fn binary_file() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": null,
                    "approximateResultCount": "1",
                    "dynamicFilters": [],
                    "elapsedMilliseconds": 100,
                    "limitHit": false,
                    "matchCount": 1,
                    "results": [
                        {
                            "__typename": "FileMatch",
                            "pathMatches": [],
                            "chunkMatches": [
                                {
                                    "content": "ab\0cd\nfoo",
                                    "contentStart": {"line": 0},
                                    "ranges": [
                                        {
                                            "end": {"character": 3, "line": 1},
                                            "start": {"character": 0, "line": 1},
                                        },
                                    ],
                                },
                            ],
                            "file": {
                                "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                "path": "blob.bin",
                            },
                            "repository": {"name": "example.com/acme/widget"},
                            "revSpec": null,
                            "symbols": [],
                        },
                    ],
                    "timedout": [],
                },
            },
        },
    });
    assert_snapshot!(answered(&["search", "repo:example.com/acme/widget foo"], [response]));
}

/// A binary file is a binary record in JSON, with no line records.
#[test]
fn binary_file_json() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": null,
                    "approximateResultCount": "1",
                    "dynamicFilters": [],
                    "elapsedMilliseconds": 100,
                    "limitHit": false,
                    "matchCount": 1,
                    "results": [
                        {
                            "__typename": "FileMatch",
                            "pathMatches": [],
                            "chunkMatches": [
                                {
                                    "content": "a\0b",
                                    "contentStart": {"line": 0},
                                    "ranges": [
                                        {
                                            "end": {"character": 1, "line": 0},
                                            "start": {"character": 0, "line": 0},
                                        },
                                    ],
                                },
                            ],
                            "file": {
                                "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                "path": "a.bin",
                            },
                            "repository": {"name": "example.com/app"},
                            "revSpec": null,
                            "symbols": [],
                        },
                    ],
                    "timedout": [],
                },
            },
        },
    });
    assert_snapshot!(answered(&["search", "--json", "repo:example.com/app foo"], [response]));
}

/// A binary file counts itself and its symbols that have a line; its lines are
/// not shown.
#[test]
fn binary_file_with_symbols() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": null,
                    "approximateResultCount": "3",
                    "dynamicFilters": [],
                    "elapsedMilliseconds": 100,
                    "limitHit": false,
                    "matchCount": 3,
                    "results": [
                        {
                            "__typename": "FileMatch",
                            "pathMatches": [],
                            "chunkMatches": [{"content": "a\0b", "contentStart": {"line": 0}, "ranges": []}],
                            "file": {
                                "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                "path": "a.bin",
                            },
                            "repository": {"name": "example.com/app"},
                            "revSpec": null,
                            "symbols": [
                                {
                                    "containerName": null,
                                    "kind": "FUNCTION",
                                    "location": {"range": {"start": {"line": 2}}},
                                    "name": "S",
                                },
                                {
                                    "containerName": null,
                                    "kind": "FUNCTION",
                                    "location": {"range": {"start": {"line": 4}}},
                                    "name": "T",
                                },
                                {
                                    "containerName": null,
                                    "kind": "FUNCTION",
                                    "location": null,
                                    "name": "U",
                                },
                            ],
                        },
                    ],
                    "timedout": [],
                },
            },
        },
    });
    assert_snapshot!(answered(&["search", "repo:example.com/app foo"], [response]));
}

/// A blank line follows each result that prints lines, and none follows a path or
/// a repository.
#[test]
fn blank_line_follows_a_result_with_lines() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": null,
                    "approximateResultCount": "6",
                    "dynamicFilters": [],
                    "elapsedMilliseconds": 700,
                    "limitHit": false,
                    "matchCount": 6,
                    "results": [
                        {
                            "__typename": "FileMatch",
                            "pathMatches": [],
                            "chunkMatches": [
                                {
                                    "content": "x",
                                    "contentStart": {"line": 9},
                                    "ranges": [
                                        {
                                            "end": {"character": 1, "line": 9},
                                            "start": {"character": 0, "line": 9},
                                        },
                                    ],
                                },
                            ],
                            "file": {
                                "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                "path": "a.go",
                            },
                            "repository": {"name": "example.com/acme/widget"},
                            "revSpec": null,
                            "symbols": [],
                        },
                        {
                            "__typename": "FileMatch",
                            "pathMatches": [],
                            "chunkMatches": [
                                {
                                    "content": "x",
                                    "contentStart": {"line": 19},
                                    "ranges": [
                                        {
                                            "end": {"character": 1, "line": 19},
                                            "start": {"character": 0, "line": 19},
                                        },
                                    ],
                                },
                            ],
                            "file": {
                                "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                "path": "b.go",
                            },
                            "repository": {"name": "example.com/acme/widget"},
                            "revSpec": null,
                            "symbols": [],
                        },
                        {
                            "__typename": "FileMatch",
                            "pathMatches": [],
                            "chunkMatches": [],
                            "file": {
                                "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                "path": "p1",
                            },
                            "repository": {"name": "example.com/acme/widget"},
                            "revSpec": null,
                            "symbols": [],
                        },
                        {
                            "__typename": "FileMatch",
                            "pathMatches": [],
                            "chunkMatches": [],
                            "file": {
                                "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                "path": "p2",
                            },
                            "repository": {"name": "example.com/acme/widget"},
                            "revSpec": null,
                            "symbols": [],
                        },
                        {
                            "__typename": "FileMatch",
                            "pathMatches": [],
                            "chunkMatches": [
                                {
                                    "content": "x",
                                    "contentStart": {"line": 29},
                                    "ranges": [
                                        {
                                            "end": {"character": 1, "line": 29},
                                            "start": {"character": 0, "line": 29},
                                        },
                                    ],
                                },
                            ],
                            "file": {
                                "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                "path": "c.go",
                            },
                            "repository": {"name": "example.com/acme/widget"},
                            "revSpec": null,
                            "symbols": [],
                        },
                        {
                            "__typename": "CommitSearchResult",
                            "commit": {
                                "author": {
                                    "date": "2024-05-06T07:08:09Z",
                                    "person": {"displayName": "Ann"},
                                },
                                "oid": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                                "repository": {"name": "example.com/acme/widget"},
                                "subject": "Fix it",
                            },
                            "matches": [
                                {
                                    "body": {"text": "```\nFix it\n```"},
                                    "highlights": [{"character": 0, "length": 3, "line": 1}],
                                },
                            ],
                        },
                        {"__typename": "Repository", "name": "example.com/acme/gadget"},
                        {"__typename": "Repository", "name": "example.com/acme/lib"},
                    ],
                    "timedout": [],
                },
            },
        },
    });
    assert_snapshot!(answered(
        &[
            "search",
            "(repo:example.com/acme/widget foo) or (repo:example.com/acme/widget type:path foo) or (repo:example.com/acme/widget type:commit foo)"
        ],
        [response]
    ));
}

/// A clean answer 0.5 s short of its 60 s deadline came back before it, so it is complete.
#[test]
fn clean_answer_near_deadline() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": null,
                    "approximateResultCount": "1",
                    "dynamicFilters": [],
                    "elapsedMilliseconds": 59500,
                    "limitHit": false,
                    "matchCount": 1,
                    "results": [
                        {
                            "__typename": "FileMatch",
                            "pathMatches": [],
                            "chunkMatches": [
                                {
                                    "content": "foo 0",
                                    "contentStart": {"line": 0},
                                    "ranges": [
                                        {
                                            "end": {"character": 3, "line": 0},
                                            "start": {"character": 0, "line": 0},
                                        },
                                    ],
                                },
                            ],
                            "file": {
                                "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                "path": "main.go",
                            },
                            "repository": {"name": "example.com/app"},
                            "revSpec": null,
                            "symbols": [],
                        },
                    ],
                    "timedout": [],
                },
            },
        },
    });
    assert_snapshot!(answered(&["search", "repo:example.com/app foo count:1000"], [response]));
}

/// A clean zero that came back after a 1ms deadline is stopped, not empty.
#[test]
fn clean_zero_past_a_short_deadline_is_stopped() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": null,
                    "approximateResultCount": "0",
                    "dynamicFilters": [],
                    "elapsedMilliseconds": 15,
                    "limitHit": false,
                    "matchCount": 0,
                    "results": [],
                    "timedout": [],
                },
            },
        },
    });
    let ran = answered(&["search", "repo:example.com/acme/widget func timeout:1ms"], [response]);
    assert!(stopped(&ran.stdout, &ran.stderr));
    assert_snapshot!(ran);
}

/// Cloning and missing repositories get no note; the results still print.
#[test]
fn cloning_and_missing_repositories_have_no_note() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": null,
                    "approximateResultCount": "1",
                    "dynamicFilters": [],
                    "elapsedMilliseconds": 800,
                    "limitHit": false,
                    "matchCount": 1,
                    "results": [{"__typename": "Repository", "name": "example.com/acme/widget"}],
                    "timedout": [],
                },
            },
        },
    });
    assert_snapshot!(answered(&["search", "repo:example.com/acme/widget foo"], [response]));
}

/// A closed stdout is not an error.
#[test]
fn closed_stdout_is_not_an_error() {
    assert_snapshot!(
        Case::new(
            &["search", "repo:^github\\.com/gorilla/mux$@v1.8.0 \"func NewRouter\""],
            Vec::new()
        )
        .stdout_closed()
        .ran()
    );
}

/// A diff body with CR line breaks and a blank last line loses both, and a
/// highlight on the blank line is dropped.
#[test]
fn commit_crlf_blank_line_json() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": null,
                    "approximateResultCount": "1",
                    "dynamicFilters": [],
                    "elapsedMilliseconds": 100,
                    "limitHit": false,
                    "matchCount": 1,
                    "results": [
                        {
                            "__typename": "CommitSearchResult",
                            "commit": {
                                "author": {
                                    "date": "2024-05-06T07:08:09Z",
                                    "person": {"displayName": "Ann"},
                                },
                                "oid": "0123456789abcdef0123456789abcdef01234567",
                                "repository": {"name": "example.com/app"},
                                "subject": "fix",
                            },
                            "matches": [
                                {
                                    "body": {"text": "```diff\r\nx\r\n+y\r\n\r\n```"},
                                    "highlights": [{"character": 0, "length": 1, "line": 3}],
                                },
                            ],
                        },
                    ],
                    "timedout": [],
                },
            },
        },
    });
    assert_snapshot!(answered(
        &["search", "--json", "repo:example.com/app type:diff foo"],
        [response]
    ));
}

/// A commit record keeps the highlights that fall in the message or diff, and
/// drops the ones before its first line or past its last.
#[test]
fn commit_highlights_json() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": null,
                    "approximateResultCount": "4",
                    "dynamicFilters": [],
                    "elapsedMilliseconds": 100,
                    "limitHit": false,
                    "matchCount": 4,
                    "results": [
                        {
                            "__typename": "CommitSearchResult",
                            "commit": {
                                "author": {
                                    "date": "2024-05-06T07:08:09Z",
                                    "person": {"displayName": "Ann"},
                                },
                                "oid": "0123456789abcdef0123456789abcdef01234567",
                                "repository": {"name": "example.com/app"},
                                "subject": "fix",
                            },
                            "matches": [
                                {
                                    "body": {
                                        "text": "```diff\nold_test.go old_test.go\n@@ -263,0 +263,3 @@ x\n+a\n\n```",
                                    },
                                    "highlights": [
                                        {"character": 1, "length": 2, "line": 3},
                                        {"character": 1, "length": 2, "line": 4},
                                        {"character": 1, "length": 2, "line": 0},
                                        {"character": 1, "length": 2, "line": 40},
                                    ],
                                },
                            ],
                        },
                    ],
                    "timedout": [],
                },
            },
        },
    });
    assert_snapshot!(answered(
        &["search", "--json", "repo:example.com/app type:diff foo"],
        [response]
    ));
}

/// A commit search prints the commit line and the message lines around each
/// highlight.
#[test]
fn commit_result() {
    assert_snapshot!(sego(&[
        "search",
        "repo:^github\\.com/gorilla/mux$@v1.8.0 type:commit wildcard"
    ]));
}

/// A commit with two matches prints the lines around the highlights of each.
#[test]
fn commit_result_prints_the_highlights_of_each_of_its_matches() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": null,
                    "approximateResultCount": "2",
                    "dynamicFilters": [],
                    "elapsedMilliseconds": 700,
                    "limitHit": false,
                    "matchCount": 2,
                    "results": [
                        {
                            "__typename": "CommitSearchResult",
                            "commit": {
                                "author": {
                                    "date": "2024-05-06T07:08:09Z",
                                    "person": {"displayName": "Ann"},
                                },
                                "oid": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                                "repository": {"name": "example.com/acme/widget"},
                                "subject": "Fix it",
                            },
                            "matches": [
                                {
                                    "body": {"text": "```\nm1\nx2\nx3\nx4\n```"},
                                    "highlights": [{"character": 0, "length": 1, "line": 1}],
                                },
                                {
                                    "body": {"text": "```\ny1\ny2\ny3\nm4\n```"},
                                    "highlights": [{"character": 0, "length": 1, "line": 4}],
                                },
                            ],
                        },
                    ],
                    "timedout": [],
                },
            },
        },
    });
    assert_snapshot!(answered(
        &["search", "repo:example.com/acme/widget type:commit m"],
        [response]
    ));
}

/// A commit prints the message line before and after each highlight; context that
/// touches has no separator.
#[test]
fn commit_result_separates_context_that_is_not_contiguous() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": null,
                    "approximateResultCount": "4",
                    "dynamicFilters": [],
                    "elapsedMilliseconds": 700,
                    "limitHit": false,
                    "matchCount": 4,
                    "results": [
                        {
                            "__typename": "CommitSearchResult",
                            "commit": {
                                "author": {
                                    "date": "2024-05-06T07:08:09Z",
                                    "person": {"displayName": "Ann"},
                                },
                                "oid": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                                "repository": {"name": "example.com/acme/widget"},
                                "subject": "Fix it",
                            },
                            "matches": [
                                {
                                    "body": {
                                        "text": "```\nm1\nm2\nm3\nm4\nm5\nm6\nm7\nm8\nm9\nm10\nm11\nm12\n```",
                                    },
                                    "highlights": [
                                        {"character": 0, "length": 1, "line": 1},
                                        {"character": 0, "length": 1, "line": 6},
                                    ],
                                },
                            ],
                        },
                        {
                            "__typename": "CommitSearchResult",
                            "commit": {
                                "author": {
                                    "date": "2024-05-06T07:08:09Z",
                                    "person": {"displayName": "Ann"},
                                },
                                "oid": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                                "repository": {"name": "example.com/acme/widget"},
                                "subject": "Fix it again",
                            },
                            "matches": [
                                {
                                    "body": {
                                        "text": "```\nm1\nm2\nm3\nm4\nm5\nm6\nm7\nm8\nm9\nm10\nm11\nm12\n```",
                                    },
                                    "highlights": [
                                        {"character": 0, "length": 1, "line": 3},
                                        {"character": 0, "length": 1, "line": 5},
                                    ],
                                },
                            ],
                        },
                    ],
                    "timedout": [],
                },
            },
        },
    });
    assert_snapshot!(answered(
        &["search", "repo:example.com/acme/widget type:commit m"],
        [response]
    ));
}

/// A commit answer under the cap counts its commits, not the highlights the
/// server counted, and is exact.
#[test]
fn commits_exact() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": null,
                    "approximateResultCount": "5",
                    "dynamicFilters": [],
                    "elapsedMilliseconds": 100,
                    "limitHit": false,
                    "matchCount": 5,
                    "results": [
                        {
                            "__typename": "CommitSearchResult",
                            "commit": {
                                "author": {
                                    "date": "2024-05-06T07:08:09Z",
                                    "person": {"displayName": "Ann"},
                                },
                                "oid": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                                "repository": {"name": "example.com/app"},
                                "subject": "fix the parser",
                            },
                            "matches": [
                                {
                                    "body": {"text": "```\nfix the parser\nfix the lexer\n```"},
                                    "highlights": [
                                        {"character": 0, "length": 3, "line": 1},
                                        {"character": 0, "length": 3, "line": 2},
                                    ],
                                },
                            ],
                        },
                        {
                            "__typename": "CommitSearchResult",
                            "commit": {
                                "author": {
                                    "date": "2024-05-06T07:08:09Z",
                                    "person": {"displayName": "Ann"},
                                },
                                "oid": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                                "repository": {"name": "example.com/app"},
                                "subject": "fix the printer",
                            },
                            "matches": [
                                {
                                    "body": {
                                        "text": "```\nfix the printer\nfix the cache\nfix docs\n```",
                                    },
                                    "highlights": [
                                        {"character": 0, "length": 3, "line": 1},
                                        {"character": 0, "length": 3, "line": 2},
                                        {"character": 0, "length": 3, "line": 3},
                                    ],
                                },
                            ],
                        },
                    ],
                    "timedout": [],
                },
            },
        },
    });
    assert_snapshot!(answered(
        &["search", "repo:example.com/app type:commit fix count:30"],
        [response]
    ));
}

/// The JSON form of a content search.
#[test]
fn content_search_json() {
    assert_snapshot!(sego(&[
        "search",
        "--json",
        "repo:^github\\.com/gorilla/mux$@v1.8.0 \"func NewRouter\""
    ]));
}

/// A content search over a pinned tag; count:100 is given; two files match.
#[test]
fn content_search_over_a_tag() {
    assert_snapshot!(sego(&[
        "search",
        "repo:^github\\.com/gorilla/mux$@v1.8.0 \"func NewRouter\" count:100"
    ]));
}

/// count:all cut by the client's clock: the count is a lower bound and the
/// search is stopped.
#[test]
fn count_all_stopped() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": null,
                    "approximateResultCount": "3472",
                    "dynamicFilters": [],
                    "elapsedMilliseconds": 62190,
                    "limitHit": true,
                    "matchCount": 3472,
                    "results": [
                        {
                            "__typename": "FileMatch",
                            "pathMatches": [],
                            "chunkMatches": [
                                {
                                    "content": "foo 0\nfoo 1\nfoo 2",
                                    "contentStart": {"line": 0},
                                    "ranges": [
                                        {
                                            "end": {"character": 3, "line": 0},
                                            "start": {"character": 0, "line": 0},
                                        },
                                        {
                                            "end": {"character": 3, "line": 1},
                                            "start": {"character": 0, "line": 1},
                                        },
                                        {
                                            "end": {"character": 3, "line": 2},
                                            "start": {"character": 0, "line": 2},
                                        },
                                    ],
                                },
                            ],
                            "file": {
                                "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                "path": "a.go",
                            },
                            "repository": {"name": "example.com/app"},
                            "revSpec": null,
                            "symbols": [],
                        },
                        {
                            "__typename": "FileMatch",
                            "pathMatches": [],
                            "chunkMatches": [
                                {
                                    "content": "foo 0\nfoo 1",
                                    "contentStart": {"line": 0},
                                    "ranges": [
                                        {
                                            "end": {"character": 3, "line": 0},
                                            "start": {"character": 0, "line": 0},
                                        },
                                        {
                                            "end": {"character": 3, "line": 1},
                                            "start": {"character": 0, "line": 1},
                                        },
                                    ],
                                },
                            ],
                            "file": {
                                "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                "path": "b.go",
                            },
                            "repository": {"name": "example.com/app"},
                            "revSpec": null,
                            "symbols": [],
                        },
                    ],
                    "timedout": [],
                },
            },
        },
    });
    assert_snapshot!(answered(&["search", "repo:example.com/app foo count:all"], [response]));
}

/// A CR before a line break is not part of the line, in file chunks and in
/// commit messages.
#[test]
fn crlf_lines() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": null,
                    "approximateResultCount": "2",
                    "dynamicFilters": [],
                    "elapsedMilliseconds": 100,
                    "limitHit": false,
                    "matchCount": 2,
                    "results": [
                        {
                            "__typename": "FileMatch",
                            "pathMatches": [],
                            "chunkMatches": [
                                {
                                    "content": "a\r\nb\r\n",
                                    "contentStart": {"line": 0},
                                    "ranges": [
                                        {
                                            "end": {"character": 1, "line": 0},
                                            "start": {"character": 0, "line": 0},
                                        },
                                    ],
                                },
                            ],
                            "file": {
                                "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                "path": "a.go",
                            },
                            "repository": {"name": "example.com/app"},
                            "revSpec": null,
                            "symbols": [],
                        },
                        {
                            "__typename": "CommitSearchResult",
                            "commit": {
                                "author": {
                                    "date": "2024-05-06T07:08:09Z",
                                    "person": {"displayName": "Ann"},
                                },
                                "oid": "0123456789abcdef0123456789abcdef01234567",
                                "repository": {"name": "example.com/app"},
                                "subject": "fix",
                            },
                            "matches": [
                                {
                                    "body": {"text": "```\r\nx\r\n+y\r\n```"},
                                    "highlights": [{"character": 0, "length": 1, "line": 1}],
                                },
                            ],
                        },
                    ],
                    "timedout": [],
                },
            },
        },
    });
    assert_snapshot!(answered(&["search", "repo:example.com/app foo"], [response]));
}

/// count:all with more than 100 matches prints 100 of N.
#[test]
fn default_limit_cuts_at_100_matches() {
    assert_snapshot!(sego(&["search", "repo:^github\\.com/gorilla/mux$@v1.8.0 Route count:all"]));
}

/// A diff answer at the cap is a lower bound in commits.
#[test]
fn diff_commits_at_cap() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": null,
                    "approximateResultCount": "30",
                    "dynamicFilters": [],
                    "elapsedMilliseconds": 400,
                    "limitHit": true,
                    "matchCount": 30,
                    "results": [
                        {
                            "__typename": "CommitSearchResult",
                            "commit": {
                                "author": {
                                    "date": "2024-05-06T07:08:09Z",
                                    "person": {"displayName": "Ann"},
                                },
                                "oid": "0101010101010101010101010101010101010101",
                                "repository": {"name": "example.com/app"},
                                "subject": "change 1",
                            },
                            "matches": [
                                {
                                    "body": {
                                        "text": "```diff\nf1.go f1.go\n@@ -1,1 +1,1 @@\n-old\n+Vars\n```",
                                    },
                                    "highlights": [{"character": 1, "length": 4, "line": 4}],
                                },
                            ],
                        },
                        {
                            "__typename": "CommitSearchResult",
                            "commit": {
                                "author": {
                                    "date": "2024-05-06T07:08:09Z",
                                    "person": {"displayName": "Ann"},
                                },
                                "oid": "0202020202020202020202020202020202020202",
                                "repository": {"name": "example.com/app"},
                                "subject": "change 2",
                            },
                            "matches": [
                                {
                                    "body": {
                                        "text": "```diff\nf2.go f2.go\n@@ -1,1 +1,1 @@\n-old\n+Vars\n```",
                                    },
                                    "highlights": [{"character": 1, "length": 4, "line": 4}],
                                },
                            ],
                        },
                        {
                            "__typename": "CommitSearchResult",
                            "commit": {
                                "author": {
                                    "date": "2024-05-06T07:08:09Z",
                                    "person": {"displayName": "Ann"},
                                },
                                "oid": "0303030303030303030303030303030303030303",
                                "repository": {"name": "example.com/app"},
                                "subject": "change 3",
                            },
                            "matches": [
                                {
                                    "body": {
                                        "text": "```diff\nf3.go f3.go\n@@ -1,1 +1,1 @@\n-old\n+Vars\n```",
                                    },
                                    "highlights": [{"character": 1, "length": 4, "line": 4}],
                                },
                            ],
                        },
                        {
                            "__typename": "CommitSearchResult",
                            "commit": {
                                "author": {
                                    "date": "2024-05-06T07:08:09Z",
                                    "person": {"displayName": "Ann"},
                                },
                                "oid": "0404040404040404040404040404040404040404",
                                "repository": {"name": "example.com/app"},
                                "subject": "change 4",
                            },
                            "matches": [
                                {
                                    "body": {
                                        "text": "```diff\nf4.go f4.go\n@@ -1,1 +1,1 @@\n-old\n+Vars\n```",
                                    },
                                    "highlights": [{"character": 1, "length": 4, "line": 4}],
                                },
                            ],
                        },
                        {
                            "__typename": "CommitSearchResult",
                            "commit": {
                                "author": {
                                    "date": "2024-05-06T07:08:09Z",
                                    "person": {"displayName": "Ann"},
                                },
                                "oid": "0505050505050505050505050505050505050505",
                                "repository": {"name": "example.com/app"},
                                "subject": "change 5",
                            },
                            "matches": [
                                {
                                    "body": {
                                        "text": "```diff\nf5.go f5.go\n@@ -1,1 +1,1 @@\n-old\n+Vars\n```",
                                    },
                                    "highlights": [{"character": 1, "length": 4, "line": 4}],
                                },
                            ],
                        },
                        {
                            "__typename": "CommitSearchResult",
                            "commit": {
                                "author": {
                                    "date": "2024-05-06T07:08:09Z",
                                    "person": {"displayName": "Ann"},
                                },
                                "oid": "0606060606060606060606060606060606060606",
                                "repository": {"name": "example.com/app"},
                                "subject": "change 6",
                            },
                            "matches": [
                                {
                                    "body": {
                                        "text": "```diff\nf6.go f6.go\n@@ -1,1 +1,1 @@\n-old\n+Vars\n```",
                                    },
                                    "highlights": [{"character": 1, "length": 4, "line": 4}],
                                },
                            ],
                        },
                        {
                            "__typename": "CommitSearchResult",
                            "commit": {
                                "author": {
                                    "date": "2024-05-06T07:08:09Z",
                                    "person": {"displayName": "Ann"},
                                },
                                "oid": "0707070707070707070707070707070707070707",
                                "repository": {"name": "example.com/app"},
                                "subject": "change 7",
                            },
                            "matches": [
                                {
                                    "body": {
                                        "text": "```diff\nf7.go f7.go\n@@ -1,1 +1,1 @@\n-old\n+Vars\n```",
                                    },
                                    "highlights": [{"character": 1, "length": 4, "line": 4}],
                                },
                            ],
                        },
                        {
                            "__typename": "CommitSearchResult",
                            "commit": {
                                "author": {
                                    "date": "2024-05-06T07:08:09Z",
                                    "person": {"displayName": "Ann"},
                                },
                                "oid": "0808080808080808080808080808080808080808",
                                "repository": {"name": "example.com/app"},
                                "subject": "change 8",
                            },
                            "matches": [
                                {
                                    "body": {
                                        "text": "```diff\nf8.go f8.go\n@@ -1,1 +1,1 @@\n-old\n+Vars\n```",
                                    },
                                    "highlights": [{"character": 1, "length": 4, "line": 4}],
                                },
                            ],
                        },
                        {
                            "__typename": "CommitSearchResult",
                            "commit": {
                                "author": {
                                    "date": "2024-05-06T07:08:09Z",
                                    "person": {"displayName": "Ann"},
                                },
                                "oid": "0909090909090909090909090909090909090909",
                                "repository": {"name": "example.com/app"},
                                "subject": "change 9",
                            },
                            "matches": [
                                {
                                    "body": {
                                        "text": "```diff\nf9.go f9.go\n@@ -1,1 +1,1 @@\n-old\n+Vars\n```",
                                    },
                                    "highlights": [{"character": 1, "length": 4, "line": 4}],
                                },
                            ],
                        },
                        {
                            "__typename": "CommitSearchResult",
                            "commit": {
                                "author": {
                                    "date": "2024-05-06T07:08:09Z",
                                    "person": {"displayName": "Ann"},
                                },
                                "oid": "0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a",
                                "repository": {"name": "example.com/app"},
                                "subject": "change 10",
                            },
                            "matches": [
                                {
                                    "body": {
                                        "text": "```diff\nf10.go f10.go\n@@ -1,1 +1,1 @@\n-old\n+Vars\n```",
                                    },
                                    "highlights": [{"character": 1, "length": 4, "line": 4}],
                                },
                            ],
                        },
                        {
                            "__typename": "CommitSearchResult",
                            "commit": {
                                "author": {
                                    "date": "2024-05-06T07:08:09Z",
                                    "person": {"displayName": "Ann"},
                                },
                                "oid": "0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b",
                                "repository": {"name": "example.com/app"},
                                "subject": "change 11",
                            },
                            "matches": [
                                {
                                    "body": {
                                        "text": "```diff\nf11.go f11.go\n@@ -1,1 +1,1 @@\n-old\n+Vars\n```",
                                    },
                                    "highlights": [{"character": 1, "length": 4, "line": 4}],
                                },
                            ],
                        },
                        {
                            "__typename": "CommitSearchResult",
                            "commit": {
                                "author": {
                                    "date": "2024-05-06T07:08:09Z",
                                    "person": {"displayName": "Ann"},
                                },
                                "oid": "0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c",
                                "repository": {"name": "example.com/app"},
                                "subject": "change 12",
                            },
                            "matches": [
                                {
                                    "body": {
                                        "text": "```diff\nf12.go f12.go\n@@ -1,1 +1,1 @@\n-old\n+Vars\n```",
                                    },
                                    "highlights": [{"character": 1, "length": 4, "line": 4}],
                                },
                            ],
                        },
                    ],
                    "timedout": [],
                },
            },
        },
    });
    assert_snapshot!(answered(
        &["search", "repo:example.com/app type:diff Vars count:30"],
        [response]
    ));
}

/// A diff answer under the cap counts its commits, not the highlights the
/// server counted, and is exact.
#[test]
fn diff_commits_exact() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": null,
                    "approximateResultCount": "27",
                    "dynamicFilters": [],
                    "elapsedMilliseconds": 400,
                    "limitHit": false,
                    "matchCount": 27,
                    "results": [
                        {
                            "__typename": "CommitSearchResult",
                            "commit": {
                                "author": {
                                    "date": "2024-05-06T07:08:09Z",
                                    "person": {"displayName": "Ann"},
                                },
                                "oid": "0101010101010101010101010101010101010101",
                                "repository": {"name": "example.com/app"},
                                "subject": "change 1",
                            },
                            "matches": [
                                {
                                    "body": {
                                        "text": "```diff\nf1.go f1.go\n@@ -1,1 +1,1 @@\n-old\n+skipClean\n```",
                                    },
                                    "highlights": [{"character": 1, "length": 9, "line": 4}],
                                },
                            ],
                        },
                        {
                            "__typename": "CommitSearchResult",
                            "commit": {
                                "author": {
                                    "date": "2024-05-06T07:08:09Z",
                                    "person": {"displayName": "Ann"},
                                },
                                "oid": "0202020202020202020202020202020202020202",
                                "repository": {"name": "example.com/app"},
                                "subject": "change 2",
                            },
                            "matches": [
                                {
                                    "body": {
                                        "text": "```diff\nf2.go f2.go\n@@ -1,1 +1,1 @@\n-old\n+skipClean\n```",
                                    },
                                    "highlights": [{"character": 1, "length": 9, "line": 4}],
                                },
                            ],
                        },
                        {
                            "__typename": "CommitSearchResult",
                            "commit": {
                                "author": {
                                    "date": "2024-05-06T07:08:09Z",
                                    "person": {"displayName": "Ann"},
                                },
                                "oid": "0303030303030303030303030303030303030303",
                                "repository": {"name": "example.com/app"},
                                "subject": "change 3",
                            },
                            "matches": [
                                {
                                    "body": {
                                        "text": "```diff\nf3.go f3.go\n@@ -1,1 +1,1 @@\n-old\n+skipClean\n```",
                                    },
                                    "highlights": [{"character": 1, "length": 9, "line": 4}],
                                },
                            ],
                        },
                        {
                            "__typename": "CommitSearchResult",
                            "commit": {
                                "author": {
                                    "date": "2024-05-06T07:08:09Z",
                                    "person": {"displayName": "Ann"},
                                },
                                "oid": "0404040404040404040404040404040404040404",
                                "repository": {"name": "example.com/app"},
                                "subject": "change 4",
                            },
                            "matches": [
                                {
                                    "body": {
                                        "text": "```diff\nf4.go f4.go\n@@ -1,1 +1,1 @@\n-old\n+skipClean\n```",
                                    },
                                    "highlights": [{"character": 1, "length": 9, "line": 4}],
                                },
                            ],
                        },
                        {
                            "__typename": "CommitSearchResult",
                            "commit": {
                                "author": {
                                    "date": "2024-05-06T07:08:09Z",
                                    "person": {"displayName": "Ann"},
                                },
                                "oid": "0505050505050505050505050505050505050505",
                                "repository": {"name": "example.com/app"},
                                "subject": "change 5",
                            },
                            "matches": [
                                {
                                    "body": {
                                        "text": "```diff\nf5.go f5.go\n@@ -1,1 +1,1 @@\n-old\n+skipClean\n```",
                                    },
                                    "highlights": [{"character": 1, "length": 9, "line": 4}],
                                },
                            ],
                        },
                        {
                            "__typename": "CommitSearchResult",
                            "commit": {
                                "author": {
                                    "date": "2024-05-06T07:08:09Z",
                                    "person": {"displayName": "Ann"},
                                },
                                "oid": "0606060606060606060606060606060606060606",
                                "repository": {"name": "example.com/app"},
                                "subject": "change 6",
                            },
                            "matches": [
                                {
                                    "body": {
                                        "text": "```diff\nf6.go f6.go\n@@ -1,1 +1,1 @@\n-old\n+skipClean\n```",
                                    },
                                    "highlights": [{"character": 1, "length": 9, "line": 4}],
                                },
                            ],
                        },
                        {
                            "__typename": "CommitSearchResult",
                            "commit": {
                                "author": {
                                    "date": "2024-05-06T07:08:09Z",
                                    "person": {"displayName": "Ann"},
                                },
                                "oid": "0707070707070707070707070707070707070707",
                                "repository": {"name": "example.com/app"},
                                "subject": "change 7",
                            },
                            "matches": [
                                {
                                    "body": {
                                        "text": "```diff\nf7.go f7.go\n@@ -1,1 +1,1 @@\n-old\n+skipClean\n```",
                                    },
                                    "highlights": [{"character": 1, "length": 9, "line": 4}],
                                },
                            ],
                        },
                        {
                            "__typename": "CommitSearchResult",
                            "commit": {
                                "author": {
                                    "date": "2024-05-06T07:08:09Z",
                                    "person": {"displayName": "Ann"},
                                },
                                "oid": "0808080808080808080808080808080808080808",
                                "repository": {"name": "example.com/app"},
                                "subject": "change 8",
                            },
                            "matches": [
                                {
                                    "body": {
                                        "text": "```diff\nf8.go f8.go\n@@ -1,1 +1,1 @@\n-old\n+skipClean\n```",
                                    },
                                    "highlights": [{"character": 1, "length": 9, "line": 4}],
                                },
                            ],
                        },
                    ],
                    "timedout": [],
                },
            },
        },
    });
    assert_snapshot!(answered(
        &["search", "repo:example.com/app type:diff skipClean count:all"],
        [response]
    ));
}

/// A diff answer cut by the clock is a lower bound in commits.
#[test]
fn diff_commits_stopped() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": null,
                    "approximateResultCount": "9",
                    "dynamicFilters": [],
                    "elapsedMilliseconds": 20000,
                    "limitHit": false,
                    "matchCount": 9,
                    "results": [
                        {
                            "__typename": "CommitSearchResult",
                            "commit": {
                                "author": {
                                    "date": "2024-05-06T07:08:09Z",
                                    "person": {"displayName": "Ann"},
                                },
                                "oid": "0101010101010101010101010101010101010101",
                                "repository": {"name": "example.com/app"},
                                "subject": "change 1",
                            },
                            "matches": [
                                {
                                    "body": {
                                        "text": "```diff\nf1.go f1.go\n@@ -1,1 +1,1 @@\n-old\n+Vars\n```",
                                    },
                                    "highlights": [{"character": 1, "length": 4, "line": 4}],
                                },
                            ],
                        },
                        {
                            "__typename": "CommitSearchResult",
                            "commit": {
                                "author": {
                                    "date": "2024-05-06T07:08:09Z",
                                    "person": {"displayName": "Ann"},
                                },
                                "oid": "0202020202020202020202020202020202020202",
                                "repository": {"name": "example.com/app"},
                                "subject": "change 2",
                            },
                            "matches": [
                                {
                                    "body": {
                                        "text": "```diff\nf2.go f2.go\n@@ -1,1 +1,1 @@\n-old\n+Vars\n```",
                                    },
                                    "highlights": [{"character": 1, "length": 4, "line": 4}],
                                },
                            ],
                        },
                        {
                            "__typename": "CommitSearchResult",
                            "commit": {
                                "author": {
                                    "date": "2024-05-06T07:08:09Z",
                                    "person": {"displayName": "Ann"},
                                },
                                "oid": "0303030303030303030303030303030303030303",
                                "repository": {"name": "example.com/app"},
                                "subject": "change 3",
                            },
                            "matches": [
                                {
                                    "body": {
                                        "text": "```diff\nf3.go f3.go\n@@ -1,1 +1,1 @@\n-old\n+Vars\n```",
                                    },
                                    "highlights": [{"character": 1, "length": 4, "line": 4}],
                                },
                            ],
                        },
                    ],
                    "timedout": [],
                },
            },
        },
    });
    assert_snapshot!(answered(
        &["search", "repo:example.com/app type:diff Vars count:all timeout:20s"],
        [response]
    ));
}

/// A zero has nothing to be a lower bound of, diffs or not.
#[test]
fn diff_or_answer_zero() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": null,
                    "approximateResultCount": "0",
                    "dynamicFilters": [],
                    "elapsedMilliseconds": 400,
                    "limitHit": false,
                    "matchCount": 0,
                    "results": [],
                    "timedout": [],
                },
            },
        },
    });
    assert_snapshot!(answered(
        &["search", "(repo:example.com/app type:diff foo) or (repo:example.com/app bar)"],
        [response]
    ));
}

/// A diff search over a revision range prints the hunks.
#[test]
fn diff_search_over_a_range() {
    assert_snapshot!(sego(&[
        "search",
        "repo:^github\\.com/gorilla/mux$@v1.8.0:^v1.7.4 type:diff count:3 route"
    ]));
}

/// A complete repository answer to a diff search counts repositories, not
/// highlights, so it is exact.
#[test]
fn diff_select_repo_exact() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": null,
                    "approximateResultCount": "1",
                    "dynamicFilters": [],
                    "elapsedMilliseconds": 400,
                    "limitHit": false,
                    "matchCount": 1,
                    "results": [{"__typename": "Repository", "name": "example.com/app"}],
                    "timedout": [],
                },
            },
        },
    });
    assert_snapshot!(answered(
        &["search", "repo:^example\\.com/app$ type:diff select:repo foo count:30"],
        [response]
    ));
}

/// A limitHit that came back in time: the count is a lower bound, and the search
/// is not stopped.
#[test]
fn directory_answer_below_cap() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": null,
                    "approximateResultCount": "3",
                    "dynamicFilters": [],
                    "elapsedMilliseconds": 400,
                    "limitHit": true,
                    "matchCount": 3,
                    "results": [
                        {
                            "__typename": "FileMatch",
                            "pathMatches": [],
                            "chunkMatches": [],
                            "file": {
                                "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                "path": "a/",
                            },
                            "repository": {"name": "example.com/app"},
                            "revSpec": null,
                            "symbols": [],
                        },
                        {
                            "__typename": "FileMatch",
                            "pathMatches": [],
                            "chunkMatches": [],
                            "file": {
                                "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                "path": "b/",
                            },
                            "repository": {"name": "example.com/app"},
                            "revSpec": null,
                            "symbols": [],
                        },
                        {
                            "__typename": "FileMatch",
                            "pathMatches": [],
                            "chunkMatches": [],
                            "file": {
                                "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                "path": "c/",
                            },
                            "repository": {"name": "example.com/app"},
                            "revSpec": null,
                            "symbols": [],
                        },
                    ],
                    "timedout": [],
                },
            },
        },
    });
    assert_snapshot!(answered(
        &["search", "repo:example.com/app select:file.directory foo count:30"],
        [response]
    ));
}

/// Four directories under the cap are counted exactly.
#[test]
fn directory_answer_counts_four_directories() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": null,
                    "approximateResultCount": "4",
                    "dynamicFilters": [],
                    "elapsedMilliseconds": 700,
                    "limitHit": false,
                    "matchCount": 4,
                    "results": [
                        {
                            "__typename": "FileMatch",
                            "pathMatches": [],
                            "chunkMatches": [],
                            "file": {
                                "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                "path": "src/",
                            },
                            "repository": {"name": "example.com/acme/widget"},
                            "revSpec": null,
                            "symbols": [],
                        },
                        {
                            "__typename": "FileMatch",
                            "pathMatches": [],
                            "chunkMatches": [],
                            "file": {
                                "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                "path": "src/a/",
                            },
                            "repository": {"name": "example.com/acme/widget"},
                            "revSpec": null,
                            "symbols": [],
                        },
                        {
                            "__typename": "FileMatch",
                            "pathMatches": [],
                            "chunkMatches": [],
                            "file": {
                                "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                "path": "src/b/",
                            },
                            "repository": {"name": "example.com/acme/widget"},
                            "revSpec": null,
                            "symbols": [],
                        },
                        {
                            "__typename": "FileMatch",
                            "pathMatches": [],
                            "chunkMatches": [],
                            "file": {
                                "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                "path": "docs/",
                            },
                            "repository": {"name": "example.com/acme/widget"},
                            "revSpec": null,
                            "symbols": [],
                        },
                    ],
                    "timedout": [],
                },
            },
        },
    });
    assert_snapshot!(answered(
        &["search", "repo:example.com/acme/widget select:file.directory foo count:all"],
        [response]
    ));
}

/// One directory is counted in the singular.
#[test]
fn directory_answer_counts_one_directory() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": null,
                    "approximateResultCount": "1",
                    "dynamicFilters": [],
                    "elapsedMilliseconds": 700,
                    "limitHit": false,
                    "matchCount": 1,
                    "results": [
                        {
                            "__typename": "FileMatch",
                            "pathMatches": [],
                            "chunkMatches": [],
                            "file": {
                                "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                "path": "src/",
                            },
                            "repository": {"name": "example.com/acme/widget"},
                            "revSpec": null,
                            "symbols": [],
                        },
                    ],
                    "timedout": [],
                },
            },
        },
    });
    assert_snapshot!(answered(
        &["search", "repo:example.com/acme/widget select:file.directory foo count:all"],
        [response]
    ));
}

/// A limitHit with no results is not a lower bound of anything.
#[test]
fn directory_zero_below_cap() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": null,
                    "approximateResultCount": "0",
                    "dynamicFilters": [],
                    "elapsedMilliseconds": 400,
                    "limitHit": true,
                    "matchCount": 0,
                    "results": [],
                    "timedout": [],
                },
            },
        },
    });
    assert_snapshot!(answered(
        &["search", "repo:example.com/app select:file.directory foo count:30"],
        [response]
    ));
}

/// An error at the search itself, with no data, fails the search.
/// No recording shows these located errors: a server failure sourcegraph.com doesn't give on demand.
#[test]
fn error_at_the_search_with_no_data() {
    let response = json!({
        "data": {"search": null},
        "errors": [
            {
                "message": "rpc error: code = Unknown desc = ResolveRevision: got bad commit \"^75dcda0896e109a2a22c9315bca3bb21b87b2ba5\" for repo ID 1700 at revision \"^v1.7.4^0\"",
                "path": ["search", "results"],
            },
        ],
    });
    assert_snapshot!(answered(
        &["search", "repo:^github\\.com/gorilla/mux$@v1.8.0:^v1.7.4 NewRouter"],
        [response]
    ));
}

/// An error beneath the search fails it even when results came back.
/// No recording shows these located errors: a server failure sourcegraph.com doesn't give on demand.
#[test]
fn error_in_a_search_result_with_data() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": null,
                    "approximateResultCount": "1",
                    "dynamicFilters": [],
                    "elapsedMilliseconds": 200,
                    "limitHit": false,
                    "matchCount": 1,
                    "results": [
                        {
                            "__typename": "FileMatch",
                            "pathMatches": [],
                            "chunkMatches": [
                                {
                                    "content": "foo",
                                    "contentStart": {"line": 0},
                                    "ranges": [
                                        {
                                            "end": {"character": 3, "line": 0},
                                            "start": {"character": 0, "line": 0},
                                        },
                                    ],
                                },
                            ],
                            "file": {
                                "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                "path": "main.go",
                            },
                            "repository": {"name": "example.com/acme/widget"},
                            "revSpec": null,
                            "symbols": [],
                        },
                    ],
                    "timedout": [],
                },
            },
        },
        "errors": [{"message": "boom", "path": ["search", "results", "results", 0, "file"]}],
    });
    assert_snapshot!(answered(&["search", "repo:example.com/acme/widget foo"], [response]));
}

/// The JSON form of one answer holding every kind of result, with the server's
/// own fields beside sego's.
#[test]
fn every_result_kind_json() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": {
                        "description": null,
                        "kind": "k",
                        "proposedQueries": [{"description": null, "query": "q"}],
                        "title": "T",
                    },
                    "approximateResultCount": "5",
                    "dynamicFilters": [
                        {"count": 3, "kind": "lang", "value": "go"},
                        {"count": 4, "kind": "utility", "value": "fork:yes"},
                        {"count": 9, "kind": "utility", "value": "archived:yes"},
                    ],
                    "elapsedMilliseconds": 12,
                    "limitHit": false,
                    "matchCount": 5,
                    "results": [
                        {
                            "__typename": "FileMatch",
                            "pathMatches": [],
                            "chunkMatches": [
                                {
                                    "content": "x\nfoo",
                                    "contentStart": {"line": 4},
                                    "ranges": [
                                        {
                                            "end": {"character": 3, "line": 5},
                                            "start": {"character": 0, "line": 5},
                                        },
                                    ],
                                },
                            ],
                            "file": {
                                "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                "path": "a.go",
                            },
                            "repository": {"name": "example.com/app"},
                            "revSpec": {"expr": "main"},
                            "symbols": [
                                {
                                    "containerName": null,
                                    "kind": "FUNCTION",
                                    "location": {"range": {"start": {"line": 4}}},
                                    "name": "Foo",
                                },
                            ],
                        },
                        {
                            "__typename": "FileMatch",
                            "pathMatches": [{"start": {"line": 0}}],
                            "chunkMatches": [],
                            "file": {
                                "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                "path": "b.go",
                            },
                            "repository": {"name": "example.com/app"},
                            "revSpec": null,
                            "symbols": [],
                        },
                        {
                            "__typename": "FileMatch",
                            "pathMatches": [],
                            "chunkMatches": [{"content": "a\0b", "contentStart": {"line": 0}, "ranges": []}],
                            "file": {
                                "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                "path": "bin",
                            },
                            "repository": {"name": "example.com/app"},
                            "revSpec": null,
                            "symbols": [],
                        },
                        {
                            "__typename": "CommitSearchResult",
                            "commit": {
                                "author": {
                                    "date": "2024-05-06T07:08:09Z",
                                    "person": {"displayName": "Ann"},
                                },
                                "oid": "c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2",
                                "repository": {"name": "example.com/other"},
                                "subject": "fix",
                            },
                            "matches": [
                                {
                                    "body": {"text": "```diff\nx\n+y\n```"},
                                    "highlights": [{"character": 1, "length": 2, "line": 2}],
                                },
                            ],
                        },
                        {"__typename": "Repository", "name": "example.com/third"},
                    ],
                    "timedout": [{"name": "example.com/slow"}],
                },
            },
        },
    });
    assert_snapshot!(answered(&["search", "--json", "repo:example.com/app foo"], [response]));
}

/// A given count: is sent as written, with nothing added.
#[test]
fn given_count_is_sent_as_written() {
    assert_snapshot!(sego(&["search", "repo:^github\\.com/gorilla/mux$@v1.8.0 Route count:5"]));
}

/// A file's lines and symbols are merged by line, a symbol after the line it
/// starts on, and a symbol with no line comes last.
#[test]
fn lines_and_symbols_by_line_json() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": null,
                    "approximateResultCount": "2",
                    "dynamicFilters": [],
                    "elapsedMilliseconds": 100,
                    "limitHit": false,
                    "matchCount": 2,
                    "results": [
                        {
                            "__typename": "FileMatch",
                            "pathMatches": [],
                            "chunkMatches": [
                                {
                                    "content": "a\nb\nc",
                                    "contentStart": {"line": 0},
                                    "ranges": [
                                        {
                                            "end": {"character": 1, "line": 1},
                                            "start": {"character": 0, "line": 1},
                                        },
                                    ],
                                },
                                {
                                    "content": "z",
                                    "contentStart": {"line": 20},
                                    "ranges": [
                                        {
                                            "end": {"character": 1, "line": 20},
                                            "start": {"character": 0, "line": 20},
                                        },
                                    ],
                                },
                            ],
                            "file": {
                                "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                "path": "a.go",
                            },
                            "repository": {"name": "example.com/app"},
                            "revSpec": null,
                            "symbols": [
                                {
                                    "containerName": null,
                                    "kind": "FUNCTION",
                                    "location": null,
                                    "name": "N",
                                },
                                {
                                    "containerName": null,
                                    "kind": "FUNCTION",
                                    "location": {"range": {"start": {"line": 8}}},
                                    "name": "S",
                                },
                                {
                                    "containerName": null,
                                    "kind": "FUNCTION",
                                    "location": {"range": {"start": {"line": 1}}},
                                    "name": "T",
                                },
                            ],
                        },
                    ],
                    "timedout": [],
                },
            },
        },
    });
    assert_snapshot!(answered(&["search", "--json", "repo:example.com/app foo"], [response]));
}

/// A commit message line past 500 columns is cut to a window around its highlight.
#[test]
fn long_commit_message_line_is_cut_around_its_highlight() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": null,
                    "approximateResultCount": "1",
                    "dynamicFilters": [],
                    "elapsedMilliseconds": 700,
                    "limitHit": false,
                    "matchCount": 1,
                    "results": [
                        {
                            "__typename": "CommitSearchResult",
                            "commit": {
                                "author": {
                                    "date": "2024-05-06T07:08:09Z",
                                    "person": {"displayName": "Ann"},
                                },
                                "oid": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                                "repository": {"name": "example.com/acme/widget"},
                                "subject": "Fix the parser",
                            },
                            "matches": [
                                {
                                    "body": {
                                        "text": "```\nFix the parser\naaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaNEEDLbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\n```",
                                    },
                                    "highlights": [{"character": 1000, "length": 5, "line": 2}],
                                },
                            ],
                        },
                    ],
                    "timedout": [],
                },
            },
        },
    });
    assert_snapshot!(answered(
        &["search", "repo:example.com/acme/widget type:commit NEEDL"],
        [response]
    ));
}

/// A line with two matches is cut around the one that starts first, whatever
/// order the server lists them in.
#[test]
fn long_line_is_cut_around_the_leftmost_match() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": null,
                    "approximateResultCount": "1",
                    "dynamicFilters": [],
                    "elapsedMilliseconds": 700,
                    "limitHit": false,
                    "matchCount": 1,
                    "results": [
                        {
                            "__typename": "FileMatch",
                            "pathMatches": [],
                            "chunkMatches": [
                                {
                                    "content": "aaaaaaaaaaLEFT_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaRIGHTaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                                    "contentStart": {"line": 0},
                                    "ranges": [
                                        {
                                            "end": {"character": 605, "line": 0},
                                            "start": {"character": 600, "line": 0},
                                        },
                                        {
                                            "end": {"character": 15, "line": 0},
                                            "start": {"character": 10, "line": 0},
                                        },
                                    ],
                                },
                            ],
                            "file": {
                                "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                "path": "wide.txt",
                            },
                            "repository": {"name": "example.com/acme/widget"},
                            "revSpec": null,
                            "symbols": [],
                        },
                    ],
                    "timedout": [],
                },
            },
        },
    });
    assert_snapshot!(answered(&["search", "repo:example.com/acme/widget LEFT_ RIGHT"], [response]));
}

/// A line past 500 columns is cut to a window around its match.
#[test]
fn long_line_is_cut_around_the_match() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": null,
                    "approximateResultCount": "1",
                    "dynamicFilters": [],
                    "elapsedMilliseconds": 700,
                    "limitHit": false,
                    "matchCount": 1,
                    "results": [
                        {
                            "__typename": "FileMatch",
                            "pathMatches": [],
                            "chunkMatches": [
                                {
                                    "content": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaNEEDLaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                                    "contentStart": {"line": 0},
                                    "ranges": [
                                        {
                                            "end": {"character": 1505, "line": 0},
                                            "start": {"character": 1500, "line": 0},
                                        },
                                    ],
                                },
                            ],
                            "file": {
                                "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                "path": "long.txt",
                            },
                            "repository": {"name": "example.com/acme/widget"},
                            "revSpec": null,
                            "symbols": [],
                        },
                    ],
                    "timedout": [],
                },
            },
        },
    });
    assert_snapshot!(answered(&["search", "repo:example.com/acme/widget NEEDL"], [response]));
}

/// A query with no timeout: is sent with timeout:60s in front, ahead of every branch of an or.
#[test]
fn longest_deadline_goes_in_front_of_an_or() {
    assert_snapshot!(sego(&[
        "search",
        "(repo:^github\\.com/gorilla/mux$ StrictSlash) or (repo:^github\\.com/gorilla/handlers$ Handler)"
    ]));
}

/// A misspelt filter goes to the server as written, and sego says nothing of it.
#[test]
fn misspelt_filter_goes_to_the_server_as_text() {
    assert_snapshot!(sego(&["search", "repo:^github\\.com/gorilla/mux$@v1.8.0 lnag:go NewRouter"]));
}

/// A query with control characters, given as the argument, is escaped in the error for a search that matches no repository.
#[test]
fn no_repositories_for_query_with_control_characters() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": {
                        "description": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                        "kind": null,
                        "proposedQueries": [
                            {
                                "description": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                                "query": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                            },
                            {
                                "description": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                                "query": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}ix",
                            },
                        ],
                        "title": "No repositories found",
                    },
                    "approximateResultCount": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                    "dynamicFilters": [],
                    "elapsedMilliseconds": 100,
                    "limitHit": false,
                    "matchCount": 0,
                    "results": [],
                    "timedout": [],
                },
            },
        },
    });
    assert_snapshot!(answered(
        &["search", "repo:a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i"],
        [response]
    ));
}

/// No repositories found is an error.
#[test]
fn no_repositories_found() {
    assert_snapshot!(sego(&["search", "repo:^github\\.com/sego-nonexistent-0/nope$ foo"]));
}

/// No repositories found beside results is a note, and the proposal is passed on.
#[test]
fn no_repositories_found_beside_results() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": {
                        "description": "advice",
                        "kind": null,
                        "proposedQueries": [{"description": null, "query": "q1"}],
                        "title": "No repositories found",
                    },
                    "approximateResultCount": "1",
                    "dynamicFilters": [],
                    "elapsedMilliseconds": 100,
                    "limitHit": false,
                    "matchCount": 1,
                    "results": [{"__typename": "Repository", "name": "example.com/acme/widget"}],
                    "timedout": [],
                },
            },
        },
    });
    assert_snapshot!(answered(&["search", "repo:example.com/acme/widget type:repo"], [response]));
}

/// A single proposal from the server follows the error.
#[test]
fn no_repositories_found_with_one_proposal() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": {
                        "description": "advice",
                        "kind": null,
                        "proposedQueries": [{"description": "d1", "query": "q1"}],
                        "title": "No repositories found",
                    },
                    "approximateResultCount": "0",
                    "dynamicFilters": [],
                    "elapsedMilliseconds": 100,
                    "limitHit": false,
                    "matchCount": 0,
                    "results": [],
                    "timedout": [],
                },
            },
        },
    });
    assert_snapshot!(answered(&["search", "repo:example.com/acme/widget foo"], [response]));
}

/// No repositories found is an error; the proposals are facts.
#[test]
fn no_repositories_found_with_proposals() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": {
                        "description": "Nothing matches the repo filter.",
                        "kind": null,
                        "proposedQueries": [
                            {"description": "drop the filter", "query": "foo"},
                            {
                                "description": "widen the filter",
                                "query": "repo:example.com/acme foo",
                            },
                        ],
                        "title": "No repositories found",
                    },
                    "approximateResultCount": "0",
                    "dynamicFilters": [],
                    "elapsedMilliseconds": 100,
                    "limitHit": false,
                    "matchCount": 0,
                    "results": [],
                    "timedout": [],
                },
            },
        },
    });
    assert_snapshot!(answered(&["search", "repo:example.com/acme/widget foo"], [response]));
}

/// Only the first two proposals are facts.
#[test]
fn no_repositories_found_with_three_proposals_names_two() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": {
                        "description": "advice",
                        "kind": null,
                        "proposedQueries": [
                            {"description": "d1", "query": "q1"},
                            {"description": null, "query": "q2"},
                            {"description": null, "query": "q3"},
                        ],
                        "title": "No repositories found",
                    },
                    "approximateResultCount": "0",
                    "dynamicFilters": [],
                    "elapsedMilliseconds": 100,
                    "limitHit": false,
                    "matchCount": 0,
                    "results": [],
                    "timedout": [],
                },
            },
        },
    });
    assert_snapshot!(answered(&["search", "lang:go foo"], [response]));
}

/// No repositories found with no proposals and no repo: filter is the bare error.
#[test]
fn no_repositories_found_without_proposals() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": {
                        "description": "Try using a different filter",
                        "kind": null,
                        "proposedQueries": null,
                        "title": "No repositories found",
                    },
                    "approximateResultCount": "0",
                    "dynamicFilters": [],
                    "elapsedMilliseconds": 100,
                    "limitHit": false,
                    "matchCount": 0,
                    "results": [],
                    "timedout": [],
                },
            },
        },
    });
    assert_snapshot!(answered(&["search", "lang:go foo"], [response]));
}

/// `not` before a repo: filter with a revision negates it, so the search isn't pinned to that revision.
#[test]
fn not_before_a_pinned_repository_negates_it() {
    assert_snapshot!(sego(&["search", "repo:^github\\.com/gorilla/mux$ not repo:fd@v1 zqxv"]));
}

/// A query that names NOT-INDEXED keeps the placeholders, and prints each
/// unindexed file with the index's reason.
#[test]
fn not_indexed_query_lists_unindexed_files() {
    assert_snapshot!(sego(&["search", "repo:^github\\.com/sharkdp/bat$ NOT-INDEXED count:all"]));
}

/// The JSON form gives each unindexed file as an unindexed record with its reason.
#[test]
fn not_indexed_query_lists_unindexed_files_json() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": null,
                    "approximateResultCount": "2",
                    "dynamicFilters": [],
                    "elapsedMilliseconds": 300,
                    "limitHit": false,
                    "matchCount": 2,
                    "results": [
                        {
                            "__typename": "FileMatch",
                            "pathMatches": [],
                            "chunkMatches": [
                                {
                                    "content": "NOT-INDEXED: exceeds the maximum size limit",
                                    "contentStart": {"line": 0},
                                    "ranges": [
                                        {
                                            "end": {"character": 11, "line": 0},
                                            "start": {"character": 0, "line": 0},
                                        },
                                    ],
                                },
                            ],
                            "file": {
                                "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                "path": "vendor/sqlite3.c",
                            },
                            "repository": {"name": "example.com/acme/widget"},
                            "revSpec": null,
                            "symbols": [],
                        },
                        {
                            "__typename": "FileMatch",
                            "pathMatches": [],
                            "chunkMatches": [
                                {
                                    "content": "NOT-INDEXED: contains too many trigrams",
                                    "contentStart": {"line": 0},
                                    "ranges": [
                                        {
                                            "end": {"character": 11, "line": 0},
                                            "start": {"character": 0, "line": 0},
                                        },
                                    ],
                                },
                            ],
                            "file": {
                                "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                "path": "testdata/words.txt",
                            },
                            "repository": {"name": "example.com/acme/widget"},
                            "revSpec": null,
                            "symbols": [],
                        },
                    ],
                    "timedout": [],
                },
            },
        },
    });
    assert_snapshot!(answered(
        &["search", "--json", "repo:example.com/acme/widget NOT-INDEXED count:all"],
        [response]
    ));
}

/// The alert, its proposals and the kinds not shown are notes in that order; a
/// count of one is singular.
#[test]
fn notes_follow_the_alert_then_proposals_then_unshown_kinds() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": {
                        "description": "text",
                        "kind": null,
                        "proposedQueries": [{"description": null, "query": "q"}],
                        "title": "Heads up",
                    },
                    "approximateResultCount": "1",
                    "dynamicFilters": [],
                    "elapsedMilliseconds": 100,
                    "limitHit": false,
                    "matchCount": 1,
                    "results": [
                        {"__typename": "NewKind"},
                        {"__typename": "Repository", "name": "example.com/acme/widget"},
                        {"__typename": "Other"},
                        {"__typename": "NewKind"},
                    ],
                    "timedout": [],
                },
            },
        },
    });
    assert_snapshot!(answered(&["search", "repo:example.com/acme/widget type:repo"], [response]));
}

/// `or` straight after a group's `)` is an operator, so nothing in the query is read as a regex.
#[test]
fn or_after_a_group_is_an_operator() {
    assert_snapshot!(sego(&["search", "(repo:^github\\.com/gorilla/mux$ zqxv)or useState"]));
}

/// `Or(` is an operator, so the query splits into two searches, the second across every repository.
/// Written by hand: the second search is cut, and cut results vary from run to run.
#[test]
fn or_before_a_parenthesis_splits_the_query() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": null,
                    "approximateResultCount": "30+",
                    "dynamicFilters": [{"count": 4, "kind": "utility", "value": "archived:yes"}],
                    "elapsedMilliseconds": 812,
                    "limitHit": true,
                    "matchCount": 30,
                    "results": [
                        {
                            "__typename": "FileMatch",
                            "pathMatches": [],
                            "chunkMatches": [
                                {
                                    "content": "\tr := mux.NewRouter()",
                                    "contentStart": {"line": 11},
                                    "ranges": [
                                        {
                                            "end": {"character": 20, "line": 11},
                                            "start": {"character": 11, "line": 11},
                                        },
                                    ],
                                },
                            ],
                            "file": {
                                "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                "path": "main.go",
                            },
                            "repository": {"name": "example.com/acme/server"},
                            "revSpec": null,
                            "symbols": [],
                        },
                    ],
                    "timedout": [],
                },
            },
        },
    });
    assert_snapshot!(answered(
        &["search", "repo:^github\\.com/gorilla/mux$ zqxv Or(NewRouter)"],
        [response]
    ));
}

/// Chunks that overlap are merged into one, and a chunk apart from them stays
/// separate.
#[test]
fn overlapping_chunks() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": null,
                    "approximateResultCount": "3",
                    "dynamicFilters": [],
                    "elapsedMilliseconds": 100,
                    "limitHit": false,
                    "matchCount": 3,
                    "results": [
                        {
                            "__typename": "FileMatch",
                            "pathMatches": [],
                            "chunkMatches": [
                                {
                                    "content": "c\nd",
                                    "contentStart": {"line": 2},
                                    "ranges": [
                                        {
                                            "end": {"character": 1, "line": 3},
                                            "start": {"character": 0, "line": 3},
                                        },
                                    ],
                                },
                                {
                                    "content": "a\nb\nc",
                                    "contentStart": {"line": 0},
                                    "ranges": [
                                        {
                                            "end": {"character": 1, "line": 1},
                                            "start": {"character": 0, "line": 1},
                                        },
                                    ],
                                },
                                {
                                    "content": "x",
                                    "contentStart": {"line": 9},
                                    "ranges": [
                                        {
                                            "end": {"character": 1, "line": 9},
                                            "start": {"character": 0, "line": 9},
                                        },
                                    ],
                                },
                            ],
                            "file": {
                                "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                "path": "a.go",
                            },
                            "repository": {"name": "example.com/app"},
                            "revSpec": null,
                            "symbols": [],
                        },
                    ],
                    "timedout": [],
                },
            },
        },
    });
    assert_snapshot!(answered(&["search", "repo:example.com/app foo"], [response]));
}

/// A pinned select:file.owners answer takes its count from the server.
#[test]
fn owners_pinned_exact() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": null,
                    "approximateResultCount": "2",
                    "dynamicFilters": [],
                    "elapsedMilliseconds": 400,
                    "limitHit": false,
                    "matchCount": 2,
                    "results": [
                        {
                            "__typename": "FileMatch",
                            "pathMatches": [],
                            "chunkMatches": [],
                            "file": {
                                "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                "path": "a.go",
                            },
                            "repository": {"name": "example.com/app"},
                            "revSpec": {"expr": "v1"},
                            "symbols": [],
                        },
                        {
                            "__typename": "FileMatch",
                            "pathMatches": [],
                            "chunkMatches": [],
                            "file": {
                                "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                "path": "b.go",
                            },
                            "repository": {"name": "example.com/app"},
                            "revSpec": {"expr": "v1"},
                            "symbols": [],
                        },
                    ],
                    "timedout": [],
                },
            },
        },
    });
    assert_snapshot!(answered(
        &["search", "repo:example.com/app@v1 select:file.owners NewRouter"],
        [response]
    ));
}

/// select:file.owners answers a zero at once with an alert.
#[test]
fn owners_zero() {
    assert_snapshot!(sego(&[
        "search",
        "repo:^github\\.com/gorilla/mux$@v1.8.0 select:file.owners NewRouter"
    ]));
}

/// A file whose name matches and whose contents don't is a path result beside the content results.
#[test]
fn path_and_content_results_in_one_answer_json() {
    assert_snapshot!(sego(&[
        "search",
        "--json",
        "repo:^github\\.com/sharkdp/bat$@v0.24.0 syntax_request.md count:all"
    ]));
}

/// A path result is marked as one.
#[test]
fn path_result_is_marked() {
    assert_snapshot!(sego(&[
        "search",
        "repo:^github\\.com/gorilla/mux$@v1.8.0 type:path file:\\.go$"
    ]));
}

/// A pinned select answer takes its count and N+ from the server.
#[test]
fn pinned_select_is_at_least_when_the_server_says_so() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": null,
                    "approximateResultCount": "30",
                    "dynamicFilters": [],
                    "elapsedMilliseconds": 100,
                    "limitHit": true,
                    "matchCount": 30,
                    "results": [
                        {
                            "__typename": "FileMatch",
                            "pathMatches": [],
                            "chunkMatches": [],
                            "file": {
                                "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                "path": "main.go",
                            },
                            "repository": {"name": "example.com/acme/router"},
                            "revSpec": null,
                            "symbols": [],
                        },
                    ],
                    "timedout": [],
                },
            },
        },
    });
    assert_snapshot!(answered(
        &["search", "repo:example.com/acme/router@v1 Router select:file"],
        [response]
    ));
}

/// A stopped answer at a pinned revision has no note on the files it skipped.
#[test]
fn pinned_stopped_answer_has_no_pinned_note() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": null,
                    "approximateResultCount": "1",
                    "dynamicFilters": [],
                    "elapsedMilliseconds": 100,
                    "limitHit": false,
                    "matchCount": 1,
                    "results": [
                        {
                            "__typename": "FileMatch",
                            "pathMatches": [],
                            "chunkMatches": [
                                {
                                    "content": "foo",
                                    "contentStart": {"line": 0},
                                    "ranges": [
                                        {
                                            "end": {"character": 3, "line": 0},
                                            "start": {"character": 0, "line": 0},
                                        },
                                    ],
                                },
                            ],
                            "file": {
                                "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                "path": "main.go",
                            },
                            "repository": {"name": "example.com/acme/router"},
                            "revSpec": null,
                            "symbols": [],
                        },
                    ],
                    "timedout": [{"name": "example.com/acme/router"}],
                },
            },
        },
    });
    assert_snapshot!(answered(&["search", "repo:example.com/acme/router@v1 foo"], [response]));
}

/// A repository that has only a placeholder is not counted among the
/// repositories of the answer.
#[test]
fn placeholder_in_another_repository() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": null,
                    "approximateResultCount": "2",
                    "dynamicFilters": [],
                    "elapsedMilliseconds": 100,
                    "limitHit": false,
                    "matchCount": 2,
                    "results": [
                        {
                            "__typename": "FileMatch",
                            "pathMatches": [],
                            "chunkMatches": [
                                {
                                    "content": "a binary",
                                    "contentStart": {"line": 4},
                                    "ranges": [
                                        {
                                            "end": {"character": 8, "line": 4},
                                            "start": {"character": 2, "line": 4},
                                        },
                                    ],
                                },
                            ],
                            "file": {
                                "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                "path": "a.go",
                            },
                            "repository": {"name": "example.com/b"},
                            "revSpec": null,
                            "symbols": [],
                        },
                        {
                            "__typename": "FileMatch",
                            "pathMatches": [],
                            "chunkMatches": [
                                {
                                    "content": "NOT-INDEXED: exceeds the maximum size limit",
                                    "contentStart": {"line": 0},
                                    "ranges": [
                                        {
                                            "end": {"character": 43, "line": 0},
                                            "start": {"character": 0, "line": 0},
                                        },
                                    ],
                                },
                            ],
                            "file": {
                                "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                "path": "big.bin",
                            },
                            "repository": {"name": "example.com/c"},
                            "revSpec": null,
                            "symbols": [],
                        },
                    ],
                    "timedout": [],
                },
            },
        },
    });
    assert_snapshot!(answered(&["search", "repo:example.com/ binary"], [response]));
}

/// A NOT-INDEXED placeholder is dropped and its range leaves the count.
#[test]
fn placeholder_leaves_the_count() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": null,
                    "approximateResultCount": "3",
                    "dynamicFilters": [],
                    "elapsedMilliseconds": 300,
                    "limitHit": false,
                    "matchCount": 3,
                    "results": [
                        {
                            "__typename": "FileMatch",
                            "pathMatches": [],
                            "chunkMatches": [
                                {
                                    "content": "a binary value\nanother binary",
                                    "contentStart": {"line": 9},
                                    "ranges": [
                                        {
                                            "end": {"character": 8, "line": 9},
                                            "start": {"character": 2, "line": 9},
                                        },
                                        {
                                            "end": {"character": 14, "line": 10},
                                            "start": {"character": 8, "line": 10},
                                        },
                                    ],
                                },
                            ],
                            "file": {
                                "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                "path": "src/real.txt",
                            },
                            "repository": {"name": "example.com/acme/widget"},
                            "revSpec": null,
                            "symbols": [],
                        },
                        {
                            "__typename": "FileMatch",
                            "pathMatches": [],
                            "chunkMatches": [
                                {
                                    "content": "NOT-INDEXED: contains binary content",
                                    "contentStart": {"line": 0},
                                    "ranges": [
                                        {
                                            "end": {"character": 36, "line": 0},
                                            "start": {"character": 0, "line": 0},
                                        },
                                    ],
                                },
                            ],
                            "file": {
                                "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                "path": "data/blob.dat",
                            },
                            "repository": {"name": "example.com/acme/widget"},
                            "revSpec": null,
                            "symbols": [],
                        },
                    ],
                    "timedout": [],
                },
            },
        },
    });
    assert_snapshot!(answered(&["search", "repo:example.com/acme/widget binary"], [response]));
}

/// The JSON form drops the placeholder too.
#[test]
fn placeholder_leaves_the_count_json() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": null,
                    "approximateResultCount": "3",
                    "dynamicFilters": [],
                    "elapsedMilliseconds": 300,
                    "limitHit": false,
                    "matchCount": 3,
                    "results": [
                        {
                            "__typename": "FileMatch",
                            "pathMatches": [],
                            "chunkMatches": [
                                {
                                    "content": "a binary value\nanother binary",
                                    "contentStart": {"line": 9},
                                    "ranges": [
                                        {
                                            "end": {"character": 8, "line": 9},
                                            "start": {"character": 2, "line": 9},
                                        },
                                        {
                                            "end": {"character": 14, "line": 10},
                                            "start": {"character": 8, "line": 10},
                                        },
                                    ],
                                },
                            ],
                            "file": {
                                "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                "path": "src/real.txt",
                            },
                            "repository": {"name": "example.com/acme/widget"},
                            "revSpec": null,
                            "symbols": [],
                        },
                        {
                            "__typename": "FileMatch",
                            "pathMatches": [],
                            "chunkMatches": [
                                {
                                    "content": "NOT-INDEXED: contains binary content",
                                    "contentStart": {"line": 0},
                                    "ranges": [
                                        {
                                            "end": {"character": 36, "line": 0},
                                            "start": {"character": 0, "line": 0},
                                        },
                                    ],
                                },
                            ],
                            "file": {
                                "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                "path": "data/blob.dat",
                            },
                            "repository": {"name": "example.com/acme/widget"},
                            "revSpec": null,
                            "symbols": [],
                        },
                    ],
                    "timedout": [],
                },
            },
        },
    });
    assert_snapshot!(answered(
        &["search", "--json", "repo:example.com/acme/widget binary"],
        [response]
    ));
}

/// Placeholders can't take the count below zero.
#[test]
fn placeholders_beyond_count() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": null,
                    "approximateResultCount": "2",
                    "dynamicFilters": [],
                    "elapsedMilliseconds": 100,
                    "limitHit": false,
                    "matchCount": 2,
                    "results": [
                        {
                            "__typename": "FileMatch",
                            "pathMatches": [],
                            "chunkMatches": [
                                {
                                    "content": "NOT-INDEXED: exceeds the maximum size limit",
                                    "contentStart": {"line": 0},
                                    "ranges": [
                                        {
                                            "end": {"character": 43, "line": 0},
                                            "start": {"character": 0, "line": 0},
                                        },
                                    ],
                                },
                            ],
                            "file": {
                                "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                "path": "big0.bin",
                            },
                            "repository": {"name": "example.com/app"},
                            "revSpec": null,
                            "symbols": [],
                        },
                        {
                            "__typename": "FileMatch",
                            "pathMatches": [],
                            "chunkMatches": [
                                {
                                    "content": "NOT-INDEXED: exceeds the maximum size limit",
                                    "contentStart": {"line": 0},
                                    "ranges": [
                                        {
                                            "end": {"character": 43, "line": 0},
                                            "start": {"character": 0, "line": 0},
                                        },
                                    ],
                                },
                            ],
                            "file": {
                                "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                "path": "big1.bin",
                            },
                            "repository": {"name": "example.com/app"},
                            "revSpec": null,
                            "symbols": [],
                        },
                        {
                            "__typename": "FileMatch",
                            "pathMatches": [],
                            "chunkMatches": [
                                {
                                    "content": "NOT-INDEXED: exceeds the maximum size limit",
                                    "contentStart": {"line": 0},
                                    "ranges": [
                                        {
                                            "end": {"character": 43, "line": 0},
                                            "start": {"character": 0, "line": 0},
                                        },
                                    ],
                                },
                            ],
                            "file": {
                                "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                "path": "big2.bin",
                            },
                            "repository": {"name": "example.com/app"},
                            "revSpec": null,
                            "symbols": [],
                        },
                        {
                            "__typename": "FileMatch",
                            "pathMatches": [],
                            "chunkMatches": [
                                {
                                    "content": "NOT-INDEXED: exceeds the maximum size limit",
                                    "contentStart": {"line": 0},
                                    "ranges": [
                                        {
                                            "end": {"character": 43, "line": 0},
                                            "start": {"character": 0, "line": 0},
                                        },
                                    ],
                                },
                            ],
                            "file": {
                                "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                "path": "big3.bin",
                            },
                            "repository": {"name": "example.com/app"},
                            "revSpec": null,
                            "symbols": [],
                        },
                        {
                            "__typename": "FileMatch",
                            "pathMatches": [],
                            "chunkMatches": [
                                {
                                    "content": "NOT-INDEXED: exceeds the maximum size limit",
                                    "contentStart": {"line": 0},
                                    "ranges": [
                                        {
                                            "end": {"character": 43, "line": 0},
                                            "start": {"character": 0, "line": 0},
                                        },
                                    ],
                                },
                            ],
                            "file": {
                                "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                "path": "big4.bin",
                            },
                            "repository": {"name": "example.com/app"},
                            "revSpec": null,
                            "symbols": [],
                        },
                        {
                            "__typename": "FileMatch",
                            "pathMatches": [],
                            "chunkMatches": [
                                {
                                    "content": "NOT-INDEXED: exceeds the maximum size limit",
                                    "contentStart": {"line": 0},
                                    "ranges": [
                                        {
                                            "end": {"character": 43, "line": 0},
                                            "start": {"character": 0, "line": 0},
                                        },
                                    ],
                                },
                            ],
                            "file": {
                                "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                "path": "big5.bin",
                            },
                            "repository": {"name": "example.com/app"},
                            "revSpec": null,
                            "symbols": [],
                        },
                        {
                            "__typename": "FileMatch",
                            "pathMatches": [],
                            "chunkMatches": [
                                {
                                    "content": "NOT-INDEXED: exceeds the maximum size limit",
                                    "contentStart": {"line": 0},
                                    "ranges": [
                                        {
                                            "end": {"character": 43, "line": 0},
                                            "start": {"character": 0, "line": 0},
                                        },
                                    ],
                                },
                            ],
                            "file": {
                                "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                "path": "big6.bin",
                            },
                            "repository": {"name": "example.com/app"},
                            "revSpec": null,
                            "symbols": [],
                        },
                        {
                            "__typename": "FileMatch",
                            "pathMatches": [],
                            "chunkMatches": [
                                {
                                    "content": "NOT-INDEXED: exceeds the maximum size limit",
                                    "contentStart": {"line": 0},
                                    "ranges": [
                                        {
                                            "end": {"character": 43, "line": 0},
                                            "start": {"character": 0, "line": 0},
                                        },
                                    ],
                                },
                            ],
                            "file": {
                                "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                "path": "big7.bin",
                            },
                            "repository": {"name": "example.com/app"},
                            "revSpec": null,
                            "symbols": [],
                        },
                        {
                            "__typename": "FileMatch",
                            "pathMatches": [],
                            "chunkMatches": [
                                {
                                    "content": "NOT-INDEXED: exceeds the maximum size limit",
                                    "contentStart": {"line": 0},
                                    "ranges": [
                                        {
                                            "end": {"character": 43, "line": 0},
                                            "start": {"character": 0, "line": 0},
                                        },
                                    ],
                                },
                            ],
                            "file": {
                                "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                "path": "big8.bin",
                            },
                            "repository": {"name": "example.com/app"},
                            "revSpec": null,
                            "symbols": [],
                        },
                    ],
                    "timedout": [],
                },
            },
        },
    });
    assert_snapshot!(answered(&["search", "repo:example.com/app foo count:30"], [response]));
}

/// A range that ends far past its chunk is clamped to the chunk's last line.
#[test]
fn range_end_beyond_chunk_json() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": null,
                    "approximateResultCount": "1",
                    "dynamicFilters": [],
                    "elapsedMilliseconds": 100,
                    "limitHit": false,
                    "matchCount": 1,
                    "results": [
                        {
                            "__typename": "FileMatch",
                            "pathMatches": [],
                            "chunkMatches": [
                                {
                                    "content": "ab\ncd",
                                    "contentStart": {"line": 3},
                                    "ranges": [
                                        {
                                            "end": {"character": 7, "line": 4_000_000_000_i64},
                                            "start": {"character": 1, "line": 3},
                                        },
                                    ],
                                },
                            ],
                            "file": {
                                "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                "path": "a.go",
                            },
                            "repository": {"name": "example.com/app"},
                            "revSpec": null,
                            "symbols": [],
                        },
                    ],
                    "timedout": [],
                },
            },
        },
    });
    assert_snapshot!(answered(&["search", "--json", "repo:example.com/app foo"], [response]));
}

/// A range over several lines gives each line its own offsets, and the last one
/// stops at the line's end.
#[test]
fn ranges_across_lines_json() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": null,
                    "approximateResultCount": "1",
                    "dynamicFilters": [],
                    "elapsedMilliseconds": 100,
                    "limitHit": false,
                    "matchCount": 1,
                    "results": [
                        {
                            "__typename": "FileMatch",
                            "pathMatches": [],
                            "chunkMatches": [
                                {
                                    "content": "abcd\nefgh\nij",
                                    "contentStart": {"line": 0},
                                    "ranges": [
                                        {
                                            "end": {"character": 9, "line": 2},
                                            "start": {"character": 1, "line": 0},
                                        },
                                    ],
                                },
                            ],
                            "file": {
                                "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                "path": "a.go",
                            },
                            "repository": {"name": "example.com/app"},
                            "revSpec": null,
                            "symbols": [],
                        },
                    ],
                    "timedout": [],
                },
            },
        },
    });
    assert_snapshot!(answered(&["search", "--json", "repo:example.com/app foo"], [response]));
}

/// A range counts code points, not bytes or UTF-16 units, and one that ends past
/// its line stops at the line's end.
#[test]
fn ranges_in_code_points_json() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": null,
                    "approximateResultCount": "2",
                    "dynamicFilters": [],
                    "elapsedMilliseconds": 100,
                    "limitHit": false,
                    "matchCount": 2,
                    "results": [
                        {
                            "__typename": "FileMatch",
                            "pathMatches": [],
                            "chunkMatches": [
                                {
                                    "content": "🚀 foo",
                                    "contentStart": {"line": 0},
                                    "ranges": [
                                        {
                                            "end": {"character": 5, "line": 0},
                                            "start": {"character": 2, "line": 0},
                                        },
                                    ],
                                },
                                {
                                    "content": "🚀 foo",
                                    "contentStart": {"line": 10},
                                    "ranges": [
                                        {
                                            "end": {"character": 9, "line": 10},
                                            "start": {"character": 2, "line": 10},
                                        },
                                    ],
                                },
                            ],
                            "file": {
                                "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                "path": "a.go",
                            },
                            "repository": {"name": "example.com/app"},
                            "revSpec": null,
                            "symbols": [],
                        },
                    ],
                    "timedout": [],
                },
            },
        },
    });
    assert_snapshot!(answered(&["search", "--json", "repo:example.com/app foo"], [response]));
}

/// A regexp error at search, with no gRPC wrapper and no structural pattern: the
/// reason, no fact.
/// Located errors as in `search::structural_regex_hole`.
#[test]
fn regexp_error() {
    let response = json!({
        "data": {"search": null},
        "errors": [{"message": "error parsing regexp: missing closing ): `(a`", "path": ["search"]}],
    });
    assert_snapshot!(answered(&["search", "repo:example.com/acme/widget@v1 /(a/"], [response]));
}

/// A reason of exactly 200 characters inside whitespace: trimmed, and not cut.
/// Located errors as in `search::structural_regex_hole`.
#[test]
fn regexp_error_boundary() {
    let response = json!({
        "data": {"search": null},
        "errors": [
            {
                "message": "error parsing regexp:   yyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyy   ",
                "path": ["search"],
            },
        ],
    });
    assert_snapshot!(answered(&["search", "repo:example.com/acme/widget@v1 /d/"], [response]));
}

/// A control character in the reason is escaped.
/// Located errors as in `search::structural_regex_hole`.
#[test]
fn regexp_error_escape() {
    let response = json!({
        "data": {"search": null},
        "errors": [{"message": "error parsing regexp: bad \u{1b}[31m", "path": ["search"]}],
    });
    assert_snapshot!(answered(&["search", "repo:example.com/acme/widget@v1 /e/"], [response]));
}

/// A regexp error over two lines: the first line only.
/// Located errors as in `search::structural_regex_hole`.
#[test]
fn regexp_error_first_line() {
    let response = json!({
        "data": {"search": null},
        "errors": [
            {
                "message": "error parsing regexp: missing closing ): `(a`\nsecond line",
                "path": ["search"],
            },
        ],
    });
    assert_snapshot!(answered(&["search", "repo:example.com/acme/widget@v1 /c/"], [response]));
}

/// A regular expression the server can't compile, reported as an alert, is the same usage error as one reported as a GraphQL error.
#[test]
fn regexp_error_in_an_alert() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": {
                        "description": "Error parsing regexp: missing closing ): `(a`",
                        "kind": null,
                        "proposedQueries": null,
                        "title": "Unable To Process Query",
                    },
                    "approximateResultCount": "0",
                    "dynamicFilters": [],
                    "elapsedMilliseconds": 3,
                    "limitHit": false,
                    "matchCount": 0,
                    "results": [],
                    "timedout": [],
                },
            },
        },
    });
    assert_snapshot!(answered(&["search", "repo:example.com/acme/widget /(a/"], [response]));
}

/// A wrapped, two-line, long regexp error: the first line, cut at 200
/// characters.
/// Located errors as in `search::structural_regex_hole`.
#[test]
fn regexp_error_long() {
    let response = json!({
        "data": {"search": null},
        "errors": [
            {
                "message": "rpc error: code = Unknown desc = rpc error: code = Internal desc = error parsing regexp: xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx\nsecond line",
                "path": ["search"],
            },
        ],
    });
    assert_snapshot!(answered(&["search", "repo:example.com/acme/widget@v1 /b/"], [response]));
}

/// A regexp error that isn't the message's own start stays generic.
/// Located errors as in `search::structural_regex_hole`.
#[test]
fn regexp_error_not_first() {
    let response = json!({
        "data": {"search": null},
        "errors": [
            {
                "message": "rpc error: code = Unknown desc = foo: error parsing regexp: x",
                "path": ["search"],
            },
        ],
    });
    assert_snapshot!(answered(&["search", "repo:example.com/acme/widget@v1 /f/"], [response]));
}

/// A query with no pattern, only a repo: filter, counts repositories when the
/// server answers with repository results.
#[test]
fn repo_filter_alone_counts_repositories() {
    assert_snapshot!(sego(&["search", "repo:^github\\.com/gorilla/mux$"]));
}

/// The repositories of an answer are the distinct ones of its results, whatever
/// the kind.
#[test]
fn repositories_across_results() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": null,
                    "approximateResultCount": "5",
                    "dynamicFilters": [],
                    "elapsedMilliseconds": 100,
                    "limitHit": false,
                    "matchCount": 5,
                    "results": [
                        {
                            "__typename": "FileMatch",
                            "pathMatches": [],
                            "chunkMatches": [
                                {
                                    "content": "foo",
                                    "contentStart": {"line": 0},
                                    "ranges": [
                                        {
                                            "end": {"character": 3, "line": 0},
                                            "start": {"character": 0, "line": 0},
                                        },
                                    ],
                                },
                            ],
                            "file": {
                                "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                "path": "a.go",
                            },
                            "repository": {"name": "example.com/a"},
                            "revSpec": null,
                            "symbols": [],
                        },
                        {
                            "__typename": "FileMatch",
                            "pathMatches": [],
                            "chunkMatches": [
                                {
                                    "content": "foo",
                                    "contentStart": {"line": 0},
                                    "ranges": [
                                        {
                                            "end": {"character": 3, "line": 0},
                                            "start": {"character": 0, "line": 0},
                                        },
                                    ],
                                },
                            ],
                            "file": {
                                "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                "path": "b.go",
                            },
                            "repository": {"name": "example.com/a"},
                            "revSpec": null,
                            "symbols": [],
                        },
                        {
                            "__typename": "FileMatch",
                            "pathMatches": [],
                            "chunkMatches": [
                                {
                                    "content": "foo",
                                    "contentStart": {"line": 0},
                                    "ranges": [
                                        {
                                            "end": {"character": 3, "line": 0},
                                            "start": {"character": 0, "line": 0},
                                        },
                                    ],
                                },
                            ],
                            "file": {
                                "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                "path": "b.go",
                            },
                            "repository": {"name": "example.com/z"},
                            "revSpec": null,
                            "symbols": [],
                        },
                        {
                            "__typename": "CommitSearchResult",
                            "commit": {
                                "author": {
                                    "date": "2024-05-06T07:08:09Z",
                                    "person": {"displayName": "Ann"},
                                },
                                "oid": "0123456789abcdef0123456789abcdef01234567",
                                "repository": {"name": "example.com/commits"},
                                "subject": "fix",
                            },
                            "matches": [
                                {
                                    "body": {"text": "```\nfoo\n```"},
                                    "highlights": [{"character": 0, "length": 3, "line": 1}],
                                },
                            ],
                        },
                        {"__typename": "Repository", "name": "example.com/a"},
                    ],
                    "timedout": [],
                },
            },
        },
    });
    assert_snapshot!(answered(&["search", "repo:example.com/ foo"], [response]));
}

/// Three repositories are counted in the plural, with no repository count beside
/// them.
#[test]
fn repository_answer_counts_three_repositories() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": null,
                    "approximateResultCount": "3",
                    "dynamicFilters": [],
                    "elapsedMilliseconds": 700,
                    "limitHit": false,
                    "matchCount": 3,
                    "results": [
                        {"__typename": "Repository", "name": "example.com/acme/widget"},
                        {"__typename": "Repository", "name": "example.com/acme/gadget"},
                        {"__typename": "Repository", "name": "example.com/acme/lib"},
                    ],
                    "timedout": [],
                },
            },
        },
    });
    assert_snapshot!(answered(&["search", "repo:example.com/acme/ type:repo"], [response]));
}

/// A repository search prints the repository name.
#[test]
fn repository_result() {
    assert_snapshot!(sego(&["search", "repo:^github\\.com/gorilla/mux$ type:repo"]));
}

/// A result of a kind sego doesn't select is counted in a note and not printed.
#[test]
fn result_of_an_unknown_kind_is_counted_in_a_note() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": null,
                    "approximateResultCount": "1",
                    "dynamicFilters": [],
                    "elapsedMilliseconds": 100,
                    "limitHit": false,
                    "matchCount": 1,
                    "results": [{"__typename": "Weird"}],
                    "timedout": [],
                },
            },
        },
    });
    assert_snapshot!(answered(&["search", "repo:example.com/acme/widget foo"], [response]));
}

/// A result of a kind sego doesn't select is left out of the JSON form, which
/// has no note for it.
#[test]
fn result_of_an_unknown_kind_json() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": null,
                    "approximateResultCount": "1",
                    "dynamicFilters": [],
                    "elapsedMilliseconds": 100,
                    "limitHit": false,
                    "matchCount": 1,
                    "results": [{"__typename": "Weird"}],
                    "timedout": [],
                },
            },
        },
    });
    assert_snapshot!(answered(&["search", "--json", "repo:example.com/app foo"], [response]));
}

/// A result with no __typename is not an answer the search can read.
#[test]
fn result_without_a_typename() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": null,
                    "approximateResultCount": "1",
                    "dynamicFilters": [],
                    "elapsedMilliseconds": 100,
                    "limitHit": false,
                    "matchCount": 1,
                    "results": [{"name": "example.com/acme/widget"}],
                    "timedout": [],
                },
            },
        },
    });
    assert_snapshot!(answered(&["search", "repo:example.com/acme/widget foo"], [response]));
}

/// A revision the repository lacks is an error.
#[test]
fn revision_missing() {
    assert_snapshot!(sego(&["search", "repo:^github\\.com/gorilla/mux$@v9.9.9 NewRouter"]));
}

/// One repository lacks the revision; results still come back with the alert as
/// a note.
#[test]
fn revision_missing_in_one_repository_is_a_note() {
    assert_snapshot!(sego(&[
        "search",
        "repo:^github\\.com/gorilla/(mux|handlers)$@v1.8.0 \"func NewRouter\" count:5"
    ]));
}

/// A missing revision splits from its repository at the first @.
#[test]
fn revision_with_an_at_sign() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": {
                        "description": "x:\n  * a/b@v@2",
                        "kind": null,
                        "proposedQueries": null,
                        "title": "Some revisions could not be searched",
                    },
                    "approximateResultCount": "0",
                    "dynamicFilters": [],
                    "elapsedMilliseconds": 100,
                    "limitHit": false,
                    "matchCount": 0,
                    "results": [],
                    "timedout": [],
                },
            },
        },
    });
    assert_snapshot!(answered(&["search", "repo:example.com/acme/lib@v1 foo"], [response]));
}

/// Past three missing revisions the last ones are counted.
#[test]
fn revisions_missing_five() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": {
                        "description": "These repository revisions do not exist:\n* r1@v1\n* r2@v2\n* r3@v3\n* r4@v4\n* r5@v5",
                        "kind": null,
                        "proposedQueries": null,
                        "title": "Some revisions could not be searched",
                    },
                    "approximateResultCount": "0",
                    "dynamicFilters": [],
                    "elapsedMilliseconds": 100,
                    "limitHit": false,
                    "matchCount": 0,
                    "results": [],
                    "timedout": [],
                },
            },
        },
    });
    assert_snapshot!(answered(&["search", "repo:example.com/acme/lib@v1 foo"], [response]));
}

/// Three missing revisions are all named.
#[test]
fn revisions_missing_three() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": {
                        "description": "These repository revisions do not exist:\n* r1@v1\n* r2@v2\n* r3@v3",
                        "kind": null,
                        "proposedQueries": null,
                        "title": "Some revisions could not be searched",
                    },
                    "approximateResultCount": "0",
                    "dynamicFilters": [],
                    "elapsedMilliseconds": 100,
                    "limitHit": false,
                    "matchCount": 0,
                    "results": [],
                    "timedout": [],
                },
            },
        },
    });
    assert_snapshot!(answered(&["search", "repo:example.com/acme/lib@v1 foo"], [response]));
}

/// A description with no repository lines is printed as the error.
#[test]
fn revisions_missing_without_lines() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": {
                        "description": "odd text",
                        "kind": null,
                        "proposedQueries": null,
                        "title": "Some revisions could not be searched",
                    },
                    "approximateResultCount": "0",
                    "dynamicFilters": [],
                    "elapsedMilliseconds": 100,
                    "limitHit": false,
                    "matchCount": 0,
                    "results": [],
                    "timedout": [],
                },
            },
        },
    });
    assert_snapshot!(answered(&["search", "repo:example.com/acme/lib@v1 foo"], [response]));
}

/// select:file.directory on a flat repository returns the root as ./.
#[test]
fn select_directory_of_a_flat_repository_is_the_root() {
    assert_snapshot!(sego(&[
        "search",
        "repo:^github\\.com/gorilla/mux$@v1.8.0 select:file.directory NewRouter"
    ]));
}

/// select:file.directory returns directories, and the root as ./.
#[test]
fn select_directory_returns_directories_and_the_root() {
    let ran = sego(&[
        "search",
        "repo:^github\\.com/gorilla/websocket$@v1.5.3 select:file.directory upgrade",
    ]);
    assert!(!stopped(&ran.stdout, &ran.stderr));
    assert_snapshot!(ran);
}

/// A select:file result is a file record in JSON, not a path record.
#[test]
fn select_file_result_in_json() {
    assert_snapshot!(sego(&[
        "search",
        "--json",
        "repo:^github\\.com/gorilla/mux$@v1.8.0 Route select:file count:all"
    ]));
}

/// A select:file result has no lines, and its contents matched, so it isn't marked as a path result.
#[test]
fn select_file_result_is_not_a_path_result() {
    assert_snapshot!(sego(&[
        "search",
        "repo:^github\\.com/gorilla/mux$@v1.8.0 Route select:file count:all"
    ]));
}

/// A real result left after the placeholders keeps a stopped answer from being a
/// zero.
#[test]
fn stopped_answer_with_a_real_result_among_placeholders() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": null,
                    "approximateResultCount": "1",
                    "dynamicFilters": [],
                    "elapsedMilliseconds": 61300,
                    "limitHit": false,
                    "matchCount": 1,
                    "results": [
                        {
                            "__typename": "FileMatch",
                            "pathMatches": [],
                            "chunkMatches": [
                                {
                                    "content": "NOT-INDEXED: exceeds the maximum size limit",
                                    "contentStart": {"line": 0},
                                    "ranges": [
                                        {
                                            "end": {"character": 3, "line": 0},
                                            "start": {"character": 0, "line": 0},
                                        },
                                    ],
                                },
                            ],
                            "file": {
                                "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                "path": "big",
                            },
                            "repository": {"name": "example.com/acme/widget"},
                            "revSpec": null,
                            "symbols": [],
                        },
                        {
                            "__typename": "FileMatch",
                            "pathMatches": [],
                            "chunkMatches": [],
                            "file": {
                                "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                "path": "a.go",
                            },
                            "repository": {"name": "example.com/acme/widget"},
                            "revSpec": null,
                            "symbols": [],
                        },
                    ],
                    "timedout": [{"name": "t"}],
                },
            },
        },
    });
    assert_snapshot!(answered(&["search", "repo:example.com/acme/widget foo"], [response]));
}

/// A limitHit below the cap that came back past the deadline is a cut: the
/// count is a lower bound and the search is stopped.
#[test]
fn stopped_below_cap() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": null,
                    "approximateResultCount": "2",
                    "dynamicFilters": [],
                    "elapsedMilliseconds": 21300,
                    "limitHit": true,
                    "matchCount": 2,
                    "results": [
                        {
                            "__typename": "FileMatch",
                            "pathMatches": [],
                            "chunkMatches": [
                                {
                                    "content": "foo\nbar foo",
                                    "contentStart": {"line": 0},
                                    "ranges": [
                                        {
                                            "end": {"character": 3, "line": 0},
                                            "start": {"character": 0, "line": 0},
                                        },
                                        {
                                            "end": {"character": 7, "line": 1},
                                            "start": {"character": 4, "line": 1},
                                        },
                                    ],
                                },
                            ],
                            "file": {
                                "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                "path": "main.go",
                            },
                            "repository": {"name": "example.com/app"},
                            "revSpec": null,
                            "symbols": [],
                        },
                    ],
                    "timedout": [],
                },
            },
        },
    });
    assert_snapshot!(answered(
        &["search", "repo:example.com/app foo timeout:20s count:30"],
        [response]
    ));
}

/// With timeout:0s every answer arrives past the deadline, so a zero is stopped.
#[test]
fn stopped_past_a_zero_deadline() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": null,
                    "approximateResultCount": "0",
                    "dynamicFilters": [],
                    "elapsedMilliseconds": 0,
                    "limitHit": false,
                    "matchCount": 0,
                    "results": [],
                    "timedout": [],
                },
            },
        },
    });
    assert_snapshot!(answered(
        &["search", "repo:example.com/acme/widget foo timeout:0s"],
        [response]
    ));
}

/// A repository the server stopped searching makes the search stopped, even when
/// it answered in time, and the count a lower bound.
#[test]
fn stopped_within_deadline_with_results() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": null,
                    "approximateResultCount": "5",
                    "dynamicFilters": [],
                    "elapsedMilliseconds": 1200,
                    "limitHit": false,
                    "matchCount": 5,
                    "results": [
                        {
                            "__typename": "FileMatch",
                            "pathMatches": [],
                            "chunkMatches": [
                                {
                                    "content": "foo 0\nfoo 1\nfoo 2\nfoo 3\nfoo 4",
                                    "contentStart": {"line": 0},
                                    "ranges": [
                                        {
                                            "end": {"character": 3, "line": 0},
                                            "start": {"character": 0, "line": 0},
                                        },
                                        {
                                            "end": {"character": 3, "line": 1},
                                            "start": {"character": 0, "line": 1},
                                        },
                                        {
                                            "end": {"character": 3, "line": 2},
                                            "start": {"character": 0, "line": 2},
                                        },
                                        {
                                            "end": {"character": 3, "line": 3},
                                            "start": {"character": 0, "line": 3},
                                        },
                                        {
                                            "end": {"character": 3, "line": 4},
                                            "start": {"character": 0, "line": 4},
                                        },
                                    ],
                                },
                            ],
                            "file": {
                                "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                "path": "main.go",
                            },
                            "repository": {"name": "example.com/app"},
                            "revSpec": null,
                            "symbols": [],
                        },
                    ],
                    "timedout": [{"name": "example.com/app"}],
                },
            },
        },
    });
    assert_snapshot!(answered(&["search", "repo:example.com/app foo count:30"], [response]));
}

/// A zero past the deadline is stopped even when the server says limitHit.
#[test]
fn stopped_zero_when_the_server_cut_the_results() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": null,
                    "approximateResultCount": "0",
                    "dynamicFilters": [],
                    "elapsedMilliseconds": 61300,
                    "limitHit": true,
                    "matchCount": 0,
                    "results": [],
                    "timedout": [],
                },
            },
        },
    });
    assert_snapshot!(answered(&["search", "repo:example.com/acme/widget foo"], [response]));
}

/// Zero results past the deadline with nothing named timed out: an error with no
/// fact.
#[test]
fn stopped_zero_with_no_repository_named() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": null,
                    "approximateResultCount": "0",
                    "dynamicFilters": [],
                    "elapsedMilliseconds": 60009,
                    "limitHit": false,
                    "matchCount": 0,
                    "results": [],
                    "timedout": [],
                },
            },
        },
    });
    assert_snapshot!(answered(&["search", "repo:example.com/acme/widget foo"], [response]));
}

/// Zero results with timed-out repositories: an error that names none of them.
#[test]
fn stopped_zero_with_repositories_the_server_stopped() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": null,
                    "approximateResultCount": "0",
                    "dynamicFilters": [],
                    "elapsedMilliseconds": 1200,
                    "limitHit": false,
                    "matchCount": 0,
                    "results": [],
                    "timedout": [
                        {"name": "example.com/acme/widget"},
                        {"name": "example.com/acme/gadget"},
                        {"name": "example.com/acme/lib"},
                        {"name": "example.com/acme/router"},
                        {"name": "example.com/acme/server"},
                        {"name": "example.com/acme/client"},
                        {"name": "example.com/acme/cli"},
                    ],
                },
            },
        },
    });
    assert_snapshot!(answered(&["search", "repo:example.com/acme/widget foo"], [response]));
}

/// A structural regex hole the server's Go regexp can't compile: the reason, and
/// why.
#[test]
fn structural_regex_hole() {
    assert_snapshot!(sego(&[
        "search",
        "repo:^github\\.com/gorilla/mux$ lang:go patterntype:structural fmt.:[fn~Err(?=orf)\\w+](...)"
    ]));
}

/// A symbol on a line a chunk matched prints after that line, in a file that holds
/// both chunks and symbols.
#[test]
fn symbol_on_a_matched_line() {
    assert_snapshot!(sego(&[
        "search",
        "(repo:^github\\.com/gorilla/mux$@v1.8.0 type:symbol NewRouter) or (repo:^github\\.com/gorilla/mux$@v1.8.0 NewRouter) count:100"
    ]));
}

/// A symbol search prints one row per symbol.
#[test]
fn symbol_result() {
    assert_snapshot!(sego(&[
        "search",
        "repo:^github\\.com/gorilla/mux$@v1.8.0 type:symbol NewRouter"
    ]));
}

/// A symbol search prints a JSON record per symbol, with no path record.
#[test]
fn symbol_result_json() {
    assert_snapshot!(sego(&[
        "search",
        "--json",
        "repo:^github\\.com/gorilla/mux$@v1.8.0 type:symbol NewRouter"
    ]));
}

/// A path that arrives as two results, one of symbols and one of chunks, prints
/// as one group, and a path with neither stays apart.
#[test]
fn symbols_and_chunks_of_one_path() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": null,
                    "approximateResultCount": "2",
                    "dynamicFilters": [],
                    "elapsedMilliseconds": 100,
                    "limitHit": false,
                    "matchCount": 2,
                    "results": [
                        {
                            "__typename": "FileMatch",
                            "pathMatches": [],
                            "chunkMatches": [],
                            "file": {
                                "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                "path": "a.go",
                            },
                            "repository": {"name": "example.com/app"},
                            "revSpec": null,
                            "symbols": [
                                {
                                    "containerName": null,
                                    "kind": "FUNCTION",
                                    "location": {"range": {"start": {"line": 9}}},
                                    "name": "F",
                                },
                            ],
                        },
                        {
                            "__typename": "FileMatch",
                            "pathMatches": [],
                            "chunkMatches": [],
                            "file": {
                                "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                "path": "b.go",
                            },
                            "repository": {"name": "example.com/app"},
                            "revSpec": null,
                            "symbols": [],
                        },
                        {
                            "__typename": "FileMatch",
                            "pathMatches": [],
                            "chunkMatches": [
                                {
                                    "content": "x",
                                    "contentStart": {"line": 2},
                                    "ranges": [
                                        {
                                            "end": {"character": 1, "line": 2},
                                            "start": {"character": 0, "line": 2},
                                        },
                                    ],
                                },
                            ],
                            "file": {
                                "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                "path": "a.go",
                            },
                            "repository": {"name": "example.com/app"},
                            "revSpec": null,
                            "symbols": [
                                {
                                    "containerName": null,
                                    "kind": "FUNCTION",
                                    "location": {"range": {"start": {"line": 1}}},
                                    "name": "E",
                                },
                            ],
                        },
                    ],
                    "timedout": [],
                },
            },
        },
    });
    assert_snapshot!(answered(
        &["search", "(repo:example.com/app type:symbol foo) or (repo:example.com/app foo)"],
        [response]
    ));
}

/// A symbol prints after the chunk line it starts on, between chunks when no
/// chunk holds its line, and a symbol with no line is left out.
#[test]
fn symbols_sit_between_the_chunk_lines_around_them() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": null,
                    "approximateResultCount": "6",
                    "dynamicFilters": [],
                    "elapsedMilliseconds": 700,
                    "limitHit": false,
                    "matchCount": 6,
                    "results": [
                        {
                            "__typename": "FileMatch",
                            "pathMatches": [],
                            "chunkMatches": [
                                {
                                    "content": "a\nfn one() {",
                                    "contentStart": {"line": 8},
                                    "ranges": [
                                        {
                                            "end": {"character": 6, "line": 9},
                                            "start": {"character": 3, "line": 9},
                                        },
                                    ],
                                },
                                {
                                    "content": "b",
                                    "contentStart": {"line": 10},
                                    "ranges": [
                                        {
                                            "end": {"character": 1, "line": 10},
                                            "start": {"character": 0, "line": 10},
                                        },
                                    ],
                                },
                                {
                                    "content": "c\nd\ne\nf",
                                    "contentStart": {"line": 22},
                                    "ranges": [
                                        {
                                            "end": {"character": 1, "line": 24},
                                            "start": {"character": 0, "line": 24},
                                        },
                                    ],
                                },
                                {
                                    "content": "g",
                                    "contentStart": {"line": 29},
                                    "ranges": [
                                        {
                                            "end": {"character": 1, "line": 29},
                                            "start": {"character": 0, "line": 29},
                                        },
                                    ],
                                },
                            ],
                            "file": {
                                "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                "path": "a.rs",
                            },
                            "repository": {"name": "example.com/acme/widget"},
                            "revSpec": null,
                            "symbols": [
                                {
                                    "containerName": null,
                                    "kind": "FUNCTION",
                                    "location": {"range": {"start": {"line": 9}}},
                                    "name": "one",
                                },
                                {
                                    "containerName": null,
                                    "kind": "STRUCT",
                                    "location": {"range": {"start": {"line": 19}}},
                                    "name": "Mid",
                                },
                                {
                                    "containerName": null,
                                    "kind": "FUNCTION",
                                    "location": {"range": {"start": {"line": 24}}},
                                    "name": "inner",
                                },
                                {
                                    "containerName": null,
                                    "kind": "FUNCTION",
                                    "location": {"range": {"start": {"line": 25}}},
                                    "name": "edge",
                                },
                                {
                                    "containerName": null,
                                    "kind": "STRUCT",
                                    "location": null,
                                    "name": "ghost",
                                },
                                {
                                    "containerName": null,
                                    "kind": "STRUCT",
                                    "location": {"range": {"start": {"line": 39}}},
                                    "name": "Tail",
                                },
                            ],
                        },
                    ],
                    "timedout": [],
                },
            },
        },
    });
    assert_snapshot!(answered(
        &[
            "search",
            "(repo:example.com/acme/widget type:symbol one) or (repo:example.com/acme/widget one)"
        ],
        [response]
    ));
}

/// A file whose symbols have no line is a path match.
#[test]
fn symbols_without_line() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": null,
                    "approximateResultCount": "1",
                    "dynamicFilters": [],
                    "elapsedMilliseconds": 100,
                    "limitHit": false,
                    "matchCount": 1,
                    "results": [
                        {
                            "__typename": "FileMatch",
                            "pathMatches": [],
                            "chunkMatches": [],
                            "file": {
                                "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                "path": "a.go",
                            },
                            "repository": {"name": "example.com/app"},
                            "revSpec": null,
                            "symbols": [
                                {
                                    "containerName": null,
                                    "kind": "FUNCTION",
                                    "location": null,
                                    "name": "N",
                                },
                                {
                                    "containerName": null,
                                    "kind": "FUNCTION",
                                    "location": null,
                                    "name": "M",
                                },
                            ],
                        },
                    ],
                    "timedout": [],
                },
            },
        },
    });
    assert_snapshot!(answered(&["search", "repo:example.com/app type:symbol foo"], [response]));
}

/// A file whose symbols have no line is a file record, then its symbols.
#[test]
fn symbols_without_line_json() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": null,
                    "approximateResultCount": "1",
                    "dynamicFilters": [],
                    "elapsedMilliseconds": 100,
                    "limitHit": false,
                    "matchCount": 1,
                    "results": [
                        {
                            "__typename": "FileMatch",
                            "pathMatches": [],
                            "chunkMatches": [],
                            "file": {
                                "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                "path": "a.go",
                            },
                            "repository": {"name": "example.com/app"},
                            "revSpec": null,
                            "symbols": [
                                {
                                    "containerName": null,
                                    "kind": "FUNCTION",
                                    "location": null,
                                    "name": "N",
                                },
                                {
                                    "containerName": null,
                                    "kind": "FUNCTION",
                                    "location": null,
                                    "name": "M",
                                },
                            ],
                        },
                    ],
                    "timedout": [],
                },
            },
        },
    });
    assert_snapshot!(answered(
        &["search", "--json", "repo:example.com/app type:symbol foo"],
        [response]
    ));
}

/// A term starting with - arrives as its own argument and needs no -- before it.
#[test]
fn term_starting_with_a_dash() {
    assert_snapshot!(sego(&[
        "search",
        "repo:^github\\.com/gorilla/mux$@v1.8.0",
        "-file:_test\\.go",
        "NewRouter"
    ]));
}

/// A timeout: filter sets the deadline the elapsed time is judged against.
#[test]
fn timeout_filter_sets_the_deadline() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": null,
                    "approximateResultCount": "0",
                    "dynamicFilters": [],
                    "elapsedMilliseconds": 5004,
                    "limitHit": false,
                    "matchCount": 0,
                    "results": [],
                    "timedout": [],
                },
            },
        },
    });
    assert_snapshot!(answered(
        &["search", "repo:example.com/acme/widget foo timeout:5s"],
        [response]
    ));
}

/// Two revisions give the same path once per commit.
#[test]
fn two_revisions_give_a_path_once_per_commit() {
    assert_snapshot!(sego(&[
        "search",
        "repo:^github\\.com/gorilla/mux$@v1.8.0:v1.7.4 \"func NewRouter\""
    ]));
}

/// An Unable To Process Query alert is an error.
#[test]
fn unable_to_process_query_is_an_error() {
    assert_snapshot!(sego(&["search", "lang:nosuchlang foo"]));
}

/// A description of only whitespace is no description.
#[test]
fn unable_to_process_query_with_blank_description() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": {
                        "description": "  \n ",
                        "kind": null,
                        "proposedQueries": null,
                        "title": "Unable To Process Query",
                    },
                    "approximateResultCount": "0",
                    "dynamicFilters": [],
                    "elapsedMilliseconds": 100,
                    "limitHit": false,
                    "matchCount": 0,
                    "results": [],
                    "timedout": [],
                },
            },
        },
    });
    assert_snapshot!(answered(&["search", "lang:nosuchlang foo"], [response]));
}

/// An Unable To Process Query alert is an error even when results came back.
#[test]
fn unable_to_process_query_with_results_is_an_error() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": {
                        "description": "bad",
                        "kind": null,
                        "proposedQueries": null,
                        "title": "Unable To Process Query",
                    },
                    "approximateResultCount": "1",
                    "dynamicFilters": [],
                    "elapsedMilliseconds": 100,
                    "limitHit": false,
                    "matchCount": 1,
                    "results": [
                        {
                            "__typename": "FileMatch",
                            "pathMatches": [],
                            "chunkMatches": [
                                {
                                    "content": "foo",
                                    "contentStart": {"line": 0},
                                    "ranges": [
                                        {
                                            "end": {"character": 3, "line": 0},
                                            "start": {"character": 0, "line": 0},
                                        },
                                    ],
                                },
                            ],
                            "file": {
                                "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                "path": "main.go",
                            },
                            "repository": {"name": "example.com/acme/widget"},
                            "revSpec": null,
                            "symbols": [],
                        },
                    ],
                    "timedout": [],
                },
            },
        },
    });
    assert_snapshot!(answered(&["search", "lang:nosuchlang foo"], [response]));
}

/// An Unable To Process Query alert with no description is an error titled by the
/// alert.
#[test]
fn unable_to_process_query_without_description() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": {
                        "description": null,
                        "kind": null,
                        "proposedQueries": null,
                        "title": "Unable To Process Query",
                    },
                    "approximateResultCount": "0",
                    "dynamicFilters": [],
                    "elapsedMilliseconds": 100,
                    "limitHit": false,
                    "matchCount": 0,
                    "results": [],
                    "timedout": [],
                },
            },
        },
    });
    assert_snapshot!(answered(&["search", "lang:nosuchlang foo"], [response]));
}

/// The widen line keeps the utility filters only, in the order the server
/// lists them.
#[test]
fn widen_line_keeps_server_order() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": null,
                    "approximateResultCount": "0",
                    "dynamicFilters": [
                        {"count": 2, "kind": "lang", "value": "lang:go"},
                        {"count": 4, "kind": "utility", "value": "fork:yes"},
                        {"count": 68912, "kind": "utility", "value": "archived:yes"},
                    ],
                    "elapsedMilliseconds": 100,
                    "limitHit": false,
                    "matchCount": 0,
                    "results": [],
                    "timedout": [],
                },
            },
        },
    });
    assert_snapshot!(answered(&["search", "repo:example.com/widen foo"], [response]));
}

/// A search scoped by repo: lists the utility filters on a non-zero answer too.
#[test]
fn widen_line_on_a_repo_scoped_answer_with_results() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": null,
                    "approximateResultCount": "1",
                    "dynamicFilters": [
                        {"count": 10, "kind": "utility", "value": "archived:yes"},
                        {"count": 1, "kind": "utility", "value": "fork:yes"},
                        {"count": 1, "kind": "lang", "value": "lang:go"},
                    ],
                    "elapsedMilliseconds": 100,
                    "limitHit": false,
                    "matchCount": 1,
                    "results": [
                        {
                            "__typename": "FileMatch",
                            "pathMatches": [],
                            "chunkMatches": [
                                {
                                    "content": "foo",
                                    "contentStart": {"line": 0},
                                    "ranges": [
                                        {
                                            "end": {"character": 3, "line": 0},
                                            "start": {"character": 0, "line": 0},
                                        },
                                    ],
                                },
                            ],
                            "file": {
                                "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                "path": "main.go",
                            },
                            "repository": {"name": "example.com/acme/widget"},
                            "revSpec": null,
                            "symbols": [],
                        },
                    ],
                    "timedout": [],
                },
            },
        },
    });
    assert_snapshot!(answered(&["search", "repo:example.com/acme/widget foo"], [response]));
}

/// With no results the line lists the utility filters only.
#[test]
fn widen_line_on_a_repo_scoped_zero() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": null,
                    "approximateResultCount": "0",
                    "dynamicFilters": [
                        {"count": 4, "kind": "utility", "value": "archived:yes"},
                        {"count": 9, "kind": "utility", "value": "fork:yes"},
                        {"count": 3, "kind": "lang", "value": "lang:go"},
                    ],
                    "elapsedMilliseconds": 100,
                    "limitHit": false,
                    "matchCount": 0,
                    "results": [],
                    "timedout": [],
                },
            },
        },
    });
    assert_snapshot!(answered(&["search", "repo:example.com/acme/widget foo"], [response]));
}

/// With a repo: filter the widen line is on a stopped answer too.
#[test]
fn widen_line_on_a_stopped_repo_scoped_answer() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": null,
                    "approximateResultCount": "1",
                    "dynamicFilters": [{"count": 2, "kind": "utility", "value": "fork:yes"}],
                    "elapsedMilliseconds": 61300,
                    "limitHit": false,
                    "matchCount": 1,
                    "results": [
                        {
                            "__typename": "FileMatch",
                            "pathMatches": [],
                            "chunkMatches": [
                                {
                                    "content": "foo",
                                    "contentStart": {"line": 0},
                                    "ranges": [
                                        {
                                            "end": {"character": 3, "line": 0},
                                            "start": {"character": 0, "line": 0},
                                        },
                                    ],
                                },
                            ],
                            "file": {
                                "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                "path": "main.go",
                            },
                            "repository": {"name": "example.com/acme/widget"},
                            "revSpec": null,
                            "symbols": [],
                        },
                    ],
                    "timedout": [{"name": "example.com/acme/widget"}],
                },
            },
        },
    });
    assert_snapshot!(answered(&["search", "repo:example.com/acme/widget foo"], [response]));
}

/// With a repo: filter the widen line is on every answer, an at least one too.
#[test]
fn widen_line_on_an_at_least_repo_scoped_answer() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": null,
                    "approximateResultCount": "1",
                    "dynamicFilters": [{"count": 3, "kind": "utility", "value": "archived:yes"}],
                    "elapsedMilliseconds": 100,
                    "limitHit": true,
                    "matchCount": 1,
                    "results": [
                        {
                            "__typename": "FileMatch",
                            "pathMatches": [],
                            "chunkMatches": [
                                {
                                    "content": "foo",
                                    "contentStart": {"line": 0},
                                    "ranges": [
                                        {
                                            "end": {"character": 3, "line": 0},
                                            "start": {"character": 0, "line": 0},
                                        },
                                    ],
                                },
                            ],
                            "file": {
                                "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                "path": "main.go",
                            },
                            "repository": {"name": "example.com/acme/widget"},
                            "revSpec": null,
                            "symbols": [],
                        },
                    ],
                    "timedout": [],
                },
            },
        },
    });
    assert_snapshot!(answered(&["search", "repo:example.com/acme/widget foo count:1"], [response]));
}

/// Without a repo: filter the widen line is for a zero only.
#[test]
fn widen_line_on_an_unscoped_answer_with_results_is_left_out() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": null,
                    "approximateResultCount": "1",
                    "dynamicFilters": [
                        {"count": 3, "kind": "utility", "value": "archived:yes"},
                        {"count": 1, "kind": "lang", "value": "lang:go"},
                    ],
                    "elapsedMilliseconds": 100,
                    "limitHit": false,
                    "matchCount": 1,
                    "results": [
                        {
                            "__typename": "FileMatch",
                            "pathMatches": [],
                            "chunkMatches": [
                                {
                                    "content": "Foo",
                                    "contentStart": {"line": 0},
                                    "ranges": [
                                        {
                                            "end": {"character": 3, "line": 0},
                                            "start": {"character": 0, "line": 0},
                                        },
                                    ],
                                },
                            ],
                            "file": {
                                "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                "path": "main.go",
                            },
                            "repository": {"name": "example.com/acme/widget"},
                            "revSpec": null,
                            "symbols": [],
                        },
                    ],
                    "timedout": [],
                },
            },
        },
    });
    assert_snapshot!(answered(&["search", "lang:go Foo"], [response]));
}

/// A zero with no repo: filter lists the utility filters that would widen the
/// search.
#[test]
fn widen_line_on_an_unscoped_zero_lists_utility_filters() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": null,
                    "approximateResultCount": "0",
                    "dynamicFilters": [
                        {"count": 68912, "kind": "utility", "value": "archived:yes"},
                        {"count": 1, "kind": "utility", "value": "fork:yes"},
                        {"count": 3, "kind": "lang", "value": "lang:go"},
                    ],
                    "elapsedMilliseconds": 700,
                    "limitHit": false,
                    "matchCount": 0,
                    "results": [],
                    "timedout": [],
                },
            },
        },
    });
    assert_snapshot!(answered(&["search", "lang:go Foo"], [response]));
}

/// A count of zero beside a result is no zero, so the widen line stays out.
#[test]
fn widen_line_waits_for_a_real_zero() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": null,
                    "approximateResultCount": "0",
                    "dynamicFilters": [{"count": 4, "kind": "utility", "value": "fork:yes"}],
                    "elapsedMilliseconds": 700,
                    "limitHit": false,
                    "matchCount": 0,
                    "results": [
                        {
                            "__typename": "FileMatch",
                            "pathMatches": [],
                            "chunkMatches": [],
                            "file": {
                                "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                "path": "main.go",
                            },
                            "repository": {"name": "example.com/acme/widget"},
                            "revSpec": null,
                            "symbols": [],
                        },
                    ],
                    "timedout": [],
                },
            },
        },
    });
    assert_snapshot!(answered(&["search", "lang:go Foo"], [response]));
}

/// A zero whose placeholders used up the cap is at least, so the widen line stays
/// out.
#[test]
fn widen_line_waits_for_an_answer_that_is_not_at_least() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": null,
                    "approximateResultCount": "2",
                    "dynamicFilters": [{"count": 4, "kind": "utility", "value": "fork:yes"}],
                    "elapsedMilliseconds": 100,
                    "limitHit": true,
                    "matchCount": 2,
                    "results": [
                        {
                            "__typename": "FileMatch",
                            "pathMatches": [],
                            "chunkMatches": [
                                {
                                    "content": "NOT-INDEXED: exceeds the maximum size limit",
                                    "contentStart": {"line": 0},
                                    "ranges": [
                                        {
                                            "end": {"character": 3, "line": 0},
                                            "start": {"character": 0, "line": 0},
                                        },
                                        {
                                            "end": {"character": 3, "line": 0},
                                            "start": {"character": 0, "line": 0},
                                        },
                                    ],
                                },
                            ],
                            "file": {
                                "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                "path": "big.js",
                            },
                            "repository": {"name": "example.com/acme/widget"},
                            "revSpec": null,
                            "symbols": [],
                        },
                    ],
                    "timedout": [],
                },
            },
        },
    });
    assert_snapshot!(answered(&["search", "lang:javascript Foo count:2"], [response]));
}

/// A filter that would widen the search is escaped in the widen line.
#[test]
fn widen_line_with_control_characters() {
    let response = json!({
        "data": {
            "search": {
                "results": {
                    "alert": null,
                    "approximateResultCount": "0",
                    "dynamicFilters": [
                        {
                            "count": 2,
                            "kind": "utility",
                            "value": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                        },
                    ],
                    "elapsedMilliseconds": 700,
                    "limitHit": false,
                    "matchCount": 0,
                    "results": [],
                    "timedout": [],
                },
            },
        },
    });
    assert_snapshot!(answered(&["search", "lang:go Foo"], [response]));
}
