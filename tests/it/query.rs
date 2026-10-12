use std::time::Duration;

use sego::internal::search::lint::{Token, tokenize};
use sego::internal::search::query::{
    Unit, cap, deadline_ms, duration_text, go_duration, pattern_type, prepare,
    seconds, tenths, unindexed_query, unit,
};

const OVERFLOW: &str = "340282366920938463463374607431.9s";

#[test]
fn facts() {
    let appended = |query: &str| format!("{query} count:100");
    for query in ["NewRouter", "\"count:5\" foo"] {
        let prepared = prepare(query);
        assert_eq!(prepared.sent, appended(query), "{query}");
        assert_eq!(prepared.added, ["count:100"], "{query}");
    }
    for query in ["COUNT:5 foo", "foo (bar count:all)", "-count:5 x"] {
        let prepared = prepare(query);
        assert_eq!(prepared.sent, query);
        assert!(prepared.added.is_empty(), "{query}");
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
    ];
    for (query, expected) in rows {
        assert_eq!(prepare(query).pattern_type, expected, "{query}");
        assert_eq!(pattern_type(&tokenize(query)), expected, "{query}");
    }
}

#[test]
fn deadlines() {
    let rows = [
        ("foo", 60_000),
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

#[test]
fn seconds_text() {
    let rows = [(60_000, "60"), (1_500, "1.5"), (200, "0.2"), (1, "0.001")];
    for (ms, text) in rows {
        assert_eq!(seconds(ms), text);
    }
    assert_eq!(seconds(0), "0");
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
        ("foo SELECT:file.directory", Unit::Files),
        ("type:path foo", Unit::Files),
        (r"repo:x file:\.go$", Unit::Files),
        ("type:symbol Route", Unit::Symbols),
        ("type:symbol type:file Route", Unit::Matches),
        ("repo:x type:commit author:a", Unit::Matches),
        ("repo:x type:diff", Unit::Matches),
        ("type:Symbol foo", Unit::Matches),
        ("foo", Unit::Matches),
        ("\"foo bar\"", Unit::Matches),
        ("/foo/", Unit::Matches),
    ];
    for (query, expected) in rows {
        assert_eq!(unit(&t(query)), expected, "{query}");
    }
    assert_eq!(
        [Unit::Matches, Unit::Symbols, Unit::Files, Unit::Repos]
            .map(|u| (u.plural(), u.singular())),
        [
            ("matches", "match"),
            ("symbols", "symbol"),
            ("files", "file"),
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
    assert_eq!(duration_text(500), "500ms");
    assert_eq!(duration_text(20_000), "20s");
}

#[test]
fn unindexed_query_absent_when_not_eligible() {
    for q in [
        "foo",
        "-repo:x foo",
        "repo:x a or b",
        "repo:x@v1 foo",
        "repo:x -r:y@v1 foo",
        "repo:x rev:v1 foo",
        "repo:x revision:v1 foo",
        "repo:x type:diff foo",
        "repo:x type:commit foo",
        "repo:x type:repo",
        r#"repo:x file:"a b" foo"#,
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
fn prepared_carries_the_facts() {
    let prepared = prepare("repo:x type:diff foo");
    assert_eq!(prepared.query, "repo:x type:diff foo");
    assert_eq!(prepared.cap, Some(100));
    assert_eq!(prepared.unit, Unit::Matches);
    assert!(prepared.diff);
    assert_eq!(prepared.deadline_ms, 60_000);
    assert_eq!(prepared.unindexed, None);

    let prepared = prepare("repo:x file:a$ count:all");
    assert_eq!(prepared.cap, None);
    assert_eq!(prepared.unit, Unit::Files);
    assert!(!prepared.diff);
    assert!(prepared.unindexed.is_some());
}
