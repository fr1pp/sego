use std::time::{Duration, Instant};

use crate::search::lint::{Token, tokenize};
use crate::search::plan::plan;
use crate::search::query::{
    Unit, cap, content_search, deadline_ms, duration_text, go_duration,
    index_no, lower_bound, prepare, select_cut, select_drops, tenths,
    unindexed_query, unit,
};

const OVERFLOW: &str = "340282366920938463463374607431.9s";

#[test]
fn facts() {
    let rows = [
        ("NewRouter", Some(30), 20_000),
        ("\"count:5\" foo", Some(30), 20_000),
        ("COUNT:5 foo", Some(5), 60_000),
        ("foo (bar count:all)", None, 60_000),
        ("-count:5 x", Some(5), 60_000),
    ];
    for (query, cap, deadline) in rows {
        let prepared = prepare(query);
        assert_eq!(prepared.query, query);
        assert_eq!(prepared.cap, cap, "{query}");
        assert_eq!(prepared.deadline_ms, deadline, "{query}");
    }
}

#[test]
fn pattern_types() {
    let rows = [
        ("foo", "keyword"),
        ("patterntype:regexp", "regexp"),
        ("patterntype:regex", "regexp"),
        ("patterntype:Regexp", "keyword"),
        ("patterntype:literal patterntype:structural", "structural"),
        ("patterntype:structural patterntype:bogus", "structural"),
        ("PatternType:lucky", "lucky"),
        ("patterntype:\"regexp\"", "regexp"),
        ("patterntype:'literal'", "literal"),
        ("PATTERNTYPE:'structural' foo(...)", "structural"),
        ("PaTtErNtYpE:regexp", "regexp"),
        ("patterntype:literal foo (bar or baz)", "keyword"),
    ];
    for (query, expected) in rows {
        assert_eq!(prepare(query).pattern_type, expected, "{query}");
    }
}

#[test]
fn deadlines() {
    let rows = [
        ("foo", 20_000),
        ("foo count:5", 60_000),
        ("foo timeout:5s", 5_000),
        ("foo timeout:1m30s", 60_000),
        ("foo timeout:1.5s", 1_500),
        ("foo timeout:1ns", 1),
        ("foo timeout:0s", 0),
        ("foo timeout:-3s", 0),
        ("foo timeout:abc", 60_000),
        ("foo timeout:5", 60_000),
        ("foo timeout:\"5s\"", 5_000),
        ("foo timeout:'5s'", 5_000),
        ("foo timeout:-0s", 0),
        ("foo timeout:2s timeout:9s", 2_000),
    ];
    for (query, ms) in rows {
        assert_eq!(prepare(query).deadline_ms, ms, "{query}");
    }
    assert_eq!(
        prepare(&format!("foo timeout:{OVERFLOW}")).deadline_ms,
        60_000
    );
}

#[test]
fn go_durations() {
    let rows = [
        ("1h2m3.5s", Some(Duration::from_millis(3_723_500))),
        ("300ms", Some(Duration::from_millis(300))),
        ("2µs", Some(Duration::from_micros(2))),
        ("2μs", Some(Duration::from_micros(2))),
        ("0", Some(Duration::ZERO)),
        ("+1s", Some(Duration::from_secs(1))),
        (".5s", Some(Duration::from_millis(500))),
        ("-0", Some(Duration::ZERO)),
        ("-0s", Some(Duration::ZERO)),
        ("", None),
        ("1", None),
        ("-", None),
        ("s", None),
        ("1x", None),
        ("-3s", None),
        ("9223372036854775807ns", Some(Duration::from_nanos(i64::MAX as u64))),
        ("9223372036854775808ns", None),
        (OVERFLOW, None),
        ("999999999999999999999999999999999999999999h", None),
    ];
    for (text, expected) in rows {
        assert_eq!(go_duration(text), expected, "{text:?}");
    }
}

fn t(query: &str) -> Vec<Token> {
    tokenize(query)
}

#[test]
fn cap_in_force() {
    assert_eq!(cap(&t("foo")), Some(30));
    assert_eq!(cap(&t("foo count:5")), Some(5));
    assert_eq!(cap(&t("foo COUNT:all")), None);
    assert_eq!(cap(&t("foo count:5 count:7")), Some(7));
    assert_eq!(cap(&t("foo count:5 count:all")), None);
    assert_eq!(cap(&t("foo count:many")), None);
}

