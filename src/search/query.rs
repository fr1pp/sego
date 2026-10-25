use std::time::Duration;

use super::lint::{
    Filter, Op, Token, check, flag_note, one_repository, tokenize, unquoted,
};
use super::plan::{PatternType, Plan, plan};

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
const SETTING_FIELDS: [&str; 8] = [
    "count",
    "timeout",
    "type",
    "select",
    "case",
    "patterntype",
    "index",
    "content",
];
/// The spellings the server reads as No for `index:`: `n` and `no` in three
/// cases, and the false spellings it accepts for any boolean; case matters,
/// so `nO` and `fALSE` are rejected.
const INDEX_NO: [&str; 11] =
    ["n", "N", "no", "NO", "No", "0", "f", "F", "false", "FALSE", "False"];
pub const UNINDEXED_PATTERN: &str = r"^NOT-INDEXED:\ (exceeds\ the\ maximum\ size\ limit|contains\ too\ many\ trigrams)$";
const PATTERN_TYPES: [&str; 8] = [
    "standard",
    "regex",
    "regexp",
    "literal",
    "structural",
    "codycontext",
    "keyword",
    "lucky",
];

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
pub struct Prepared {
    pub query: String,
    pub pattern_type: String,
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

pub fn prepare(query: &str) -> Prepared {
    let tokens = tokenize(query);
    let plan = plan(query);
    let deadline_ms = deadline_ms(&tokens);
    Prepared {
        query: query.to_owned(),
        pattern_type: pattern_type(&tokens, &plan),
        notes: check(&tokens, &plan),
        cap: cap(&tokens),
        unit: unit(&tokens),
        by_results: stated_unit(&tokens).is_none(),
        lower_bound: lower_bound(&tokens, &plan),
        diff: chosen(&tokens, "type").any(|value| value == "diff"),
        unindexed: unindexed_query(&tokens, deadline_ms),
        pinned: pinned(&tokens),
        select_cut: select_cut(&tokens),
        select_drops: select_drops(&tokens),
        content_search: content_search(&tokens),
        index_no: index_no(&tokens, &plan),
        repo_scoped: repo_scoped(&tokens),
        repo_names: repo_names(&tokens),
        flag_note: flag_note(&tokens, search_type(&tokens, &plan)),
        deadline_ms,
    }
}

fn field(token: &Token, name: &str) -> bool {
    matches!(token, Token::Filter(f) if f.field.eq_ignore_ascii_case(name))
}

fn values<'a>(
    tokens: &'a [Token],
    name: &'a str,
) -> impl Iterator<Item = &'a str> {
    tokens.iter().filter_map(move |token| match token {
        Token::Filter(f) if f.field.eq_ignore_ascii_case(name) => {
            Some(f.value.as_str())
        }
        _ => None,
    })
}

/// The values of the non-negated `name:` filters; a negated `type:` or
/// `select:` doesn't choose what the results are.
pub fn chosen<'a>(
    tokens: &'a [Token],
    name: &'a str,
) -> impl Iterator<Item = &'a str> {
    tokens.iter().filter_map(move |token| match token {
        Token::Filter(f)
            if !f.negated && f.field.eq_ignore_ascii_case(name) =>
        {
            Some(f.value.as_str())
        }
        _ => None,
    })
}

/// How many results the query asks for: the last `count:` value, 30 when
/// there is none. `all`, or a value that isn't a number, is no cap.
pub fn cap(tokens: &[Token]) -> Option<u64> {
    let Some(value) = values(tokens, "count").last() else {
        return Some(DEFAULT_CAP);
    };
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    value.parse().ok()
}

/// What the results count, from the query's `select:` and `type:` values.
/// A query with none of those and no pattern searches for files by name or
/// for repositories; the results tell which (`Prepared::by_results`).
pub fn unit(tokens: &[Token]) -> Unit {
    stated_unit(tokens).unwrap_or(Unit::Files)
}

