//! Searching: hits, the summary, suggested filters and alerts.

use std::ops::Range;
use std::time::Duration;

use serde::Deserialize;
use serde::de::IgnoredAny;

use crate::address::{CommitId, Pinned, RepoName};
use crate::diagnostic::{Diagnostic, SearchProblem};
use crate::domain::{
    Alert, CommitHit, FileHit, Filter, Hit, Line, RepoHit, SearchReport,
    Skipped, Span, Symbol, Unavailable,
};

use super::envelope::Envelope;
use super::vocabulary;
use super::{FetchError, Plan};

const QUERY: &str = r"query Search($query: String!) {
  search(query: $query, version: V3, patternType: keyword) {
    results {
      matchCount limitHit approximateResultCount elapsedMilliseconds
      dynamicFilters { value count limitHit kind }
      cloning { name } missing { name } timedout { name }
      alert { title description proposedQueries { description query } }
      results {
        __typename
        ... on FileMatch {
          repository { name }
          file { path commit { oid } }
          pathMatches { start { line character } end { line character } }
          symbols { name kind containerName location { range { start { line character } } } }
          chunkMatches { content contentStart { line } ranges { start { line character } end { line character } } }
        }
        ... on Repository { name description stars }
        ... on CommitSearchResult {
          commit { oid subject author { person { name } date } repository { name } }
          diffPreview { value }
        }
      }
    }
  }
}
";

pub fn plan(query: &str) -> Plan {
    Plan { query: QUERY, variables: serde_json::json!({"query": query}) }
}

/// `query`'s first `timeout:` token outside double quotes: the byte range
/// of its value and the duration it gives. `None` if there is none or its
/// value isn't a Go duration.
fn timeout_token(query: &str) -> Option<(Range<usize>, Duration)> {
    const FIELD: &str = "timeout:";
    let token = tokens(query).into_iter().find(|token| {
        query[token.clone()]
            .get(..FIELD.len())
            .is_some_and(|field| field.eq_ignore_ascii_case(FIELD))
    })?;
    let value = token.start + FIELD.len()..token.end;
    let duration = go_duration(&query[value.clone()])?;
    Some((value, duration))
}

/// The byte ranges of `query`'s tokens: runs of anything but whitespace,
/// where whitespace between double quotes, as in `"a b"` or
/// `content:"a b"`, doesn't end a token.
fn tokens(query: &str) -> Vec<Range<usize>> {
    let mut tokens = Vec::new();
    let mut start = None;
    let (mut quoted, mut escaped) = (false, false);
    for (i, c) in query.char_indices() {
        if quoted {
            match c {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                '"' => quoted = false,
                _ => {}
            }
        } else if c.is_whitespace() {
            if let Some(start) = start.take() {
                tokens.push(start..i);
            }
        } else {
            start.get_or_insert(i);
            quoted = c == '"';
        }
    }
    if let Some(start) = start {
        tokens.push(start..query.len());
    }
    tokens
}

/// Whether `token` is one of `query`'s tokens, as `tokens` splits them.
pub fn has_token(query: &str, token: &str) -> bool {
    tokens(query).into_iter().any(|range| query[range] == *token)
}

/// The nanoseconds in each of Go's duration units.
const GO_UNITS: &[(&str, u128)] = &[
    ("h", 3_600_000_000_000),
    ("m", 60_000_000_000),
    ("s", 1_000_000_000),
    ("ms", 1_000_000),
    ("us", 1_000),
    ("\u{b5}s", 1_000),
    ("\u{3bc}s", 1_000),
    ("ns", 1),
];

/// `text` as Go's `time.ParseDuration` reads it, such as `1m30s` or
/// `1.5s`, but with no sign; `None` if it isn't one.
fn go_duration(text: &str) -> Option<Duration> {
    let number = |c: char| c.is_ascii_digit() || c == '.';
    let mut rest = text;
    let mut nanos: u128 = 0;
    if rest.is_empty() {
        return None;
    }
    while !rest.is_empty() {
        let (amount, tail) =
            rest.split_at(rest.find(|c| !number(c)).unwrap_or(rest.len()));
        let (unit, tail) =
            tail.split_at(tail.find(number).unwrap_or(tail.len()));
        let &(_, scale) = GO_UNITS.iter().find(|(name, _)| *name == unit)?;
        let (whole, fraction) = amount.split_once('.').unwrap_or((amount, ""));
        if whole.is_empty() && fraction.is_empty() {
            return None;
        }
        let whole: u128 =
            if whole.is_empty() { 0 } else { whole.parse().ok()? };
        let mut part = whole.checked_mul(scale)?;
        let mut divisor = 1;
        // Digits past the 18th are below a nanosecond in every unit.
        for digit in fraction.chars() {
            let digit = digit.to_digit(10)?;
            if divisor < 10u128.pow(18) {
                divisor *= 10;
                part += u128::from(digit) * scale / divisor;
            }
        }
        nanos = nanos.checked_add(part)?;
        rest = tail;
    }
    u64::try_from(nanos).ok().map(Duration::from_nanos)
}

/// `duration` in Go's syntax, in its largest whole unit.
fn go_text(duration: Duration) -> String {
    let nanos = duration.as_nanos();
    GO_UNITS
        .iter()
        .find(|&&(_, scale)| nanos.is_multiple_of(scale))
        .map_or_else(
            || format!("{nanos}ns"),
            |&(unit, scale)| format!("{}{unit}", nanos / scale),
        )
}