#[test]
fn deadline_defaults_to_20_s() {
    assert_eq!(deadline_ms(&t("foo")), 20_000);
    assert_eq!(deadline_ms(&t("foo count:5")), 60_000);
    assert_eq!(deadline_ms(&t("foo timeout:90s")), 60_000);
    assert_eq!(deadline_ms(&t("foo count:all timeout:5s")), 5_000);
    assert_eq!(deadline_ms(&t("foo timeout:-3s")), 0);
}

#[test]
fn unit_from_the_query() {
    let rows = [
        ("repo:x foo select:repo", Unit::Repos),
        ("foo type:repo", Unit::Repos),
        ("foo SELECT:file.directory", Unit::Directories),
        ("foo select:file.directory", Unit::Directories),
        ("foo select:file", Unit::Files),
        ("foo select:file.path", Unit::Files),
        ("foo select:file.owners", Unit::Files),
        ("foo -select:file.directory", Unit::Matches),
        ("repo:x select:file.directory select:repo", Unit::Repos),
        ("type:path foo select:file.directory", Unit::Directories),
        ("foo select:file select:file.directory", Unit::Directories),
        ("type:path foo", Unit::Files),
        (r"repo:x file:\.go$", Unit::Files),
        ("type:symbol Route", Unit::Symbols),
        ("type:symbol type:file Route", Unit::Matches),
        ("(type:symbol X) or (Y type:symbol X)", Unit::Matches),
        ("repo:x type:commit author:a", Unit::Commits),
        ("repo:x type:diff", Unit::Commits),
        ("repo:x type:diff type:commit foo", Unit::Commits),
        ("repo:x type:diff select:repo", Unit::Repos),
        ("(type:diff foo) or bar", Unit::Matches),
        ("type:diff type:file foo", Unit::Matches),
        ("-type:diff foo", Unit::Matches),
        ("repo:x select:commit foo", Unit::Matches),
        ("type:Symbol foo", Unit::Matches),
        ("-type:symbol foo", Unit::Matches),
        ("-type:path foo", Unit::Matches),
        ("-select:repo foo", Unit::Matches),
        ("foo", Unit::Matches),
        ("\"foo bar\"", Unit::Matches),
        ("/foo/", Unit::Matches),
    ];
    for (query, expected) in rows {
        assert_eq!(unit(&t(query)), expected, "{query}");
    }
    assert_eq!(
        [
            Unit::Matches,
            Unit::Commits,
            Unit::Symbols,
            Unit::Files,
            Unit::Directories,
            Unit::Repos
        ]
        .map(|u| (u.plural(), u.singular())),
        [
            ("matches", "match"),
            ("commits", "commit"),
            ("symbols", "symbol"),
            ("files", "file"),
            ("directories", "directory"),
            ("repos", "repo"),
        ],
    );
}

#[test]
fn unindexed_query_for_an_eligible_query() {
    assert_eq!(
        unindexed_query(
            &t(r"repo:^github\.com/gorilla/mux$ file:\.go$ -lang:markdown foo count:5"),
            60_000,
        )
        .as_deref(),
        Some(
            r"repo:^github\.com/gorilla/mux$ file:\.go$ -lang:markdown type:file select:file index:only patterntype:regexp ^NOT-INDEXED:\ (exceeds\ the\ maximum\ size\ limit|contains\ too\ many\ trigrams)$ count:all timeout:60s"
        ),
    );
    assert_eq!(
        unindexed_query(&t("R:x -F:y foo"), 1_500).as_deref(),
        Some(
            r"R:x -F:y type:file select:file index:only patterntype:regexp ^NOT-INDEXED:\ (exceeds\ the\ maximum\ size\ limit|contains\ too\ many\ trigrams)$ count:all timeout:1500ms"
        ),
    );
    assert_eq!(
        unindexed_query(&t("repo:x g:team foo case:yes content:y"), 20_000)
            .as_deref(),
        Some(
            r"repo:x g:team type:file select:file index:only patterntype:regexp ^NOT-INDEXED:\ (exceeds\ the\ maximum\ size\ limit|contains\ too\ many\ trigrams)$ count:all timeout:20s"
        ),
    );
    assert_eq!(
        unindexed_query(&t("repo:x@HEAD rev:HEAD foo"), 20_000).as_deref(),
        Some(
            r"repo:x@HEAD type:file select:file index:only patterntype:regexp ^NOT-INDEXED:\ (exceeds\ the\ maximum\ size\ limit|contains\ too\ many\ trigrams)$ count:all timeout:20s"
        ),
    );
    assert_eq!(duration_text(500), "500ms");
    assert_eq!(duration_text(20_000), "20s");
}