fn stated_unit(tokens: &[Token]) -> Option<Unit> {
    let selects = || chosen(tokens, "select");
    let types = || chosen(tokens, "type");
    let has_or = tokens.iter().any(|t| matches!(t, Token::Op(Op::Or)));
    if selects().any(repo_select) || types().any(|v| v == "repo") {
        Some(Unit::Repos)
    } else if selects().any(|v| v == "file.directory") {
        Some(Unit::Directories)
    } else if selects().any(|v| v.starts_with("file"))
        || types().any(|v| v == "path")
    {
        Some(Unit::Files)
    } else if types().any(|v| v == "symbol")
        && types().all(|v| v == "symbol")
        && !has_or
    {
        Some(Unit::Symbols)
    } else if types().any(|v| v == "commit" || v == "diff")
        && types().all(|v| v == "commit" || v == "diff")
        && !has_or
    {
        Some(Unit::Commits)
    } else if types().any(|v| v == "commit" || v == "diff")
        || has_pattern(tokens)
    {
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
/// - the server clears zoekt's skipped-shard flag for a `repo` select, so
///   only a scope of one repository, which can't be cut short, or a search
///   of commits, which zoekt doesn't run, is exact.
pub fn lower_bound(tokens: &[Token], plan: &Plan) -> bool {
    let directories = chosen(tokens, "select")
        .any(|value| value == "file.directory")
        && cap(tokens).is_some();
    let repos = chosen(tokens, "select").any(repo_select)
        && !history(tokens)
        && !one_repository(plan);
    directories || repos
}

/// Whether the query searches only commits: it chooses a `type:`, every one
/// is `commit` or `diff`, and it has no `or`. The commit search reports its
/// own cut in `limitHit`, unlike the indexed search behind a `repo` select.
fn history(tokens: &[Token]) -> bool {
    let mut types = chosen(tokens, "type").peekable();
    types.peek().is_some()
        && types.all(|v| v == "commit" || v == "diff")
        && !tokens.iter().any(|t| matches!(t, Token::Op(Op::Or)))
}

pub fn duration_text(ms: u64) -> String {
    if ms.is_multiple_of(1000) {
        format!("{}s", ms / 1000)
    } else {
        format!("{ms}ms")
    }
}

/// `ms` in seconds, rounded to one decimal place.
pub fn tenths(ms: u64) -> String {
    let tenths = (ms + 50) / 100;
    format!("{}.{}", tenths / 10, tenths % 10)
}

/// A second search for the files the index skipped, within the query's
/// scope. `None` unless the query is a plain text search (`plain_scope`)
/// whose revisions are all `HEAD` or empty, and none negated: the side
/// query is exact only on the default branch's index.
pub fn unindexed_query(tokens: &[Token], deadline_ms: u64) -> Option<String> {
    let scope = plain_scope(tokens)?;
    if filters(tokens).any(|f| {
        names_unindexed_revision(f) || (f.negated && names_revision(f))
    }) {
        return None;
    }
    Some(format!(
        "{} type:file select:file index:only patterntype:regexp {UNINDEXED_PATTERN} count:all timeout:{}",
        scope.join(" "),
        duration_text(deadline_ms),
    ))
}

/// Whether the query is a content search pinned, through a non-negated
/// revision filter, to a revision other than `HEAD` or empty, which the
/// side query can't list the skipped files of.
pub fn pinned(tokens: &[Token]) -> bool {
    filters(tokens).any(|f| !f.negated && names_unindexed_revision(f))
        && content_search(tokens)
}

/// The cap that can cut a pinned query's `select:` answer: the searcher
/// caps each repository and revision before `select:`, and the server
/// doesn't always report the cut. `HEAD` is read from the index instead;
/// `file.owners` runs through its own select job with no file cap, and a
/// `repo` select keeps every repository with a match.
pub fn select_cut(tokens: &[Token]) -> Option<u64> {
    let bounded = chosen(tokens, "select")
        .any(|v| v != "file.owners" && !repo_select(v));
    let searcher =
        filters(tokens).any(|f| !f.negated && names_unindexed_revision(f));
    if !bounded || !searcher || !content_search(tokens) {
        return None;
    }
    cap(tokens)
}

/// Whether the `select:` that `select_cut` bounds can drop every result
/// the cap kept, so that a zero is a lower bound too: `symbol…` and
/// `content` drop the results they can't convert.
pub fn select_drops(tokens: &[Token]) -> bool {
    select_cut(tokens).is_some()
        && chosen(tokens, "select")
            .any(|v| v.starts_with("symbol") || v == "content")
}

/// Whether the query has a pattern searched for in file contents or the
/// symbols read from them: every `type:` it chooses is `file` or `symbol`.
pub fn content_search(tokens: &[Token]) -> bool {
    has_pattern(tokens)
        && chosen(tokens, "type").all(|v| v == "file" || v == "symbol")
}

/// Whether every search of the query's plan has `index:no`, which searches
/// the files the index skipped for too many trigrams but still not those
/// over 1 MiB. The server rejects a second `index:` in a search. A query
/// whose searches aren't read (`Plan::each`) has it when any non-negated
/// `index:` is No.
pub fn index_no(tokens: &[Token], plan: &Plan) -> bool {
    let no = |negated: bool, field: &str, value: &str| {
        !negated
            && field.eq_ignore_ascii_case("index")
            && INDEX_NO.contains(&value)
    };
    match plan.each() {
        Some(searches) if !searches.is_empty() => {
            searches.iter().all(|search| {
                search.iter().any(|f| no(f.negated, &f.field, &f.value))
            })
        }
        _ => filters(tokens).any(|f| no(f.negated, &f.field, &f.value)),
    }
}

pub fn has_pattern(tokens: &[Token]) -> bool {
    tokens.iter().any(|token| {
        matches!(token, Token::Term(_) | Token::Quoted(_) | Token::Pattern(_))
    })
}

/// Whether the query has a non-negated `repo:` or `r:` filter. A search
/// narrowed only by other filters covers the whole corpus.
pub fn repo_scoped(tokens: &[Token]) -> bool {
    filters(tokens).any(|f| is_repo(f) && !f.negated)
}

/// A `type:repo` search for the name in the query's one positive `repo:`
/// value: the segment after the owner (`pgx` in `^github\.com/jackc/pgx/v5$`),
/// or the owner when there is none. A first segment that holds a `.` is the
/// host. `None` when there is no such value, the name isn't literal, or the
/// name is the value as written.
pub fn repo_names(tokens: &[Token]) -> Option<String> {
    let mut repos = filters(tokens).filter(|f| is_repo(f) && !f.negated);
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
    let name = segments
        .get(1)
        .filter(|segment| !segment.is_empty())
        .or(segments.first())?;
    (literal_name(name) && *name != value)
        .then(|| format!("repo:{name} type:repo"))
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

fn filters(tokens: &[Token]) -> impl Iterator<Item = &Filter> {
    tokens.iter().filter_map(|token| match token {
        Token::Filter(f) => Some(f),
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
    } else if let Some((_, revisions)) =
        f.value.split_once('@').filter(|_| is_repo(f))
    {
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
fn plain_scope(tokens: &[Token]) -> Option<Vec<String>> {
    let path_function = |f: &Filter| {
        is(f, &["file", "f", "path"])
            && (f.value.starts_with("has.")
                || f.value.starts_with("contains."))
    };
    let plain = repo_scoped(tokens)
        && has_pattern(tokens)
        && filters(tokens).all(|f| {
            is(f, &SCOPE_FIELDS) || is(f, &SETTING_FIELDS) || is_revision(f)
        })
        && !filters(tokens).any(path_function)
        && !tokens.iter().any(|t| matches!(t, Token::Op(Op::Or | Op::Not)))
        && !chosen(tokens, "type")
            .any(|v| matches!(v, "commit" | "diff" | "repo" | "path"));
    let scope: Vec<String> = filters(tokens)
        .filter(|f| is(f, &SCOPE_FIELDS))
        .map(Filter::raw)
        .collect();
    if !plain || scope.iter().any(|raw| raw.contains(char::is_whitespace)) {
        return None;
    }
    Some(scope)
}

/// The pattern type the server searches in: the plan's, or for a query it
/// rejects while reading it, the last `patterntype:` value as written.
pub fn search_type(tokens: &[Token], plan: &Plan) -> PatternType {
    match plan {
        Plan::Read(searches) => searches.pattern_type,
        _ => written_type(tokens)
            .and_then(PatternType::named)
            .unwrap_or(PatternType::Keyword),
    }
}

/// The search type's name, or `lucky` where the query names it.
pub fn pattern_type(tokens: &[Token], plan: &Plan) -> String {
    let ty = search_type(tokens, plan);
    let lucky =
        ty == PatternType::Standard && written_type(tokens) == Some("lucky");
    if lucky { "lucky" } else { ty.name() }.to_owned()
}

/// The last `patterntype:` value that names a type, unquoted.
fn written_type(tokens: &[Token]) -> Option<&str> {
    tokens.iter().rev().find_map(|token| match token {
        Token::Filter(f)
            if !f.negated && f.field.eq_ignore_ascii_case("patterntype") =>
        {
            let value = unquoted(&f.value);
            PATTERN_TYPES.contains(&value).then_some(value)
        }
        _ => None,
    })
}

/// How long the search may run: the first `timeout:`, at most a minute;
/// without one, a minute when the query has a `count:`, else 20 seconds.
pub fn deadline_ms(tokens: &[Token]) -> u64 {
    let Some(Token::Filter(timeout)) =
        tokens.iter().find(|token| field(token, "timeout"))
    else {
        return if tokens.iter().any(|token| field(token, "count")) {
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
        let unit_len = after
            .find(|c: char| c == '.' || c.is_ascii_digit())
            .unwrap_or(after.len());
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
            value = value
                .checked_mul(10)?
                .checked_add(u128::from(digit - b'0'))?;
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
mod tests;
