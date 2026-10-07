use std::time::Duration;

use sego::internal::search::lint::tokenize;
use sego::internal::search::query::{
    go_duration, pattern_type, prepare, seconds,
};

const OVERFLOW: &str = "340282366920938463463374607431.9s";

#[test]
fn facts() {
    let appended = |query: &str| format!("{query} count:100");
    for query in ["NewRouter", "\"count:5\" foo"] {
        let prepared = prepare(query);
        assert_eq!(prepared.sent, appended(query), "{query}");
        assert_eq!(prepared.added, ["count:100"], "{query}");
        assert_eq!(prepared.tokens, tokenize(&prepared.sent), "{query}");
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