#[test]
fn unindexed_query_absent_when_not_eligible() {
    for q in [
        "foo",
        "-repo:x foo",
        "repo:x a or b",
        "repo:x NOT file:test foo",
        "NOT repo:x repo:y foo",
        "repo:x@v1 foo",
        "repo:x@HEAD:v1 foo",
        "repo:x -r:y@v1 foo",
        "repo:x -r:y@HEAD foo",
        "repo:x rev:v1 foo",
        "repo:x revision:v1 foo",
        "repo:x type:diff foo",
        "repo:x type:commit foo",
        "repo:x type:repo",
        r#"repo:x file:"a b" foo"#,
        r"repo:x repohasfile:go\.mod foo",
        "repo:x repohascommitafter:2024 foo",
        "repo:x author:a foo",
        "repo:x file:has.content(x) foo",
        "repo:x path:contains.file(a) foo",
        r"repo:x file:\.go$",
        "repo:x type:path foo",
    ] {
        assert_eq!(unindexed_query(&t(q), 20_000), None, "{q}");
    }
}

#[test]
fn tenths_rounds_to_one_decimal() {
    assert_eq!(tenths(1449), "1.4");
    assert_eq!(tenths(21_300), "21.3");
    assert_eq!(tenths(15), "0.0");
    assert_eq!(tenths(50), "0.1");
}

#[test]
fn lower_bound_queries() {
    let rows = [
        ("repo:x type:diff foo", false),
        ("repo:x SELECT:file.directory foo", true),
        ("repo:x select:file.directory", true),
        ("repo:x select:file.directory foo count:30", true),
        ("repo:x select:file.directory foo count:100", true),
        ("repo:x select:file.directory foo count:5", true),
        ("repo:x select:file.directory foo count:all", false),
        ("repo:x select:file.directory foo COUNT:ALL", false),
        ("repo:x select:file.directory foo count:all count:30", true),
        ("repo:x select:file foo", false),
        ("repo:x select:file.owners foo", false),
        ("repo:x type:commit foo", false),
        ("repo:x -type:diff foo", false),
        ("repo:x -select:file.directory foo", false),
        ("repo:x foo", false),
        ("foo select:repo", true),
        ("repo:x foo select:repo", true),
        (r"repo:^github\.com/gorilla/ foo select:repo", true),
        (r"repo:^github\.com/gorilla/mux$ foo select:repo", false),
        (r"repo:^github\.com/gorilla/mux$ foo -select:repo", false),
        (r"repo:has.file(path:go\.mod) foo select:repo", true),
        ("type:repo foo", false),
        // the server reports a cut history search
        (r"repo:^github\.com/gorilla/ type:commit select:repo", false),
        ("repo:x type:diff foo select:repo", false),
        ("repo:x type:commit type:diff foo select:repo", false),
        ("repo:x type:commit type:file foo select:repo", true),
        ("repo:x -type:commit foo select:repo", true),
        ("repo:x type:commit foo or bar select:repo", true),
    ];
    for (query, expected) in rows {
        assert_eq!(prepare(query).lower_bound, expected, "{query}");
        assert_eq!(
            lower_bound(&tokenize(query), &plan(query)),
            expected,
            "{query}"
        );
    }
}

