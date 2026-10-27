use std::time::Duration;

use super::checks::{check, flag_note};
use super::plan::{
    PatternType, Plan, Written, WrittenFilter as Filter, chosen, has_pattern, plan, unquoted,
};

const MAX_DEADLINE_MS: u64 = 60_000;
const DEFAULT_DEADLINE_MS: u64 = 20_000;
const DEFAULT_CAP: u64 = 30;
const SCOPE_FIELDS: [&str; 13] = [
    "repo",
    "r",
    "file",
    "f",
    "path",
    "lang",
    "l",
    "language",
    "fork",
    "archived",
    "visibility",
    "context",
    "g",
];
/// Filters that don't change which files the query searches.
const SETTING_FIELDS: [&str; 8] =
    ["count", "timeout", "type", "select", "case", "patterntype", "index", "content"];
/// The spellings the server reads as No for `index:`: `n` and `no` in three
/// cases, and the false spellings it accepts for any boolean; case matters,
/// so `nO` and `fALSE` are rejected.
const INDEX_NO: [&str; 11] = ["n", "N", "no", "NO", "No", "0", "f", "F", "false", "FALSE", "False"];
/// What the index holds in place of a file it skipped, before a space and
/// the reason.
pub const NOT_INDEXED: &str = "NOT-INDEXED:";

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
    /// The query as written, sent to the server as it is.
    pub text: String,
    pub pattern_type: PatternType,
    /// The query names `lucky`, which the server reads as `standard`.
    lucky: bool,
    pub cap: Option<u64>,
    pub unit: Unit,
    /// The query states no unit, so the results decide it: `Repos` when
    /// every result is a repository, `Commits` when every result is a
    /// commit, and `unit` otherwise.
    pub by_results: bool,
    pub lower_bound: bool,
    /// The query has a non-negated `type:diff`.
    pub diff: bool,
    pub deadline_ms: u64,
    pub unindexed: Option<String>,
    pub pinned: bool,
    pub select_cut: Option<u64>,
    pub select_drops: bool,
    pub content_search: bool,
    pub index_no: bool,
    pub repo_scoped: bool,
    pub repo_names: Option<String>,
    pub flag_note: Option<String>,
    pub notes: Vec<String>,
}

impl Query {
    /// The facts about `text` that its plan gives, and the notes of the
    /// local checks. A plan sego rejects gives none.
    pub fn new(text: &str) -> Query {
        let plan = plan(text);
        let written = plan.written();
        let deadline_ms = deadline_ms(written);
        let pattern_type = search_type(written, &plan);
        Query {
            text: text.to_owned(),
            pattern_type,
            lucky: pattern_type == PatternType::Standard && written_type(written) == Some("lucky"),
            notes: check(written, &plan, pattern_type),
            cap: cap(written),
            unit: unit(written),
            by_results: stated_unit(written).is_none(),
            lower_bound: lower_bound(written, &plan),
            diff: chosen(written, "type").any(|value| value == "diff"),
            unindexed: unindexed_query(written, deadline_ms),
            pinned: pinned(written),
            select_cut: select_cut(written),
            select_drops: select_drops(written),
            content_search: content_search(written),
            index_no: index_no(written, &plan),
            repo_scoped: repo_scoped(&plan),
            repo_names: repo_names(written),
            flag_note: flag_note(written, search_type(written, &plan)),
            deadline_ms,
        }
    }

    /// The pattern type's name as the answer gives it, `lucky` where the
    /// query names it.
    pub fn pattern_type_name(&self) -> &'static str {
        if self.lucky { "lucky" } else { self.pattern_type.name() }
    }
}

fn field(item: &Written, name: &str) -> bool {
    matches!(item, Written::Filter(f) if f.field.eq_ignore_ascii_case(name))
}

fn values<'a>(written: &'a [Written], name: &'a str) -> impl Iterator<Item = &'a str> {
    written.iter().filter_map(move |item| match item {
        Written::Filter(f) if f.field.eq_ignore_ascii_case(name) => Some(f.value.as_str()),
        _ => None,
    })
}

