use std::time::Duration;

use crate::sourcegraph::NOT_INDEXED;

use super::read::{
    PatternType, Written, WrittenFilter as Filter, chosen, has_pattern, read, unquoted,
};

const MAX_DEADLINE_MS: u64 = 60_000;
const DEFAULT_DEADLINE_MS: u64 = 20_000;
const DEFAULT_CAP: u64 = 30;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    Matches,
    Commits,
    Symbols,
    Files,
    Directories,
    Repos,
}

impl Unit {
    pub fn plural(self) -> &'static str {
        match self {
            Unit::Matches => "matches",
            Unit::Commits => "commits",
            Unit::Symbols => "symbols",
            Unit::Files => "files",
            Unit::Directories => "directories",
            Unit::Repos => "repos",
        }
    }

    pub fn singular(self) -> &'static str {
        match self {
            Unit::Matches => "match",
            Unit::Commits => "commit",
            Unit::Symbols => "symbol",
            Unit::Files => "file",
            Unit::Directories => "directory",
            Unit::Repos => "repo",
        }
    }
}

#[expect(
    clippy::struct_excessive_bools,
    reason = "independent facts about one query, not a state machine"
)]
pub struct Query {
    /// The query as sent: as written, with the longest deadline in front
    /// when it sets none (ADR 0017).
    pub text: String,
    pub pattern_type: PatternType,
    /// The query names `lucky`, which the server reads as `standard`.
    lucky: bool,
    pub cap: Option<u64>,
    pub selects_files: bool,
    pub unit: Unit,
    /// The query states no unit, so the results decide it: `Repos` when
    /// every result is a repository, `Commits` when every result is a
    /// commit, and `unit` otherwise.
    pub by_results: bool,
    pub deadline_ms: u64,
    pub repo_scoped: bool,
    /// A pattern names `NOT-INDEXED`, so the placeholders of unindexed
    /// files are results, not matches to drop.
    pub lists_placeholders: bool,
}

impl Query {
    /// The query as sent, as an error's fact: a shell can change a query
    /// before sego sees it, and this shows what reached the server.
    pub fn sent(&self) -> String {
        format!("query  {}", self.text)
    }

    /// The facts about `text` that what it writes gives. A query the server
    /// rejects while reading it gives none.
    pub fn new(written: &str) -> Query {
        let text = sent(written);
        let reading = read(&text);
        let written = reading.written.as_slice();
        let deadline_ms = deadline_ms(written);
        let pattern_type = reading.pattern_type;
        Query {
            text,
            pattern_type,
            lucky: pattern_type == PatternType::Standard && written_type(written) == Some("lucky"),
            cap: cap(written),
            selects_files: chosen(written, "select").any(|v| unquoted(v).starts_with("file")),
            unit: unit(written),
            by_results: stated_unit(written).is_none(),
            repo_scoped: repo_scoped(written),
            lists_placeholders: written.iter().any(|item| {
                matches!(item, Written::Pattern { text, negated: false, .. }
                    if text.to_ascii_uppercase().contains(NOT_INDEXED))
            }),
            deadline_ms,
        }
    }

    /// The pattern type's name as the answer gives it, `lucky` where the
    /// query names it.
    pub fn pattern_type_name(&self) -> &'static str {
        if self.lucky { "lucky" } else { self.pattern_type.name() }
    }
}

/// `text` with `timeout:60s` in front when it sets no `timeout:`, where the
/// filter binds every branch of an `or`. A query the server rejects while
/// reading it, one that writes nothing, or one the filter would read
/// differently goes as written.
fn sent(text: &str) -> String {
    let written = read(text).written;
    if written.is_empty() || written.iter().any(|item| field(item, "timeout")) {
        return text.to_owned();
    }
    let timeout = format!("timeout:{}s", MAX_DEADLINE_MS / 1000);
    let with = format!("{timeout} {text}");
    let filter = Written::Filter(Filter {
        negated: false,
        field: "timeout".to_owned(),
        value: timeout["timeout:".len()..].to_owned(),
    });
    let read_with = read(&with).written;
    if read_with.split_first() == Some((&filter, written.as_slice())) {
        with
    } else {
        text.to_owned()
    }
}