#[test]
fn select_cut_queries() {
    let rows = [
        ("repo:x@v1 Router select:file", Some(30)),
        ("repo:x@v1 type:symbol /./ select:symbol.method count:50", Some(50)),
        ("repo:x rev:v1 foo select:repo", None),
        ("repo:x@v1 foo select:repo.something", None),
        ("repo:x@v1 foo select:repo select:file", Some(30)),
        ("repo:x@HEAD foo select:file", None),
        ("repo:x rev:HEAD foo select:file", None),
        ("repo:x revision:HEAD foo select:file", None),
        ("repo:x@HEAD:v1 foo select:file", Some(30)),
        ("repo:x@main foo select:file", Some(30)),
        ("repo:x@head foo select:file", Some(30)),
        ("repo:x@v1 foo select:file.directory count:5", Some(5)),
        ("repo:x@v1 foo SELECT:file", Some(30)),
        ("repo:x@v1 foo select:file count:all", None),
        ("repo:x foo select:file", None),
        ("repo:x -rev:v1 foo select:file", None),
        ("repo:x -r:y@v1 foo select:file", None),
        ("repo:x@v1 foo -select:file", None),
        ("repo:x@v1 foo select:file.owners", None),
        ("repo:x@v1 foo select:file.owners count:50", None),
        ("repo:x@v1 foo", None),
        ("repo:x@v1 select:file", None),
        ("repo:x@v1 type:commit foo select:file", None),
    ];
    for (query, expected) in rows {
        assert_eq!(prepare(query).select_cut, expected, "{query}");
        assert_eq!(select_cut(&tokenize(query)), expected, "{query}");
    }
}

#[test]
fn select_drops_queries() {
    let rows = [
        ("repo:x@v1 type:symbol foo select:symbol", true),
        ("repo:x@v1 type:symbol foo select:symbol.method", true),
        ("repo:x@v1 foo select:content", true),
        ("repo:x@v1 foo select:file", false),
        ("repo:x@v1 foo select:file.directory", false),
        ("repo:x@v1 foo select:file.owners", false),
        ("repo:x@v1 foo select:repo", false),
        ("repo:x@v1 foo select:content count:all", false),
        ("repo:x@HEAD foo select:content", false),
        ("repo:x foo select:content", false),
        ("repo:x@v1 foo -select:content", false),
    ];
    for (query, expected) in rows {
        assert_eq!(prepare(query).select_drops, expected, "{query}");
        assert_eq!(select_drops(&tokenize(query)), expected, "{query}");
    }
}

#[test]
fn prepared_carries_the_facts() {
    let prepared = prepare("repo:x type:diff foo");
    assert_eq!(prepared.query, "repo:x type:diff foo");
    assert_eq!(prepared.cap, Some(30));
    assert_eq!(prepared.unit, Unit::Commits);
    assert!(!prepared.lower_bound);
    assert!(prepared.diff);
    assert_eq!(prepared.deadline_ms, 20_000);
    assert_eq!(prepared.unindexed, None);

    let prepared = prepare("repo:x foo select:file count:all");
    assert_eq!(prepared.cap, None);
    assert_eq!(prepared.unit, Unit::Files);
    assert!(!prepared.lower_bound);
    assert!(!prepared.diff);
    assert!(prepared.unindexed.is_some());
}

#[test]
fn pinned_searches() {
    for q in ["repo:x@v1 foo", "repo:x rev:v1 foo", "repo:x revision:v1 foo"] {
        let prepared = prepare(q);
        assert!(prepared.pinned, "{q}");
        assert_eq!(prepared.unindexed, None, "{q}");
    }
    for q in [
        "repo:x foo",
        "repo:x@HEAD foo",
        "repo:x@ foo",
        "repo:x rev:HEAD foo",
        "repo:x revision:HEAD foo",
    ] {
        let prepared = prepare(q);
        assert!(!prepared.pinned, "{q}");
        assert!(prepared.unindexed.is_some(), "{q}");
    }
    for q in [
        "repo:x@HEAD:v1 foo",
        "repo:x@HEAD rev:v1 foo",
        "repo:x@head foo",
        "repo:x@main foo",
    ] {
        let prepared = prepare(q);
        assert!(prepared.pinned, "{q}");
        assert_eq!(prepared.unindexed, None, "{q}");
    }
    for q in [
        "repo:x@v1 a or b",
        r"repo:^github\.com/gorilla/mux$@v1.8.0 file:^mux\.go$ (StrictSlash or zzqqxx)",
        "repo:x rev:v1 NOT foo",
        "repo:x@v1 repohasfile:y foo",
        "repo:x@v1 type:file foo",
        "repo:x@v1 type:symbol foo",
        "repo:x rev:v1 type:symbol type:file foo",
    ] {
        let prepared = prepare(q);
        assert!(prepared.pinned, "{q}");
        assert_eq!(prepared.unindexed, None, "{q}");
    }
    for q in [
        "repo:x@v1",
        "repo:x@v1 type:path foo",
        "repo:x@v1 type:bogus foo",
        "repo:x@v1 type:commit foo",
        "repo:x@v1 type:diff foo",
        "repo:x@v1 type:repo foo",
    ] {
        assert!(!prepare(q).pinned, "{q}");
    }
    for q in
        ["repo:x -r:y@v1 foo", "repo:x -rev:v1 foo", "repo:x -rev:v1 a or b"]
    {
        let prepared = prepare(q);
        assert!(!prepared.pinned, "{q}");
        assert_eq!(prepared.unindexed, None, "{q}");
    }
}