/// How many results the query asks for: the last `count:` value, 30 when
/// there is none. The server reads a number with an optional `+`; `all`, or
/// any other value, is no cap.
fn cap(written: &[Written]) -> Option<u64> {
    let Some(value) = values(written, "count").last() else {
        return Some(DEFAULT_CAP);
    };
    let digits = value.strip_prefix('+').unwrap_or(value);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
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

/// Whether the count can fall short of the answer without `limitHit`:
/// - each index replica returns at most the cap in files, and the server
///   merges them into directories without saying it cut them, at any cap
///   but `count:all`;
/// - for a `repo` select the server drops the index's skipped-file and
///   skipped-shard flags, and reports a cut only once the repositories reach
///   the cap. Each replica still stops at the cap counted in files, and a
///   large repository's shards can fill several of those, so fewer
///   repositories than the cap come back unflagged (#32). Even `count:all`
///   can meet the index's match limits unflagged. Only a scope of one
///   repository, which can't be cut short, or a search of commits, which the
///   index doesn't run, is exact.
fn lower_bound(written: &[Written], plan: &Plan) -> bool {
    let directories =
        chosen(written, "select").any(|value| value == "file.directory") && cap(written).is_some();
    let repos =
        chosen(written, "select").any(repo_select) && !history(written) && !one_repository(plan);
    directories || repos
}

/// Whether the query searches only commits: it chooses a `type:`, every one
/// is `commit` or `diff`, and it has no `or`. The commit search reports its
/// own cut in `limitHit`, unlike the indexed search behind a `repo` select.
fn history(written: &[Written]) -> bool {
    let mut types = chosen(written, "type").peekable();
    types.peek().is_some()
        && types.all(|v| v == "commit" || v == "diff")
        && !written.contains(&Written::Or)
}

fn duration_text(ms: u64) -> String {
    if ms.is_multiple_of(1000) { format!("{}s", ms / 1000) } else { format!("{ms}ms") }
}

/// A second search for the files the index skipped, within the query's
/// scope. `None` unless the query is a plain text search (`plain_scope`)
/// whose revisions are all `HEAD` or empty, and none negated: the side
/// query is exact only on the default branch's index.
fn unindexed_query(written: &[Written], deadline_ms: u64) -> Option<String> {
    let scope = plain_scope(written)?;
    if filters(written).any(|f| names_unindexed_revision(f) || (f.negated && names_revision(f))) {
        return None;
    }
    Some(format!(
        "{} type:file select:file index:only patterntype:regexp \
         ^{NOT_INDEXED}\\ (exceeds\\ the\\ maximum\\ size\\ limit|contains\\ too\\ many\\ trigrams)$ \
         count:all timeout:{}",
        scope.join(" "),
        duration_text(deadline_ms),
    ))
}

/// Whether the query is a content search pinned, through a non-negated
/// revision filter, to a revision other than `HEAD` or empty, which the
/// side query can't list the skipped files of.
fn pinned(written: &[Written]) -> bool {
    filters(written).any(|f| !f.negated && names_unindexed_revision(f)) && content_search(written)
}

/// The cap that can cut a pinned search's `select:` answer: the searcher
/// caps each repository and revision before `select:`, and the server
/// doesn't always report the cut. `HEAD` is read from the index instead;
/// `file.owners` runs through its own select job with no file cap, and a
/// `repo` select keeps every repository with a match.
fn select_cut(written: &[Written]) -> Option<u64> {
    let bounded = chosen(written, "select").any(|v| v != "file.owners" && !repo_select(v));
    let searcher = filters(written).any(|f| !f.negated && names_unindexed_revision(f));
    if !bounded || !searcher || !content_search(written) {
        return None;
    }
    cap(written)
}

/// Whether the `select:` that `select_cut` bounds can drop every result
/// the cap kept, so that a zero is a lower bound too: `content` drops the
/// results it can't convert, and a symbol search's `select:symbol.KIND` the
/// symbols of other kinds. A content search's `select:symbol` returns
/// nothing at all, which a local check notes.
fn select_drops(written: &[Written]) -> bool {
    let symbols = chosen(written, "type").any(|v| v == "symbol");
    select_cut(written).is_some()
        && chosen(written, "select")
            .any(|v| v == "content" || (symbols && v.starts_with("symbol.")))
}

/// Whether the query has a pattern searched for in file contents or the
/// symbols read from them: every `type:` it chooses is `file` or `symbol`.
fn content_search(written: &[Written]) -> bool {
    has_pattern(written) && chosen(written, "type").all(|v| v == "file" || v == "symbol")
}

/// Whether every basic query of the query's plan has `index:no`, which searches
/// the files the index skipped for too many trigrams but still not those
/// over 1 MiB. The server rejects a second `index:` in a basic query. A query
/// whose basic queries aren't read (`Plan::each`) has it when any non-negated
/// `index:` is No.
fn index_no(written: &[Written], plan: &Plan) -> bool {
    let no = |negated: bool, field: &str, value: &str| {
        !negated && field.eq_ignore_ascii_case("index") && INDEX_NO.contains(&value)
    };
    match plan.each() {
        Some(basic_queries) if !basic_queries.is_empty() => basic_queries
            .iter()
            .all(|basic| basic.iter().any(|f| no(f.negated, &f.field, &f.value))),
        _ => filters(written).any(|f| no(f.negated, &f.field, &f.value)),
    }
}

/// Whether every basic query of the query's plan has a non-negated `repo:`
/// filter. A basic query narrowed only by other filters covers the whole corpus.
fn repo_scoped(plan: &Plan) -> bool {
    let Plan::Read(basic_queries) = plan else { return false };
    let ends = basic_queries.reach(false, |scoped, f| scoped || (f.field == "repo" && !f.negated));
    !ends.is_empty() && ends.iter().all(|&scoped| scoped)
}

/// A `type:repo` search for the name in the query's one positive `repo:`
/// value: the segment after the owner (`pgx` in `^github\.com/jackc/pgx/v5$`),
/// or the owner when there is none. A first segment that holds a `.` is the
/// host. `None` when there is no such value, the name isn't literal, or the
/// name is the value as written.
fn repo_names(written: &[Written]) -> Option<String> {
    let mut repos = filters(written).filter(|f| is_repo(f) && !f.negated);
    let (Some(repo), None) = (repos.next(), repos.next()) else {
        return None;
    };
    let value = unquoted(&repo.value);
    let value = value.split_once('@').map_or(value, |(name, _)| name);
    if value.starts_with("has.") || value.starts_with("contains.") {
        return None;
    }
    let path = value.strip_prefix('^').unwrap_or(value);
    let path = path.strip_suffix('$').unwrap_or(path);
    let mut segments: Vec<&str> = path.split('/').collect();
    if segments.len() > 1 && segments[0].contains('.') {
        segments.remove(0);
    }
    let name = segments.get(1).filter(|segment| !segment.is_empty()).or(segments.first())?;
    (literal_name(name) && *name != value).then(|| format!("repo:{name} type:repo"))
}

/// Letters, digits, `_`, `-` and `.`, each `.` or `-` escaped or not.
fn literal_name(name: &str) -> bool {
    let mut chars = name.chars();
    while let Some(c) = chars.next() {
        let literal = if c == '\\' {
            matches!(chars.next(), Some('.' | '-'))
        } else {
            c.is_ascii_alphanumeric() || "_-.".contains(c)
        };
        if !literal {
            return false;
        }
    }
    !name.is_empty()
}

fn filters(written: &[Written]) -> impl Iterator<Item = &Filter> {
    written.iter().filter_map(|item| match item {
        Written::Filter(f) => Some(f),
        _ => None,
    })
}

fn is(f: &Filter, names: &[&str]) -> bool {
    names.iter().any(|name| f.field.eq_ignore_ascii_case(name))
}

fn is_repo(f: &Filter) -> bool {
    is(f, &["repo", "r"])
}

fn is_revision(f: &Filter) -> bool {
    is(f, &["rev", "revision"])
}

/// A `repo:` value with `@`, or a `rev:` or `revision:` filter.
fn names_revision(f: &Filter) -> bool {
    (is_repo(f) && f.value.contains('@')) || is_revision(f)
}

/// A revision filter that names a revision the searcher reads: one other
/// than `HEAD`, which the index holds, or the empty revision, which means
/// `HEAD`.
fn names_unindexed_revision(f: &Filter) -> bool {
    let revisions = if is_revision(f) {
        f.value.as_str()
    } else if let Some((_, revisions)) = f.value.split_once('@').filter(|_| is_repo(f)) {
        revisions
    } else {
        return false;
    };
    revisions.split(':').any(|rev| !rev.is_empty() && rev != "HEAD")
}

/// The scope filters, as written, of a query that is plainly a text search
/// of named repositories at any revision: every filter is a scope field, a
/// setting or a revision, and what the scope restates is all that narrows
/// the files searched. `None` for any other query.
fn plain_scope(written: &[Written]) -> Option<Vec<String>> {
    let path_function = |f: &Filter| {
        is(f, &["file", "f", "path"])
            && (f.value.starts_with("has.") || f.value.starts_with("contains."))
    };
    let plain = filters(written).any(|f| is_repo(f) && !f.negated)
        && has_pattern(written)
        && filters(written)
            .all(|f| is(f, &SCOPE_FIELDS) || is(f, &SETTING_FIELDS) || is_revision(f))
        && !filters(written).any(path_function)
        && !written
            .iter()
            .any(|t| matches!(t, Written::Or | Written::Pattern { negated: true, .. }))
        && !chosen(written, "type").any(|v| matches!(v, "commit" | "diff" | "repo" | "path"));
    let scope: Vec<String> =
        filters(written).filter(|f| is(f, &SCOPE_FIELDS)).map(Filter::raw).collect();
    if !plain || scope.iter().any(|raw| raw.contains(char::is_whitespace)) {
        return None;
    }
    Some(scope)
}

/// The pattern type the server searches in: the plan's, or for a query it
/// rejects while reading it, the last `patterntype:` value as written.
fn search_type(written: &[Written], plan: &Plan) -> PatternType {
    match plan {
        Plan::Read(basic_queries) => basic_queries.pattern_type,
        _ => written_type(written).and_then(PatternType::named).unwrap_or(PatternType::Keyword),
    }
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

/// Whether the query can search only one repository: each basic query of its
/// plan has a non-negated `repo:` whose value is one exact name, the same in
/// every one. Two `repo:` filters in a basic query intersect, so one exact
/// name among them is enough. A plan of 64 or more basic queries isn't read.
fn one_repository(plan: &Plan) -> bool {
    let Some(basic_queries) = plan.each() else {
        return false;
    };
    let mut names = basic_queries.iter().map(|basic| {
        basic
            .iter()
            .filter(|f| f.field == "repo" && !f.negated)
            .find_map(|f| exact_repository(&f.value))
    });
    let Some(Some(first)) = names.next() else {
        return false;
    };
    names.all(|name| name == Some(first))
}

/// The name in a `repo:` value that matches one repository: `^NAME$`, with
/// an optional `@` revision, where NAME is `/`-separated segments that each
/// hold a letter or digit, made of letters, digits, `_`, `-` and `.`, each
/// punctuation mark (and `/`) escaped or not.
fn exact_repository(value: &str) -> Option<&str> {
    let name = value.split_once('@').map_or(value, |(name, _)| name);
    let body = name.strip_prefix('^')?.strip_suffix('$')?;
    let mut chars = body.chars();
    while let Some(c) = chars.next() {
        let literal = if c == '\\' {
            matches!(chars.next(), Some('.' | '-' | '_' | '/'))
        } else {
            c.is_ascii_alphanumeric() || "_-./".contains(c)
        };
        if !literal {
            return None;
        }
    }
    body.split('/')
        .all(|segment| segment.chars().any(|c| c.is_ascii_alphanumeric()))
        .then_some(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::time::{Duration, Instant};

    const OVERFLOW: &str = "340282366920938463463374607431.9s";

    #[test]
    fn the_cap_and_deadline_follow_the_count_filter() {
        let rows = [
            ("NewRouter", Some(30), 20_000),
            ("\"count:5\" foo", Some(30), 20_000),
            ("COUNT:5 foo", Some(5), 60_000),
            ("foo (bar count:all)", None, 60_000),
            ("-count:5 x", Some(5), 60_000),
        ];
        for (text, cap, deadline) in rows {
            let query = Query::new(text);
            assert_eq!(query.text, text);
            assert_eq!(query.cap, cap, "{text}");
            assert_eq!(query.deadline_ms, deadline, "{text}");
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
    fn the_deadline_is_the_timeout_filter_else_20_s_or_60_s_with_a_count() {
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
    fn the_cap_is_the_last_count_or_30_and_all_has_none() {
        assert_eq!(Query::new("foo").cap, Some(30));
        assert_eq!(Query::new("foo count:5").cap, Some(5));
        assert_eq!(Query::new("foo COUNT:all").cap, None);
        assert_eq!(Query::new("foo count:5 count:7").cap, Some(7));
        assert_eq!(Query::new("foo count:5 count:all").cap, None);
        assert_eq!(Query::new("foo count:many").cap, None);
        assert_eq!(Query::new("foo count:+5").cap, Some(5));
    }

    #[test]
    fn the_deadline_is_20_s_unless_a_count_or_timeout_changes_it() {
        assert_eq!(Query::new("foo").deadline_ms, 20_000);
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
    fn an_eligible_query_gets_an_unindexed_files_query() {
        assert_eq!(
            Query::new(r"repo:^github\.com/gorilla/mux$ file:\.go$ -lang:markdown foo count:5")
                .unindexed
                .as_deref(),
            Some(
                r"repo:^github\.com/gorilla/mux$ file:\.go$ -lang:markdown type:file select:file index:only patterntype:regexp ^NOT-INDEXED:\ (exceeds\ the\ maximum\ size\ limit|contains\ too\ many\ trigrams)$ count:all timeout:60s"
            ),
        );
        assert_eq!(
            Query::new("R:x -F:y foo timeout:1500ms").unindexed.as_deref(),
            Some(
                r"R:x -F:y type:file select:file index:only patterntype:regexp ^NOT-INDEXED:\ (exceeds\ the\ maximum\ size\ limit|contains\ too\ many\ trigrams)$ count:all timeout:1500ms"
            ),
        );
        assert_eq!(
            Query::new("repo:x g:team foo case:yes content:y").unindexed.as_deref(),
            Some(
                r"repo:x g:team type:file select:file index:only patterntype:regexp ^NOT-INDEXED:\ (exceeds\ the\ maximum\ size\ limit|contains\ too\ many\ trigrams)$ count:all timeout:20s"
            ),
        );
        assert_eq!(
            Query::new("repo:x@HEAD rev:HEAD foo").unindexed.as_deref(),
            Some(
                r"repo:x@HEAD type:file select:file index:only patterntype:regexp ^NOT-INDEXED:\ (exceeds\ the\ maximum\ size\ limit|contains\ too\ many\ trigrams)$ count:all timeout:20s"
            ),
        );
        for (query, scope) in [
            ("repo:x NOT file:test foo", "repo:x -file:test"),
            ("NOT repo:x repo:y foo", "-repo:x repo:y"),
        ] {
            assert_eq!(
                Query::new(query).unindexed,
                Some(format!(
                    r"{scope} type:file select:file index:only patterntype:regexp ^NOT-INDEXED:\ (exceeds\ the\ maximum\ size\ limit|contains\ too\ many\ trigrams)$ count:all timeout:20s"
                )),
                "{query}"
            );
        }
        assert_eq!(duration_text(500), "500ms");
        assert_eq!(duration_text(20_000), "20s");
    }

    #[test]
    fn an_ineligible_query_gets_no_unindexed_files_query() {
        for q in [
            "foo",
            "-repo:x foo",
            "repo:x a or b",
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
            assert_eq!(Query::new(q).unindexed, None, "{q}");
        }
    }

    #[test]
    fn a_directory_count_is_a_lower_bound_unless_count_all() {
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
            assert_eq!(Query::new(query).lower_bound, expected, "{query}");
        }
    }

    #[test]
    fn a_pinned_select_search_gets_its_cut_from_the_cap() {
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
            assert_eq!(Query::new(query).select_cut, expected, "{query}");
        }
    }

    #[test]
    fn a_pinned_select_of_content_or_of_a_symbol_kind_drops_results() {
        let rows = [
            ("repo:x@v1 type:symbol foo select:symbol", false),
            ("repo:x@v1 type:symbol foo select:symbol.method", true),
            ("repo:x@v1 foo select:symbol", false),
            ("repo:x@v1 foo select:symbol.method", false),
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
            assert_eq!(Query::new(query).select_drops, expected, "{query}");
        }
    }

    #[test]
    fn a_query_carries_every_fact() {
        let query = Query::new("repo:x type:diff foo");
        assert_eq!(query.text, "repo:x type:diff foo");
        assert_eq!(query.cap, Some(30));
        assert_eq!(query.unit, Unit::Commits);
        assert!(!query.lower_bound);
        assert!(query.diff);
        assert_eq!(query.deadline_ms, 20_000);
        assert_eq!(query.unindexed, None);

        let query = Query::new("repo:x foo select:file count:all");
        assert_eq!(query.cap, None);
        assert_eq!(query.unit, Unit::Files);
        assert!(!query.lower_bound);
        assert!(!query.diff);
        assert!(query.unindexed.is_some());
    }

    #[test]
    fn a_query_at_a_revision_other_than_head_is_a_pinned_search() {
        for q in ["repo:x@v1 foo", "repo:x rev:v1 foo", "repo:x revision:v1 foo"] {
            let query = Query::new(q);
            assert!(query.pinned, "{q}");
            assert_eq!(query.unindexed, None, "{q}");
        }
        for q in [
            "repo:x foo",
            "repo:x@HEAD foo",
            "repo:x@ foo",
            "repo:x rev:HEAD foo",
            "repo:x revision:HEAD foo",
        ] {
            let query = Query::new(q);
            assert!(!query.pinned, "{q}");
            assert!(query.unindexed.is_some(), "{q}");
        }
        for q in
            ["repo:x@HEAD:v1 foo", "repo:x@HEAD rev:v1 foo", "repo:x@head foo", "repo:x@main foo"]
        {
            let query = Query::new(q);
            assert!(query.pinned, "{q}");
            assert_eq!(query.unindexed, None, "{q}");
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
            let query = Query::new(q);
            assert!(query.pinned, "{q}");
            assert_eq!(query.unindexed, None, "{q}");
        }
        for q in [
            "repo:x@v1",
            "repo:x@v1 type:path foo",
            "repo:x@v1 type:bogus foo",
            "repo:x@v1 type:commit foo",
            "repo:x@v1 type:diff foo",
            "repo:x@v1 type:repo foo",
        ] {
            assert!(!Query::new(q).pinned, "{q}");
        }
        for q in ["repo:x -r:y@v1 foo", "repo:x -rev:v1 foo", "repo:x -rev:v1 a or b"] {
            let query = Query::new(q);
            assert!(!query.pinned, "{q}");
            assert_eq!(query.unindexed, None, "{q}");
        }
    }

    #[test]
    fn a_positive_repo_filter_scopes_a_query() {
        for q in ["repo:x foo", "r:x foo", r"repo:^github\.com/org/ file:a foo"] {
            assert!(Query::new(q).repo_scoped, "{q}");
        }
        for q in ["foo", "-repo:x foo", "file:x foo", "lang:go foo"] {
            assert!(!Query::new(q).repo_scoped, "{q}");
        }
    }

    #[test]
    fn a_query_naming_one_repository_gets_a_repository_lookup_by_its_last_name_segment() {
        let rows = [
            (
                r#"repo:^github\.com/jackc/pgx/v5$@v5.5.5 -file:_test\.go$ "conn busy" timeout:60s"#,
                Some("repo:pgx type:repo"),
            ),
            (r"repo:^github.com/jackc/pgconn$ foo", Some("repo:pgconn type:repo")),
            (r"repo:^github\.com/expressjs/security-wg$ foo", Some("repo:security-wg type:repo")),
            ("repo:github.com/jackc/pgx/v5 foo", Some("repo:pgx type:repo")),
            ("repo:jackc/pgx/v5 foo", Some("repo:pgx type:repo")),
            ("r:jackc/pgx foo", Some("repo:pgx type:repo")),
            ("REPO:jackc/pgx foo", Some("repo:pgx type:repo")),
            (r#"repo:"^github\.com/jackc/pgx$" foo"#, Some("repo:pgx type:repo")),
            (r"repo:^github\.com/jackc/ foo", Some("repo:jackc type:repo")),
            ("repo:^pgx$ foo", Some("repo:pgx type:repo")),
            (r"repo:^github\.com/a/b\.js$ foo", Some(r"repo:b\.js type:repo")),
            (r"repo:^b\.js$ foo", Some(r"repo:b\.js type:repo")),
            ("repo:a/b -repo:c/d foo", Some("repo:b type:repo")),
            ("repo:jackc/pgx@v5.5.5 foo", Some("repo:pgx type:repo")),
            (r"repo:^github\.com/a/b\-c$ foo", Some(r"repo:b\-c type:repo")),
            (r"repo:^github\.com/jackc/pgx$ type:repo", Some("repo:pgx type:repo")),
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
            assert_eq!(Query::new(q).repo_names.as_deref(), expected, "{q}");
        }
    }

    #[test]
    fn only_a_pattern_that_searches_contents_or_symbols_makes_a_content_search() {
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
            assert!(Query::new(q).content_search, "{q}");
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
            assert!(!Query::new(q).content_search, "{q}");
        }
    }

    #[test]
    fn index_no_is_any_value_the_server_reads_as_no() {
        for value in ["n", "N", "no", "NO", "No", "0", "f", "F", "false", "FALSE", "False"] {
            let q = format!("repo:x index:{value} foo");
            assert!(Query::new(&q).index_no, "{q}");
        }
        let deep = format!("{}repo:x foo{} index:no", "(".repeat(65), ")".repeat(65));
        let wide = format!("{} index:no", vec!["(repo:x foo)"; 64].join(" or "));
        for q in [deep.as_str(), wide.as_str()] {
            assert!(Query::new(q).index_no, "{q}");
        }
        for q in [
            "repo:x INDEX:no foo",
            "(repo:x index:no foo) or (repo:y index:no bar)",
            "repo:x index:no foo or bar",
        ] {
            assert!(Query::new(q).index_no, "{q}");
        }
        for q in [
            "",
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
            assert!(!Query::new(q).index_no, "{q}");
        }
    }

    /// The least time of three that `Query::new` takes for a query of `bytes`:
    /// `head`, then repeats of `unit`, then `tail` repeated as often.
    fn reading(head: &str, unit: &str, tail: &str, bytes: usize) -> Duration {
        let units = bytes / (unit.len() + tail.len());
        let query = format!("{head}{}{}", unit.repeat(units), tail.repeat(units));
        let time = || {
            let started = Instant::now();
            Query::new(&query);
            started.elapsed()
        };
        (0..3).map(|_| time()).min().unwrap_or_default()
    }

    /// 64 KiB of each shape takes at most a few times sixteen times what 4 KiB
    /// does: time that grows with the square of the length would take some 256
    /// times as long. The ratio, not the time, is checked, so a loaded machine
    /// doesn't fail it.
    #[test]
    fn a_query_is_read_in_linear_time() {
        let nests = ["(f:x a ", "(a f:x ", "(f:x a b ", "(-f:x a ", "(not f:x a ", "(content:x a "];
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
            let small = reading(head, unit, tail, 4 << 10);
            let large = reading(head, unit, tail, 64 << 10);
            assert!(
                large < small * 48 + Duration::from_millis(20),
                "{head}{unit:?}: {large:?}, against {small:?} for a sixteenth"
            );
        }
    }
}