/// `query` with its `timeout:` doubled; `None` if it has none.
pub fn with_doubled_timeout(query: &str) -> Option<String> {
    let (value, duration) = timeout_token(query)?;
    let mut doubled = query.to_owned();
    doubled.replace_range(value, &go_text(duration.saturating_mul(2)));
    Some(doubled)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Data {
    search: Option<Search>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Search {
    results: SearchResults,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct SearchResults {
    match_count: u64,
    limit_hit: bool,
    approximate_result_count: String,
    elapsed_milliseconds: u64,
    dynamic_filters: Vec<WireFilter>,
    cloning: Vec<Named>,
    missing: Vec<Named>,
    timedout: Vec<Named>,
    alert: Option<WireAlert>,
    results: Vec<Item>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Named {
    name: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct WireFilter {
    value: String,
    count: u64,
    limit_hit: bool,
    kind: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct WireAlert {
    title: String,
    description: Option<String>,
    proposed_queries: Option<Vec<Proposal>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Proposal {
    #[serde(rename = "description")]
    _description: IgnoredAny,
    query: String,
}

#[derive(Deserialize)]
#[serde(tag = "__typename", deny_unknown_fields)]
enum Item {
    #[serde(rename_all = "camelCase")]
    FileMatch {
        repository: Named,
        file: WireFile,
        path_matches: Vec<IgnoredAny>,
        symbols: Vec<WireSymbol>,
        chunk_matches: Vec<Chunk>,
    },
    Repository {
        name: String,
        description: String,
        stars: u64,
    },
    #[serde(rename_all = "camelCase")]
    CommitSearchResult {
        commit: WireCommit,
        diff_preview: Option<Highlighted>,
    },
    /// A kind of result the document asks nothing of.
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireFile {
    path: String,
    commit: Oid,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Oid {
    oid: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct WireSymbol {
    name: String,
    kind: String,
    container_name: Option<String>,
    location: Location,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Location {
    range: Option<SymbolRange>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SymbolRange {
    start: Position,
}

/// Lines of a file around its matches. `content` holds the lines from
/// `contentStart` on, and the `ranges` are file-absolute.
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Chunk {
    content: String,
    content_start: StartLine,
    ranges: Vec<WireRange>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StartLine {
    line: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireRange {
    start: Position,
    end: Position,
}

/// A 0-based line, and a 0-based codepoint offset within it.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Position {
    line: u32,
    character: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireCommit {
    oid: String,
    subject: String,
    author: Signature,
    repository: Named,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Signature {
    person: Named,
    date: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Highlighted {
    value: String,
}

/// The report `envelope` answers `query` with, or why there is none.
pub fn answer(
    query: &str,
    envelope: Envelope,
) -> Result<SearchReport, Box<dyn Diagnostic>> {
    let trace = envelope.trace.clone();
    let Data { search } = envelope.into_data()?;
    let Some(Search { results }) = search else {
        let cause = "the search is null".to_owned();
        return Err(FetchError::Field { cause, trace }.into());
    };
    let elapsed = Duration::from_millis(results.elapsed_milliseconds);
    let report = report(query, results)
        .map_err(|cause| FetchError::Field { cause, trace })?;
    classify(report, elapsed)
}

/// The report of `results`, or why one of its hits can't be read.
fn report(
    query: &str,
    results: SearchResults,
) -> Result<SearchReport, String> {
    let SearchResults {
        match_count,
        limit_hit,
        approximate_result_count,
        elapsed_milliseconds: _,
        dynamic_filters,
        cloning,
        missing,
        timedout,
        alert,
        results,
    } = results;
    let mut hits = Vec::new();
    let mut unsupported = 0;
    for item in results {
        match hit(item)? {
            Some(hit) => hits.push(hit),
            None => unsupported += 1,
        }
    }
    let names = |repos: Vec<Named>| -> Vec<String> {
        repos.into_iter().map(|repo| repo.name).collect()
    };
    let approximate = (approximate_result_count != match_count.to_string())
        .then_some(approximate_result_count);
    let filters = dynamic_filters
        .into_iter()
        .map(|filter| Filter {
            kind: vocabulary::filter_kind(&filter.kind),
            value: filter.value,
            count: filter.count,
            limit_hit: filter.limit_hit,
        })
        .collect();
    let alert = alert.map(|alert| Alert {
        title: alert.title,
        description: alert.description,
        proposed: alert
            .proposed_queries
            .unwrap_or_default()
            .into_iter()
            .map(|proposal| proposal.query)
            .collect(),
    });
    Ok(SearchReport {
        query: query.to_owned(),
        match_count,
        approximate,
        limit_hit,
        skipped: Skipped {
            timed_out: names(timedout),
            cloning: names(cloning),
            missing: names(missing),
            unsupported,
        },
        filters,
        alert,
        hits,
    })
}

/// The hit `item` is; `None` for a kind of result sego doesn't know.
fn hit(item: Item) -> Result<Option<Hit>, String> {
    let hit = match item {
        Item::FileMatch {
            repository,
            file,
            path_matches,
            symbols: wire_symbols,
            chunk_matches,
        } => {
            let at = pinned(&repository.name, file.commit.oid)?;
            if let Some(path) = file.path.strip_suffix('/') {
                let path = if path == "." { "" } else { path };
                return Ok(Some(Hit::Directory { at, path: path.to_owned() }));
            }
            let binary =
                chunk_matches.iter().any(|chunk| chunk.content.contains('\0'));
            let lines = if binary { Vec::new() } else { lines(chunk_matches) };
            Hit::File(FileHit {
                at,
                path: file.path,
                path_match: !path_matches.is_empty(),
                binary,
                lines,
                symbols: symbols(wire_symbols),
            })
        }
        Item::Repository { name, description, stars } => {
            let repo = RepoName::parse(&name).map_err(|e| e.to_string())?;
            Hit::Repository(RepoHit { repo, description, stars })
        }
        Item::CommitSearchResult { commit, diff_preview } => {
            Hit::Commit(CommitHit {
                at: pinned(&commit.repository.name, commit.oid)?,
                date: commit.author.date,
                author: commit.author.person.name,
                subject: commit.subject,
                diff: diff_preview.map(|diff| diff.value),
            })
        }
        Item::Other => return Ok(None),
    };
    Ok(Some(hit))
}

fn pinned(name: &str, oid: String) -> Result<Pinned, String> {
    let repo = RepoName::parse(name).map_err(|error| error.to_string())?;
    Ok(Pinned { repo, commit: CommitId(oid) })
}

/// `report` as an answer, or why there is none. A search with hits is
/// always an answer; one without is classified by its alert, then by
/// whether it was cut short.
fn classify(
    mut report: SearchReport,
    elapsed: Duration,
) -> Result<SearchReport, Box<dyn Diagnostic>> {
    if !report.hits.is_empty() {
        return Ok(report);
    }
    let bad_query = |alert: &Alert| alert.title == vocabulary::ALERT_BAD_QUERY;
    if let Some(alert) = report.alert.take_if(|alert| bad_query(alert)) {
        return Err(SearchProblem::BadQuery { alert }.into());
    }
    let alert = report.alert.as_ref();
    match alert.map(|alert| (alert.title.as_str(), &alert.description)) {
        Some((vocabulary::ALERT_NO_REPOS, _)) => return Ok(report),
        Some((vocabulary::ALERT_MISSING_REVISION, Some(description))) => {
            if let Some((repo, rev)) = missing_revision(description) {
                return Err(Unavailable::RevisionNotFound { repo, rev }.into());
            }
        }
        _ => {}
    }
    let reasons = cut_short(&report, elapsed);
    if reasons.is_empty() {
        return Ok(report);
    }
    let query = report.query;
    let reason = reasons.join(", ");
    Err(SearchProblem::Incomplete { query, reason }.into())
}

/// The repository and revision of the first `* <repo>@<rev>` line of a
/// missing-revision alert's description.
fn missing_revision(description: &str) -> Option<(String, String)> {
    description.lines().find_map(|line| {
        let (repo, rev) = line.strip_prefix("* ")?.split_once('@')?;
        Some((repo.to_owned(), rev.to_owned()))
    })
}

/// How a search was cut short, if it was: the upstream's signals of
/// partial results, and `query`'s `timeout:` when the search ran that long.
fn cut_short(report: &SearchReport, elapsed: Duration) -> Vec<String> {
    let repositories = |count: usize, one: &str, many: &str| match count {
        0 => None,
        1 => Some(format!("1 repository {one}")),
        n => Some(format!("{n} repositories {many}")),
    };
    let Skipped { timed_out, cloning, missing, .. } = &report.skipped;
    let query = report.query.as_str();
    let timeout = timeout_token(query)
        .filter(|(_, timeout)| !timeout.is_zero() && elapsed >= *timeout)
        .map(|(value, _)| {
            format!("it ran for its whole timeout:{}", &query[value])
        });
    [
        report.limit_hit.then(|| "it hit a limit".to_owned()),
        repositories(timed_out.len(), "timed out", "timed out"),
        repositories(cloning.len(), "is still cloning", "are still cloning"),
        repositories(missing.len(), "is missing", "are missing"),
        timeout,
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// The lines of a file's chunks, numbered from 1, with the spans their
/// ranges give.
fn lines(chunks: Vec<Chunk>) -> Vec<Line> {
    let mut lines = Vec::new();
    for Chunk { content, content_start, ranges } in chunks {
        let first = content_start.line;
        let mut texts: Vec<&str> = content.split('\n').collect();
        if content.ends_with('\n') {
            texts.pop();
        }
        let mut chunk: Vec<Line> = Vec::new();
        for (i, text) in texts.into_iter().enumerate() {
            let number =
                u32::try_from(i + 1).ok().and_then(|i| first.checked_add(i));
            let Some(number) = number else { break };
            let text = text.to_owned();
            chunk.push(Line { number, text, spans: Vec::new() });
        }
        for range in &ranges {
            mark(&mut chunk, range);
        }
        lines.extend(chunk);
    }
    lines
}

/// Adds `range`'s span on each of `lines` it covers. A span that runs past
/// its line is dropped, as is the empty part of a range across lines at a
/// line's end or start.
fn mark(lines: &mut [Line], range: &WireRange) {
    let WireRange { start, end } = range;
    for line in lines.iter_mut() {
        let at = line.number - 1;
        if at < start.line || at > end.line {
            continue;
        }
        let length =
            u32::try_from(line.text.chars().count()).unwrap_or(u32::MAX);
        let from = if at == start.line { start.character } else { 0 };
        let to = if at == end.line { end.character } else { length };
        if from > to || to > length || (from == to && start.line != end.line) {
            continue;
        }
        line.spans.push(Span { start: from, end: to });
    }
}

/// The symbols with a location, on 1-based lines. A symbol without one has
/// no line to show it on.
fn symbols(symbols: Vec<WireSymbol>) -> Vec<Symbol> {
    symbols
        .into_iter()
        .filter_map(|symbol| {
            let line = symbol.location.range?.start.line.checked_add(1)?;
            Some(Symbol {
                line,
                kind: vocabulary::symbol_kind(&symbol.kind),
                name: symbol.name,
                container: symbol
                    .container_name
                    .filter(|container| !container.is_empty()),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use expect_test::expect;

    use super::*;
    use crate::diagnostic::report;
    use crate::domain::{FileHit, Hit, Line, Span};
    use crate::sourcegraph::Reply;
    use crate::sourcegraph::envelope::decode;
    use crate::sourcegraph::testing;

    const MUX: &str = r"repo:^github\.com/gorilla/mux$";
    const MUX_TAG: &str = r"repo:^github\.com/gorilla/mux$@v1.7.0";

    /// The capture cases, by name: each is `tests/search/<name>.json`.
    fn cases() -> Vec<(&'static str, String)> {
        vec![
            ("content", format!("{MUX_TAG} NewRouter case:yes count:all")),
            ("symbols", format!("{MUX_TAG} type:symbol /^Use/")),
            ("paths", format!("{MUX_TAG} file:. type:path count:all")),
            (
                "directories",
                format!("{MUX_TAG} file:. select:file.directory count:all"),
            ),
            ("commits", format!("{MUX} type:commit Walk count:3")),
            ("diffs", format!("{MUX} type:diff Walk count:3")),
            ("repos", r"repo:^github\.com/gorilla/ type:repo mux".to_owned()),
            ("no-hits", format!("{MUX_TAG} zzzzunlikely")),
            ("no-repos", r"repo:^github\.com/nope/nope$ x".to_owned()),
            ("bad-query", format!("{MUX_TAG} /(/")),
            (
                "missing-revision",
                r"repo:^github\.com/gorilla/mux$@v9.9.9 NewRouter".to_owned(),
            ),
            ("timed-out", format!("{MUX_TAG} NewRouter timeout:1ms")),
            ("limit-hit", "lang:go context.WithTimeout count:5".to_owned()),
        ]
    }

    #[test]
    #[ignore = "network: run by just capture"]
    fn capture() {
        for (case, query) in cases() {
            testing::capture("search", case, &plan(&query));
        }
    }

    fn case(name: &str) -> String {
        cases()
            .into_iter()
            .find_map(|(case, query)| (case == name).then_some(query))
            .unwrap_or_else(|| panic!("no case {name}"))
    }

    /// The answer to case `name`, whose capture is `capture`.
    fn search(
        name: &str,
        capture: &str,
    ) -> Result<SearchReport, Box<dyn Diagnostic>> {
        answer(&case(name), testing::envelope(capture))
    }

    fn answered(
        result: Result<SearchReport, Box<dyn Diagnostic>>,
    ) -> SearchReport {
        match result {
            Ok(report) => report,
            Err(diagnostic) => panic!("{}", report(&*diagnostic)),
        }
    }

    /// A failure's code, outcome and report.
    fn failure(result: Result<SearchReport, Box<dyn Diagnostic>>) -> String {
        let diagnostic = match result {
            Ok(report) => panic!("an answer: {report:?}"),
            Err(diagnostic) => diagnostic,
        };
        format!(
            "[{} {:?}]\n{}",
            diagnostic.code(),
            diagnostic.outcome(),
            report(&*diagnostic)
        )
    }

    fn files(report: &SearchReport) -> Vec<&FileHit> {
        report
            .hits
            .iter()
            .map(|hit| match hit {
                Hit::File(file) => file,
                hit => panic!("not a file hit: {hit:?}"),
            })
            .collect()
    }

    /// The text each span covers.
    fn matched(line: &Line) -> Vec<String> {
        line.spans
            .iter()
            .map(|span| {
                line.text
                    .chars()
                    .skip(span.start as usize)
                    .take((span.end - span.start) as usize)
                    .collect()
            })
            .collect()
    }

    /// Each line as `number:text` for a match and `number-text` for
    /// context, with the matched text after it.
    fn grep(lines: &[Line]) -> String {
        let mut text = String::new();
        for line in lines {
            let marker = if line.spans.is_empty() { '-' } else { ':' };
            let _ = write!(text, "{}{marker}{}", line.number, line.text);
            for matched in matched(line) {
                let _ = write!(text, " [{matched}]");
            }
            text.push('\n');
        }
        text
    }

    /// A reply with a complete search's defaults, overridden by `results`.
    fn reply(results: &serde_json::Value) -> Envelope {
        let mut fields = serde_json::json!({
            "matchCount": 0, "limitHit": false,
            "approximateResultCount": "0", "elapsedMilliseconds": 0,
            "dynamicFilters": [], "cloning": [], "missing": [],
            "timedout": [], "alert": null, "results": []
        });
        for (key, value) in results.as_object().unwrap() {
            fields[key] = value.clone();
        }
        let body =
            serde_json::json!({"data": {"search": {"results": fields}}});
        decode(Reply {
            status: 200,
            body: body.to_string().into_bytes(),
            trace: None,
            location: None,
        })
        .unwrap()
    }

    /// One `FileMatch` in `github.com/x/a` at `a.go`.
    fn file_match(chunks: &serde_json::Value) -> serde_json::Value {
        serde_json::json!({
            "__typename": "FileMatch",
            "repository": {"name": "github.com/x/a"},
            "file": {"path": "a.go",
                "commit": {"oid": "a7962380ca08b5a188038c69871b8d3fbdf31e89"}},
            "pathMatches": [], "symbols": [], "chunkMatches": chunks
        })
    }

    /// The timeout `cut_short` waits for: `query`'s, unless it is zero.
    fn query_timeout(query: &str) -> Option<Duration> {
        timeout_token(query)
            .map(|(_, duration)| duration)
            .filter(|duration| !duration.is_zero())
    }

    #[test]
    fn query_timeouts() {
        let ms = Duration::from_millis;
        assert_eq!(query_timeout("x timeout:1ms"), Some(ms(1)));
        assert_eq!(query_timeout("timeout:1m30s x"), Some(ms(90_000)));
        assert_eq!(query_timeout("x"), None);
        assert_eq!(query_timeout("\"timeout:5s\""), None);
        assert_eq!(query_timeout("\"a timeout:5s\" x"), None);
        assert_eq!(query_timeout("content:\"a timeout:5s\""), None);
        assert_eq!(query_timeout("x timeout:2h"), Some(ms(7_200_000)));
        assert_eq!(query_timeout("x timeout:1.5s"), Some(ms(1500)));
        assert_eq!(query_timeout("x timeout:soon"), None);
        assert_eq!(query_timeout("x timeout:5"), None);
        assert_eq!(query_timeout("x timeout:0s"), None);
        assert_eq!(query_timeout("x mytimeout:5s"), None);
    }

    #[test]
    fn tokens_keep_quoted_spaces() {
        let query = r#"repo:x before:"2 months ago" "a b" c"#;
        assert!(has_token(query, r#"before:"2 months ago""#));
        assert!(has_token(query, r#""a b""#));
        assert!(has_token(query, "repo:x"));
        assert!(!has_token(query, "months"));
        assert!(!has_token(query, "repo"));
    }

    #[test]
    fn doubled_timeouts() {
        let doubled = |query| with_doubled_timeout(query);
        assert_eq!(doubled("x timeout:1ms").as_deref(), Some("x timeout:2ms"));
        assert_eq!(
            doubled("timeout:1m30s x").as_deref(),
            Some("timeout:3m x")
        );
        assert_eq!(doubled("x timeout:45s").as_deref(), Some("x timeout:90s"));
        assert_eq!(doubled("x timeout:0.5s").as_deref(), Some("x timeout:1s"));
        assert_eq!(doubled("x"), None);
    }

    #[test]
    fn content() {
        let report = answered(search(
            "content",
            include_str!("../../tests/search/content.json"),
        ));
        assert_eq!(report.query, case("content"));
        assert_eq!((report.match_count, report.limit_hit), (91, false));
        assert_eq!(report.approximate, None);
        let files = files(&report);
        assert_eq!(files.len(), 10);
        for file in &files {
            assert_eq!(file.at.word(), "github.com/gorilla/mux@a7962380ca08");
            for line in &file.lines {
                assert!(line.number >= 1);
                for matched in matched(line) {
                    assert_eq!(matched, "NewRouter", "{}", file.path);
                }
            }
        }
        let first = files[0];
        assert_eq!(first.path, "example_route_test.go");
        assert!(!first.path_match);
        assert_eq!(first.lines[1].number, 14);
        assert_eq!(first.lines[1].spans, [Span { start: 10, end: 19 }]);
        expect![[r#"
            13-func ExampleRoute_HeadersRegexp() {
            14:	r := mux.NewRouter() [NewRouter]
            15-	route := r.NewRoute().HeadersRegexp("Accept", "html")
            35-func ExampleRoute_HeadersRegexp_exactMatch() {
            36:	r := mux.NewRouter() [NewRouter]
            37-	route := r.NewRoute().HeadersRegexp("Origin", "^https://example.co$")
        "#]].assert_eq(&grep(&first.lines));
        expect![[r#"
            [
                Filter {
                    kind: Term {
                        value: "type",
                        raw: None,
                    },
                    value: "type:file",
                    count: 91,
                    limit_hit: true,
                },
                Filter {
                    kind: Term {
                        value: "repo",
                        raw: None,
                    },
                    value: "repo:^github\\.com/gorilla/mux$@v1.7.0",
                    count: 91,
                    limit_hit: true,
                },
                Filter {
                    kind: Term {
                        value: "lang",
                        raw: None,
                    },
                    value: "lang:go",
                    count: 73,
                    limit_hit: true,
                },
                Filter {
                    kind: Term {
                        value: "file",
                        raw: None,
                    },
                    value: "-file:_test\\.\\w+$",
                    count: 51,
                    limit_hit: true,
                },
                Filter {
                    kind: Term {
                        value: "lang",
                        raw: None,
                    },
                    value: "lang:markdown",
                    count: 18,
                    limit_hit: true,
                },
            ]
        "#]]
        .assert_debug_eq(&report.filters);
    }

    #[test]
    fn symbols() {
        let report = answered(search(
            "symbols",
            include_str!("../../tests/search/symbols.json"),
        ));
        let files = files(&report);
        assert!(files.iter().all(|file| file.lines.is_empty()));
        let symbol = &files[0].symbols[0];
        assert_eq!(files[0].path, "middleware.go");
        assert_eq!((symbol.line, symbol.name.as_str()), (24, "Use"));
        assert_eq!(symbol.kind.value, "method");
        assert_eq!(symbol.container.as_deref(), Some("mux.Router"));
        let mut found = String::new();
        for file in &files {
            for symbol in &file.symbols {
                let Symbol { line, kind, name, container } = symbol;
                let container = container.as_deref().unwrap_or_default();
                let _ = writeln!(
                    found,
                    "{}:{line} {} {name} ({container})",
                    file.path, kind.value
                );
            }
        }
        expect![[r#"
            middleware.go:24 method Use (mux.Router)
            middleware.go:31 method useInterface (mux.Router)
            mux.go:75 field useEncodedPath (mux.routeConf)
            mux.go:269 method UseEncodedPath (mux.Router)
            regexp.go:19 field useEncodedPath (mux.routeRegexpOptions)
        "#]]
        .assert_eq(&found);
        expect![[r#"
            File(
                FileHit {
                    at: Pinned {
                        repo: RepoName(
                            "github.com/gorilla/mux",
                        ),
                        commit: CommitId(
                            "a7962380ca08b5a188038c69871b8d3fbdf31e89",
                        ),
                    },
                    path: "middleware.go",
                    path_match: false,
                    binary: false,
                    lines: [],
                    symbols: [
                        Symbol {
                            line: 24,
                            kind: Term {
                                value: "method",
                                raw: None,
                            },
                            name: "Use",
                            container: Some(
                                "mux.Router",
                            ),
                        },
                        Symbol {
                            line: 31,
                            kind: Term {
                                value: "method",
                                raw: None,
                            },
                            name: "useInterface",
                            container: Some(
                                "mux.Router",
                            ),
                        },
                    ],
                },
            )
        "#]]
        .assert_debug_eq(&report.hits[0]);
    }

    #[test]
    fn paths() {
        let report = answered(search(
            "paths",
            include_str!("../../tests/search/paths.json"),
        ));
        let files = files(&report);
        assert_eq!(files.len(), 22);
        assert!(files.iter().all(|file| file.path_match
            && file.lines.is_empty()
            && file.symbols.is_empty()
            && !file.binary));
        expect![[r#"
            File(
                FileHit {
                    at: Pinned {
                        repo: RepoName(
                            "github.com/gorilla/mux",
                        ),
                        commit: CommitId(
                            "a7962380ca08b5a188038c69871b8d3fbdf31e89",
                        ),
                    },
                    path: "example_authentication_middleware_test.go",
                    path_match: true,
                    binary: false,
                    lines: [],
                    symbols: [],
                },
            )
        "#]]
        .assert_debug_eq(&report.hits[0]);
    }

    #[test]
    fn directories() {
        let report = answered(search(
            "directories",
            include_str!("../../tests/search/directories.json"),
        ));
        let paths: Vec<_> = report
            .hits
            .iter()
            .map(|hit| match hit {
                Hit::Directory { path, .. } => path.as_str(),
                hit => panic!("not a directory hit: {hit:?}"),
            })
            .collect();
        assert_eq!(paths, ["", ".github"]);
        for hit in &report.hits {
            let Hit::Directory { at, .. } = hit else { unreachable!() };
            assert_eq!(at.word(), "github.com/gorilla/mux@a7962380ca08");
        }
        expect![[r#"
            [
                Directory {
                    at: Pinned {
                        repo: RepoName(
                            "github.com/gorilla/mux",
                        ),
                        commit: CommitId(
                            "a7962380ca08b5a188038c69871b8d3fbdf31e89",
                        ),
                    },
                    path: "",
                },
                Directory {
                    at: Pinned {
                        repo: RepoName(
                            "github.com/gorilla/mux",
                        ),
                        commit: CommitId(
                            "a7962380ca08b5a188038c69871b8d3fbdf31e89",
                        ),
                    },
                    path: ".github",
                },
            ]
        "#]]
        .assert_debug_eq(&report.hits);
    }

    #[test]
    fn commits() {
        let report = answered(search(
            "commits",
            include_str!("../../tests/search/commits.json"),
        ));
        assert_eq!(report.hits.len(), 3);
        assert_eq!(report.approximate.as_deref(), Some("3+"));
        for hit in &report.hits {
            let Hit::Commit(commit) = hit else {
                panic!("not a commit hit: {hit:?}");
            };
            assert_eq!(commit.at.repo.as_str(), "github.com/gorilla/mux");
            assert_eq!(commit.at.commit.0.len(), 40);
            assert_eq!(commit.diff, None);
        }
        let Hit::Commit(first) = &report.hits[0] else { unreachable!() };
        assert_eq!(
            (first.date.as_str(), first.author.as_str()),
            ("2018-01-20T04:47:48Z", "Kamil Kisiel")
        );
        assert!(first.subject.contains("walking routes"));
        expect![[r#"
            Commit(
                CommitHit {
                    at: Pinned {
                        repo: RepoName(
                            "github.com/gorilla/mux",
                        ),
                        commit: CommitId(
                            "dc835075986d1932ee5495f32e3631770715fd6c",
                        ),
                    },
                    date: "2018-01-20T04:47:48Z",
                    author: "Kamil Kisiel",
                    subject: "[docs] README.md: Improve \"walking routes\" example. (#337) (#323)",
                    diff: None,
                },
            )
        "#]].assert_debug_eq(&report.hits[0]);
        let kinds: Vec<_> =
            report.filters.iter().map(|filter| filter.kind.value).collect();
        expect![[r#"
            [
                "type",
                "commit_date",
                "repo",
                "author",
                "author",
                "author",
            ]
        "#]]
        .assert_debug_eq(&kinds);
    }

    #[test]
    fn diffs() {
        let report = answered(search(
            "diffs",
            include_str!("../../tests/search/diffs.json"),
        ));
        assert!(!report.hits.is_empty());
        for hit in &report.hits {
            let Hit::Commit(commit) = hit else {
                panic!("not a commit hit: {hit:?}");
            };
            assert!(
                commit.diff.as_ref().is_some_and(|diff| diff.contains("Walk"))
            );
        }
        expect![[r#"
            Commit(
                CommitHit {
                    at: Pinned {
                        repo: RepoName(
                            "github.com/gorilla/mux",
                        ),
                        commit: CommitId(
                            "946b6237eb8d0ce3225f502b7fd4208d0b60ce5f",
                        ),
                    },
                    date: "2019-11-14T20:19:09Z",
                    author: "Franklin Harding",
                    subject: "Fix the CORSMethodMiddleware bug with subrouters",
                    diff: Some(
                        "middleware.go middleware.go\n@@ -60,3 +60,1 @@ func CORSMethodMiddleware(r *Router) MiddlewareFunc {\n \n-\terr := r.Walk(func(route *Route, _ *Router, _ []*Route) error {\n-\t\tfor _, m := range route.matchers {\n",
                    ),
                },
            )
        "#]].assert_debug_eq(&report.hits[0]);
    }

    #[test]
    fn repos() {
        let report = answered(search(
            "repos",
            include_str!("../../tests/search/repos.json"),
        ));
        let [Hit::Repository(repo)] = &report.hits[..] else {
            panic!("not one repository hit: {:?}", report.hits);
        };
        assert_eq!(repo.repo.as_str(), "github.com/gorilla/mux");
        assert!(repo.stars > 0);
        assert!(repo.description.contains("HTTP router"));
        expect![[r#"
            [
                Repository(
                    RepoHit {
                        repo: RepoName(
                            "github.com/gorilla/mux",
                        ),
                        description: "Package gorilla/mux is a powerful HTTP router and URL matcher for building Go web servers with 🦍",
                        stars: 21839,
                    },
                ),
            ]
        "#]].assert_debug_eq(&report.hits);
    }

    #[test]
    fn no_hits() {
        let report = answered(search(
            "no-hits",
            include_str!("../../tests/search/no-hits.json"),
        ));
        assert!(report.hits.is_empty());
        assert!(report.alert.is_none());
        expect![[r#"
            SearchReport {
                query: "repo:^github\\.com/gorilla/mux$@v1.7.0 zzzzunlikely",
                match_count: 0,
                approximate: None,
                limit_hit: false,
                skipped: Skipped {
                    timed_out: [],
                    cloning: [],
                    missing: [],
                    unsupported: 0,
                },
                filters: [],
                alert: None,
                hits: [],
            }
        "#]]
        .assert_debug_eq(&report);
    }

    #[test]
    fn no_repos() {
        let report = answered(search(
            "no-repos",
            include_str!("../../tests/search/no-repos.json"),
        ));
        assert!(report.hits.is_empty());
        let alert = report.alert.as_ref().unwrap();
        assert_eq!(alert.title, vocabulary::ALERT_NO_REPOS);
        expect![[r#"
            Some(
                Alert {
                    title: "No repositories found",
                    description: Some(
                        "Try using a different `repo:<regexp>` filter to see results",
                    ),
                    proposed: [],
                },
            )
        "#]].assert_debug_eq(&report.alert);
    }

    #[test]
    fn bad_query() {
        let result = search(
            "bad-query",
            include_str!("../../tests/search/bad-query.json"),
        );
        let failure = failure(result);
        assert!(failure.starts_with("[bad_query Invalid]\n"), "{failure}");
        expect![[r#"
            [bad_query Invalid]
            error: the upstream can't run this query
            caused by: Error parsing regexp: missing closing ): `(`
        "#]]
        .assert_eq(&failure);
    }

    #[test]
    fn missing_revision() {
        let result = search(
            "missing-revision",
            include_str!("../../tests/search/missing-revision.json"),
        );
        let failure = failure(result);
        assert!(
            failure.starts_with("[revision_not_found AnsweredNo]\n"),
            "{failure}"
        );
        expect![[r#"
            [revision_not_found AnsweredNo]
            revision v9.9.9 not found in github.com/gorilla/mux
            hint: sego list github.com/gorilla/mux
        "#]]
        .assert_eq(&failure);
    }

    #[test]
    fn timed_out() {
        let result = search(
            "timed-out",
            include_str!("../../tests/search/timed-out.json"),
        );
        let failure = failure(result);
        assert!(failure.starts_with("[incomplete Unanswered]\n"), "{failure}");
        expect![[r#"
            [incomplete Unanswered]
            error: the search was cut short with no hits: it ran for its whole timeout:1ms
            hint: sego search 'repo:^github\.com/gorilla/mux$@v1.7.0 NewRouter timeout:2ms'
        "#]].assert_eq(&failure);
    }

    #[test]
    fn limit_hit() {
        let report = answered(search(
            "limit-hit",
            include_str!("../../tests/search/limit-hit.json"),
        ));
        assert!(report.limit_hit);
        assert_eq!(report.hits.len(), 5);
        assert_eq!(report.approximate.as_deref(), Some("5+"));
        let lang = report
            .filters
            .iter()
            .find(|filter| filter.kind.value == "lang")
            .unwrap();
        assert_eq!((lang.value.as_str(), lang.count), ("lang:go", 5));
        assert!(report.filters.iter().all(|filter| filter.limit_hit));
        expect![[r#"
            [
                Filter {
                    kind: Term {
                        value: "utility",
                        raw: None,
                    },
                    value: "archived:yes",
                    count: 68678,
                    limit_hit: true,
                },
                Filter {
                    kind: Term {
                        value: "utility",
                        raw: None,
                    },
                    value: "fork:yes",
                    count: 282,
                    limit_hit: true,
                },
                Filter {
                    kind: Term {
                        value: "type",
                        raw: None,
                    },
                    value: "type:file",
                    count: 5,
                    limit_hit: true,
                },
            ]
        "#]]
        .assert_debug_eq(&&report.filters[..3]);
    }

    #[test]
    fn bad_offsets_are_dropped() {
        // "🦍x" is 2 codepoints and 5 bytes: 0..4 runs past its end.
        let body = br#"{"data":{"search":{"results":{
            "matchCount":2,"limitHit":false,"approximateResultCount":"2",
            "elapsedMilliseconds":5,"dynamicFilters":[],"cloning":[],
            "missing":[],"timedout":[],"alert":null,
            "results":[{"__typename":"FileMatch",
                "repository":{"name":"github.com/x/a"},
                "file":{"path":"a.go","commit":{
                    "oid":"a7962380ca08b5a188038c69871b8d3fbdf31e89"}},
                "pathMatches":[],"symbols":[],
                "chunkMatches":[{"content":"\ud83e\udd8dx\nab x",
                    "contentStart":{"line":4},
                    "ranges":[
                        {"start":{"line":4,"character":0},
                            "end":{"line":4,"character":4}},
                        {"start":{"line":5,"character":3},
                            "end":{"line":5,"character":4}},
                        {"start":{"line":5,"character":3},
                            "end":{"line":5,"character":9}},
                        {"start":{"line":9,"character":0},
                            "end":{"line":9,"character":1}}
                    ]}]}]
        }}}}"#;
        let reply = Reply {
            status: 200,
            body: body.to_vec(),
            trace: None,
            location: None,
        };
        let report = answered(answer("x", decode(reply).unwrap()));
        let files = files(&report);
        expect![[r#"
            5-🦍x
            6:ab x [x]
        "#]]
        .assert_eq(&grep(&files[0].lines));
        assert_eq!(files[0].lines[0].text, "🦍x");
        assert_eq!(files[0].lines[0].spans, []);
        assert_eq!(files[0].lines[1].spans, [Span { start: 3, end: 4 }]);
    }

    #[test]
    fn a_nul_in_a_chunk_makes_a_binary_hit_with_no_lines() {
        let body = br#"{"data":{"search":{"results":{
            "matchCount":1,"limitHit":false,"approximateResultCount":"1",
            "elapsedMilliseconds":5,"dynamicFilters":[],"cloning":[],
            "missing":[],"timedout":[],"alert":null,
            "results":[{"__typename":"FileMatch",
                "repository":{"name":"github.com/x/a"},
                "file":{"path":"logo.png","commit":{
                    "oid":"a7962380ca08b5a188038c69871b8d3fbdf31e89"}},
                "pathMatches":[{"start":{"line":0,"character":0},
                    "end":{"line":0,"character":4}}],
                "symbols":[{"name":"logo","kind":"CONSTANT",
                    "containerName":null,
                    "location":{"range":{"start":{"line":2,"character":0}}}}],
                "chunkMatches":[{"content":"PNG\u0000\u0001logo",
                    "contentStart":{"line":0},
                    "ranges":[{"start":{"line":0,"character":5},
                        "end":{"line":0,"character":9}}]}]}]
        }}}}"#;
        let reply = Reply {
            status: 200,
            body: body.to_vec(),
            trace: None,
            location: None,
        };
        let report = answered(answer("logo", decode(reply).unwrap()));
        assert_eq!(report.match_count, 1);
        let files = files(&report);
        let file = files[0];
        assert!(file.binary);
        assert_eq!(file.lines.len(), 0);
        assert!(file.path_match);
        assert_eq!(file.symbols.len(), 1);
        assert_eq!(file.symbols[0].line, 3);
    }

    #[test]
    fn spans_count_codepoints_outside_the_bmp() {
        let chunks = serde_json::json!([{
            "content": "🦍 NewRouter()",
            "contentStart": {"line": 0},
            "ranges": [{"start": {"line": 0, "character": 2},
                "end": {"line": 0, "character": 11}}]
        }]);
        let report = answered(answer(
            "NewRouter",
            reply(&serde_json::json!({"results": [file_match(&chunks)]})),
        ));
        let line = &files(&report)[0].lines[0];
        assert_eq!(line.spans, [Span { start: 2, end: 11 }]);
        assert_eq!(matched(line), ["NewRouter"]);
    }

    #[test]
    fn ranges_across_lines_mark_each_line() {
        let chunks = serde_json::json!([{
            "content": "a {\n  b\n}\nc",
            "contentStart": {"line": 9},
            "ranges": [{"start": {"line": 9, "character": 2},
                "end": {"line": 11, "character": 1}}]
        }]);
        let report = answered(answer(
            "x",
            reply(&serde_json::json!({"results": [file_match(&chunks)]})),
        ));
        expect![[r#"
            10:a { [{]
            11:  b [  b]
            12:} [}]
            13-c
        "#]]
        .assert_eq(&grep(&files(&report)[0].lines));
    }

    #[test]
    fn searches_cut_short_with_no_hits_are_incomplete() {
        let named = |name: &str| serde_json::json!([{"name": name}]);
        let cut = [
            ("x", serde_json::json!({"limitHit": true})),
            ("x", serde_json::json!({"timedout": named("github.com/x/a")})),
            ("x", serde_json::json!({"cloning": named("github.com/x/b")})),
            ("x", serde_json::json!({"missing": named("github.com/x/c")})),
            ("x timeout:1s", serde_json::json!({"elapsedMilliseconds": 1000})),
        ];
        let mut text = String::new();
        for (query, results) in &cut {
            text.push_str(&failure(answer(query, reply(results))));
        }
        expect![[r#"
            [incomplete Unanswered]
            error: the search was cut short with no hits: it hit a limit
            hint: sego search x
            [incomplete Unanswered]
            error: the search was cut short with no hits: 1 repository timed out
            hint: sego search x
            [incomplete Unanswered]
            error: the search was cut short with no hits: 1 repository is still cloning
            hint: sego search x
            [incomplete Unanswered]
            error: the search was cut short with no hits: 1 repository is missing
            hint: sego search x
            [incomplete Unanswered]
            error: the search was cut short with no hits: it ran for its whole timeout:1s
            hint: sego search 'x timeout:2s'
        "#]].assert_eq(&text);

        let within = reply(&serde_json::json!({"elapsedMilliseconds": 999}));
        let report = answered(answer("x timeout:1s", within));
        assert!(report.hits.is_empty());

        let chunks = serde_json::json!([]);
        let with_hits = reply(&serde_json::json!({
            "limitHit": true, "timedout": named("github.com/x/a"),
            "results": [file_match(&chunks)]
        }));
        let report = answered(answer("x", with_hits));
        assert!(report.limit_hit);
        assert_eq!(report.skipped.timed_out, ["github.com/x/a"]);
    }

    #[test]
    fn alerts_with_hits_and_unknown_alerts_stay_on_the_report() {
        let alert = |title: &str| {
            serde_json::json!({"title": title, "description": "d",
                "proposedQueries": [{"description": null, "query": "y"}]})
        };
        let chunks = serde_json::json!([]);
        for title in
            [vocabulary::ALERT_BAD_QUERY, vocabulary::ALERT_MISSING_REVISION]
        {
            let report = answered(answer(
                "x",
                reply(&serde_json::json!({
                    "alert": alert(title), "results": [file_match(&chunks)]
                })),
            ));
            assert_eq!(report.alert.unwrap().title, title);
        }
        let report = answered(answer(
            "x",
            reply(&serde_json::json!({"alert": alert("Something new")})),
        ));
        expect![[r#"
            Some(
                Alert {
                    title: "Something new",
                    description: Some(
                        "d",
                    ),
                    proposed: [
                        "y",
                    ],
                },
            )
        "#]]
        .assert_debug_eq(&report.alert);
    }

    #[test]
    fn a_missing_revision_alert_without_a_revision_is_reported() {
        let report = answered(answer(
            "x",
            reply(&serde_json::json!({"alert": {
                "title": vocabulary::ALERT_MISSING_REVISION,
                "description": "no list here", "proposedQueries": null
            }})),
        ));
        assert!(report.alert.is_some());
    }

    #[test]
    fn unknown_result_types_are_counted_as_unsupported() {
        let report = answered(answer(
            "x",
            reply(&serde_json::json!({"results": [
                {"__typename": "Notebook"},
                file_match(&serde_json::json!([])),
            ]})),
        ));
        assert_eq!(report.skipped.unsupported, 1);
        assert_eq!(report.hits.len(), 1);
    }

    #[test]
    fn a_null_search_is_bad_response() {
        let reply = Reply {
            status: 200,
            body: br#"{"data":{"search":null}}"#.to_vec(),
            trace: None,
            location: None,
        };
        let error = answer("x", decode(reply).unwrap()).unwrap_err();
        assert_eq!(error.code(), "bad_response");
    }
}