#[test]
fn repo_scoped_queries() {
    for q in ["repo:x foo", "r:x foo", r"repo:^github\.com/org/ file:a foo"] {
        assert!(prepare(q).repo_scoped, "{q}");
    }
    for q in ["foo", "-repo:x foo", "file:x foo", "lang:go foo"] {
        assert!(!prepare(q).repo_scoped, "{q}");
    }
}

#[test]
fn repo_names_queries() {
    let rows = [
        (
            r#"repo:^github\.com/jackc/pgx/v5$@v5.5.5 -file:_test\.go$ "conn busy" timeout:60s"#,
            Some("repo:pgx type:repo"),
        ),
        (r"repo:^github.com/jackc/pgconn$ foo", Some("repo:pgconn type:repo")),
        (
            r"repo:^github\.com/expressjs/security-wg$ foo",
            Some("repo:security-wg type:repo"),
        ),
        ("repo:github.com/jackc/pgx/v5 foo", Some("repo:pgx type:repo")),
        ("repo:jackc/pgx/v5 foo", Some("repo:pgx type:repo")),
        ("r:jackc/pgx foo", Some("repo:pgx type:repo")),
        ("REPO:jackc/pgx foo", Some("repo:pgx type:repo")),
        (r#"repo:"^github\.com/jackc/pgx$" foo"#, Some("repo:pgx type:repo")),
        (r"repo:^github\.com/jackc/ foo", Some("repo:jackc type:repo")),
        ("repo:^pgx$ foo", Some("repo:pgx type:repo")),
        (r"repo:^github\.com/a/b\.js$ foo", Some(r"repo:b\.js type:repo")),
        ("repo:a/b -repo:c/d foo", Some("repo:b type:repo")),
        ("repo:jackc/pgx@v5.5.5 foo", Some("repo:pgx type:repo")),
        (r"repo:^github\.com/a/b\-c$ foo", Some(r"repo:b\-c type:repo")),
        (
            r"repo:^github\.com/jackc/pgx$ type:repo",
            Some("repo:pgx type:repo"),
        ),
        // the search would be the value itself
        ("repo:pgx foo", None),
        ("repo:pgx@v1 foo", None),
        ("repo:pgx type:repo", None),
        // no one positive repo: value
        ("foo", None),
        ("-repo:a/b foo", None),
        ("repo:a/b repo:c/d foo", None),
        ("repo:a/b foo or repo:c/d bar", None),
        // the name isn't literal
        (r"repo:^github\.com/gorilla/(mux|handlers)$ foo", None),
        (r"repo:^github\.com/ foo", None),
        ("repo:has.file(path:go.mod) foo", None),
        ("repo:contains.file(path:a/b) foo", None),
        ("repo:has.file(path:src/a/b/c.go) foo", None),
    ];
    for (q, expected) in rows {
        assert_eq!(prepare(q).repo_names.as_deref(), expected, "{q}");
    }
}

#[test]
fn content_searches() {
    for q in [
        "foo",
        "repo:x foo",
        "repo:x a or b",
        "repo:x NOT foo",
        "\"foo bar\"",
        "/foo/",
        "repo:x type:file foo",
        "repo:x -type:symbol foo",
        "repo:x foo select:repo",
        "repo:x@v1 (a or b)",
        "type:symbol foo",
        "repo:x type:symbol foo",
        "type:symbol type:file foo",
        "(type:symbol X) or (Y type:symbol X)",
    ] {
        assert!(content_search(&t(q)), "{q}");
        assert!(prepare(q).content_search, "{q}");
    }
    for q in [
        "repo:x",
        r"repo:x file:\.go$",
        "repo:x type:path foo",
        "repo:x type:symbol type:path foo",
        "repo:x type:bogus foo",
        "repo:x type:Symbol foo",
        "repo:x type:repo foo",
        "repo:x type:commit foo",
        "repo:x type:diff foo",
    ] {
        assert!(!content_search(&t(q)), "{q}");
        assert!(!prepare(q).content_search, "{q}");
    }
}

#[test]
fn index_no_queries() {
    for value in
        ["n", "N", "no", "NO", "No", "0", "f", "F", "false", "FALSE", "False"]
    {
        let q = format!("repo:x index:{value} foo");
        assert!(index_no(&t(&q), &plan(&q)), "{q}");
        assert!(prepare(&q).index_no, "{q}");
    }
    let deep =
        format!("{}repo:x foo{} index:no", "(".repeat(65), ")".repeat(65));
    let wide = format!("{} index:no", vec!["(repo:x foo)"; 64].join(" or "));
    for q in [deep.as_str(), wide.as_str()] {
        assert!(index_no(&t(q), &plan(q)), "{q}");
    }
    for q in [
        "repo:x INDEX:no foo",
        "(repo:x index:no foo) or (repo:y index:no bar)",
        "repo:x index:no foo or bar",
    ] {
        assert!(index_no(&t(q), &plan(q)), "{q}");
    }
    for q in [
        "(repo:x index:no foo) or (repo:y bar)",
        "(repo:x foo) or (repo:y index:no bar)",
        "repo:x foo",
        "repo:x index:yes foo",
        "repo:x index:only foo",
        "repo:x index:true foo",
        "repo:x index:1 foo",
        "repo:x index:nO foo",
        "repo:x index:fALSE foo",
        "repo:x index:off foo",
        "repo:x -index:no foo",
    ] {
        assert!(!index_no(&t(q), &plan(q)), "{q}");
        assert!(!prepare(q).index_no, "{q}");
    }
}

/// The least time of three that `prepare` takes for a query of `bytes`:
/// `head`, then repeats of `unit`, then `tail` repeated as often.
fn preparing(head: &str, unit: &str, tail: &str, bytes: usize) -> Duration {
    let units = bytes / (unit.len() + tail.len());
    let query = format!("{head}{}{}", unit.repeat(units), tail.repeat(units));
    let time = || {
        let started = Instant::now();
        prepare(&query);
        started.elapsed()
    };
    (0..3).map(|_| time()).min().unwrap_or_default()
}

/// 64 KiB of each shape takes at most a few times sixteen times what 4 KiB
/// does: time that grows with the square of the length would take some 256
/// times as long. The ratio, not the time, is checked, so a loaded machine
/// doesn't fail it.
#[test]
fn prepares_in_linear_time() {
    let nests = [
        "(f:x a ",
        "(a f:x ",
        "(f:x a b ",
        "(-f:x a ",
        "(not f:x a ",
        "(content:x a ",
    ];
    let mut shapes: Vec<(&str, &str, &str)> = vec![
        ("", "f:a ", ""),
        ("", "\"a ", ""),
        ("", "a(b ", ""),
        ("", "foo or ", ""),
        ("", "repo:x foo or bar ", ""),
        ("", "(repo:a or repo:b) ", ""),
        ("patterntype:standard ", "( a", ""),
        ("", "(a)or x ", ""),
        ("", "(a b ", ""),
        ("", "a or (", ""),
        ("", "x and (a ", ""),
        ("", "(", ""),
    ];
    for unit in nests {
        shapes.push(("", unit, ")"));
    }
    for head in [
        "patterntype:regexp ",
        "patterntype:literal ",
        "patterntype:standard ",
        "patterntype:structural ",
    ] {
        shapes.push((head, "(f:x a ", ")"));
    }
    for (head, unit, tail) in shapes {
        let small = preparing(head, unit, tail, 4 << 10);
        let large = preparing(head, unit, tail, 64 << 10);
        assert!(
            large < small * 48 + Duration::from_millis(20),
            "{head}{unit:?}: {large:?}, against {small:?} for a sixteenth"
        );
    }
}