/// How many results the query asks for: the last `count:` value, 30 when
/// there is none. The server reads a number with an optional `+`; `all`, or
/// any other value, is no cap. A query with `or` can run as several searches,
/// each with its own cap, so it has none to compare a total with.
fn cap(written: &[Written]) -> Option<u64> {
    if written.contains(&Written::Or) {
        return None;
    }
    let last = filters(written).filter(|f| f.field.eq_ignore_ascii_case("count")).last();
    let Some(filter) = last else { return Some(DEFAULT_CAP) };
    let value = unquoted(&filter.value);
    let digits = value.strip_prefix('+').unwrap_or(value);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

fn field(item: &Written, name: &str) -> bool {
    matches!(item, Written::Filter(f) if f.field.eq_ignore_ascii_case(name))
}

/// What the results count, from the query's `select:` and `type:` values.
/// A query with none of those and no pattern searches for files by name or
/// for repositories; the results tell which (`Query::by_results`).
fn unit(written: &[Written]) -> Unit {
    stated_unit(written).unwrap_or(Unit::Files)
}

fn stated_unit(written: &[Written]) -> Option<Unit> {
    let selects = || chosen(written, "select");
    let types = || chosen(written, "type");
    let has_or = written.contains(&Written::Or);
    if selects().any(repo_select) || types().any(|v| v == "repo") {
        Some(Unit::Repos)
    } else if selects().any(|v| v == "file.directory") {
        Some(Unit::Directories)
    } else if selects().any(|v| v.starts_with("file")) || types().any(|v| v == "path") {
        Some(Unit::Files)
    } else if types().any(|v| v == "symbol") && types().all(|v| v == "symbol") && !has_or {
        Some(Unit::Symbols)
    } else if types().any(|v| v == "commit" || v == "diff")
        && types().all(|v| v == "commit" || v == "diff")
        && !has_or
    {
        Some(Unit::Commits)
    } else if types().any(|v| v == "commit" || v == "diff") || has_pattern(written) {
        Some(Unit::Matches)
    } else {
        None
    }
}

/// A `select:` value whose results are repositories: `repo`, or a path
/// under it such as `repo.topics`.
fn repo_select(value: &str) -> bool {
    value.split('.').next() == Some("repo")
}

/// Whether the query has a non-negated `repo:` filter and no `or`. A query
/// narrowed only by other filters covers the whole corpus, and an operand of
/// `or` can sit beside no `repo:` filter at all.
fn repo_scoped(written: &[Written]) -> bool {
    !written.contains(&Written::Or) && filters(written).any(|f| is_repo(f) && !f.negated)
}

fn filters(written: &[Written]) -> impl Iterator<Item = &Filter> {
    written.iter().filter_map(|item| match item {
        Written::Filter(f) => Some(f),
        _ => None,
    })
}

fn is_repo(f: &Filter) -> bool {
    f.field.eq_ignore_ascii_case("repo") || f.field.eq_ignore_ascii_case("r")
}

/// The last `patterntype:` value that names a type, unquoted.
fn written_type(written: &[Written]) -> Option<&str> {
    written.iter().rev().find_map(|item| match item {
        Written::Filter(f) if !f.negated && f.field.eq_ignore_ascii_case("patterntype") => {
            let value = unquoted(&f.value);
            PatternType::named(value).map(|_| value)
        }
        _ => None,
    })
}

/// How long the search may run: the first `timeout:`, at most a minute;
/// without one, a minute when the query has a `count:`, else 20 seconds.
fn deadline_ms(written: &[Written]) -> u64 {
    let Some(Written::Filter(timeout)) = written.iter().find(|item| field(item, "timeout")) else {
        return if written.iter().any(|item| field(item, "count")) {
            MAX_DEADLINE_MS
        } else {
            DEFAULT_DEADLINE_MS
        };
    };
    match parse(unquoted(&timeout.value)) {
        Some((true, _)) => 0,
        Some((false, d)) => u64::try_from(d.as_nanos().div_ceil(1_000_000))
            .map_or(MAX_DEADLINE_MS, |ms| ms.min(MAX_DEADLINE_MS)),
        None => MAX_DEADLINE_MS,
    }
}

/// The sign (true for negative) and magnitude. Go keeps durations in an
/// `i64` of nanoseconds, so a magnitude past that range is rejected.
fn parse(text: &str) -> Option<(bool, Duration)> {
    let (negative, mut rest) = match text.as_bytes().first()? {
        b'-' => (true, &text[1..]),
        b'+' => (false, &text[1..]),
        _ => (false, text),
    };
    if rest == "0" {
        return Some((negative, Duration::ZERO));
    }
    if rest.is_empty() {
        return None;
    }
    let mut total: u128 = 0;
    while !rest.is_empty() {
        let (whole, after) = digits(rest);
        let (fraction, after) = match after.strip_prefix('.') {
            Some(tail) => digits(tail),
            None => ("", after),
        };
        if whole.is_empty() && fraction.is_empty() {
            return None;
        }
        let unit_len = after.find(|c: char| c == '.' || c.is_ascii_digit()).unwrap_or(after.len());
        let unit: u128 = match &after[..unit_len] {
            "ns" => 1,
            "us" | "\u{b5}s" | "\u{3bc}s" => 1_000,
            "ms" => 1_000_000,
            "s" => 1_000_000_000,
            "m" => 60_000_000_000,
            "h" => 3_600_000_000_000,
            _ => return None,
        };
        rest = &after[unit_len..];
        let mut value: u128 = 0;
        for digit in whole.bytes() {
            value = value.checked_mul(10)?.checked_add(u128::from(digit - b'0'))?;
            if value > 1 << 63 {
                return None;
            }
        }
        total = total.checked_add(value.checked_mul(unit)?)?;
        let mut scaled: u128 = 0;
        let mut scale: u128 = 1;
        for digit in fraction.bytes().take(18) {
            scaled = scaled * 10 + u128::from(digit - b'0');
            scale *= 10;
        }
        total = total.checked_add(scaled * unit / scale)?;
        if total > 1 << 63 {
            return None;
        }
    }
    if !negative && total == 1 << 63 {
        return None;
    }
    let nanos = u64::try_from(total).ok()?;
    Some((negative, Duration::from_nanos(nanos)))
}

fn digits(text: &str) -> (&str, &str) {
    let end = text.find(|c: char| !c.is_ascii_digit()).unwrap_or(text.len());
    text.split_at(end)
}

#[cfg(test)]
mod tests {
    use super::*;

    const OVERFLOW: &str = "340282366920938463463374607431.9s";

    #[test]
    fn a_query_with_no_timeout_is_sent_with_the_longest_deadline() {
        let rows = [
            ("NewRouter", "timeout:60s NewRouter"),
            ("foo count:5", "timeout:60s foo count:5"),
            ("(repo:a x) or (repo:b y)", "timeout:60s (repo:a x) or (repo:b y)"),
            ("\"timeout:5s\" foo", "timeout:60s \"timeout:5s\" foo"),
            ("foo timeout:5s", "foo timeout:5s"),
            ("(a timeout:5s) or b", "(a timeout:5s) or b"),
            ("foo TIMEOUT:5s", "foo TIMEOUT:5s"),
            ("or foo", "or foo"),
            ("not (foo)", "not (foo)"),
            ("", ""),
        ];
        for (written, sent) in rows {
            assert_eq!(Query::new(written).text, sent, "{written}");
        }
    }

    #[test]
    fn the_pattern_type_is_the_last_valid_patterntype_filter_else_keyword() {
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
            ("patterntype:lucky foo repo:regexp", "lucky"),
            ("patterntype:literal foo (bar or baz)", "keyword"),
        ];
        for (query, expected) in rows {
            assert_eq!(Query::new(query).pattern_type_name(), expected, "{query}");
        }
    }

    #[test]
    fn the_deadline_is_the_timeout_filter_else_60_s() {
        let rows = [
            ("foo", 60_000),
            ("foo count:5", 60_000),
            ("or foo", 20_000),
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
            ("foo timeout:-9223372036854775808ns", 0),
        ];
        for (query, ms) in rows {
            assert_eq!(Query::new(query).deadline_ms, ms, "{query}");
        }
        assert_eq!(Query::new(&format!("foo timeout:{OVERFLOW}")).deadline_ms, 60_000);
    }

    #[test]
    fn go_durations_parse_as_the_server_reads_them() {
        let go_duration = |text: &str| match parse(text)? {
            (true, d) if !d.is_zero() => None,
            (_, d) => Some(d),
        };
        let rows = [
            ("1h2m3.5s", Some(Duration::from_millis(3_723_500))),
            ("300ms", Some(Duration::from_millis(300))),
            ("2µs", Some(Duration::from_micros(2))),
            ("2μs", Some(Duration::from_micros(2))),
            ("0", Some(Duration::ZERO)),
            ("+1s", Some(Duration::from_secs(1))),
            (".5s", Some(Duration::from_millis(500))),
            ("1.25s", Some(Duration::from_millis(1_250))),
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
            ("10000000000s", None),
            (OVERFLOW, None),
            ("999999999999999999999999999999999999999999h", None),
        ];
        for (text, expected) in rows {
            assert_eq!(go_duration(text), expected, "{text:?}");
        }
    }

    #[test]
    fn the_deadline_is_60_s_unless_a_timeout_changes_it() {
        assert_eq!(Query::new("foo").deadline_ms, 60_000);
        assert_eq!(Query::new("foo count:5").deadline_ms, 60_000);
        assert_eq!(Query::new("foo timeout:90s").deadline_ms, 60_000);
        assert_eq!(Query::new("foo count:all timeout:5s").deadline_ms, 5_000);
        assert_eq!(Query::new("foo timeout:-3s").deadline_ms, 0);
    }

    #[test]
    fn the_unit_follows_select_and_type() {
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
            ("type:commit (repo:x or repo:y)", Unit::Matches),
            ("type:diff (repo:x or repo:y)", Unit::Matches),
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
            assert_eq!(Query::new(query).unit, expected, "{query}");
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
    fn a_query_carries_every_fact() {
        let query = Query::new("repo:x type:diff foo");
        assert_eq!(query.text, "timeout:60s repo:x type:diff foo");
        assert_eq!(query.unit, Unit::Commits);
        assert_eq!(query.deadline_ms, 60_000);

        let query = Query::new("repo:x foo select:file count:all");
        assert_eq!(query.unit, Unit::Files);
    }

    #[test]
    fn a_positive_repo_filter_without_or_scopes_a_query() {
        for q in ["repo:x foo", "r:x foo", r"repo:^github\.com/org/ file:a foo"] {
            assert!(Query::new(q).repo_scoped, "{q}");
        }
        for q in ["foo", "-repo:x foo", "file:x foo", "lang:go foo", "repo:x a or b"] {
            assert!(!Query::new(q).repo_scoped, "{q}");
        }
    }
}
