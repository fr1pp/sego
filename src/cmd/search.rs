use std::fmt::Write as _;

use serde::Serialize;

use super::{Command, Help, Present, UsageError};
use crate::args::Cli;
use crate::diagnostic::{Diagnostic, Outcome};
use crate::domain::{
    Alert, CommitHit, CutShort, FileHit, Filter, Hit, Line, RepoHit,
    SearchReport, Skipped, Symbol, Term,
};
use crate::escape::{command_line, content_line, sanitise};
use crate::render::json::{Partial, document};
use crate::render::text::{
    Marker, address_line, hint, notice, numbered, path_word,
};
use crate::sourcegraph::Fetch;
use crate::sourcegraph::envelope::decode;
use crate::sourcegraph::query::has_token;
use crate::sourcegraph::search;

pub const HELP: Help = Help {
    about: "\
sego search prints the code, paths, symbols, commits and repositories that
match a Sourcegraph query.
",
    synopsis: "[--json] <query>...",
    details: "\
Operands:
  <query>   a Sourcegraph search query. The operands are joined with spaces
            and sent as they are. A token that starts with a single dash,
            such as -file:_test, is part of the query.

Output:
  A summary line: the number of matches, with + when the upstream counted
  only some, and the number of results. Then, when there is one, a line
  each:

    alert:      the upstream's message about the query, then its proposed
                queries as hint: lines
    notice:     that results may be missing, and why: the search hit a
                limit, such as its count:, repositories timed out, are
                still cloning or are missing, or it ran for its whole
                timeout; then the search with a longer timeout: as a hint:
                line, when that could help
    notice:     how many results were of a kind sego doesn't know
    narrow:     when a limit was hit, filters to add and their match counts
    widen:      when nothing matched, filters that bring in repositories the
                search left out, such as archived:yes

  Then the results, in the upstream's order, under a line naming their
  repository and commit, repeated whenever it changes:

    github.com/gorilla/mux@a7962380ca08
    mux.go
      22-
      23: // NewRouter returns a new router instance.
      24: func NewRouter() *Router {
      25-     return &Router{namedRoutes: make(map[string]*Route)}

  A file's lines are N: for a line that matches, N- for context, and --
  between runs that aren't adjacent; its symbols are N: kind name
  (container); a binary file shows none. A path match is the path alone,
  and a directory ends in /. A commit is its commit, date, author and
  subject, then any diff below it. A repository is its name and stars,
  then its description. A line wider than 500 columns is cut, and says
  so; --json has it whole.

  With --json, one document of kind \"search\": query, match_count,
  approximate (null when exact), limit_hit, skipped {timed_out, cloning,
  missing, unsupported}, filters [{kind, value, count, limit_hit}] when
  shown, alert {title, description, hints} or null, partial {reason,
  hints} when results may be missing, and results. Each
  result has a type: file {repo, commit, path, path_match, binary, lines
  [{n, text, matches [{start, end}]}], symbols [{line, kind, name,
  container}]}, directory {repo, commit, path}, commit {repo, commit,
  date, author, subject, diff} or repository {repo, description, stars}.
  Commits have 40 hex digits; match offsets count codepoints. Anything
  else is reported on stderr, and its document, of kind \"unavailable\" or
  \"error\", also goes to stdout.

Matching:
  Matching is case-insensitive unless the query says case:yes. A misspelt
  filter or an unbalanced quote becomes search text and matches nothing,
  silently. A search runs for its timeout:, up to 60s; without one, for
  20s, or 60s with count:. A search with no results under a short timeout:
  may have been cut short; when it ran for its whole timeout, it is
  unanswered.

Exit codes:
  0  answered      the results
  1  answered no   no results from a complete search; no repository
                   matches; a revision that doesn't exist
  2  invalid       no query; a query the upstream can't run; an invalid
                   SEGO_ENDPOINT
  3  unanswered    a search cut short with no results, or no usable answer
                   from the upstream; retry, or narrow and retry

Examples:
  sego search NewRouter
  sego search 'func NewRouter'
  sego search 'repo:^github\\.com/gorilla/mux$' NewRouter
  sego search -file:_test NewRouter
",
};

/// The query `cli` asks for: its operands joined with spaces.
pub fn request(cli: &Cli) -> Result<String, UsageError> {
    let query = cli.operands.join(" ");
    if query.trim().is_empty() {
        let message = "missing query; expected <query>...";
        return Err(UsageError::new(message, Some(Command::Search)));
    }
    Ok(query)
}

pub fn run(
    cli: &Cli,
    fetch: Fetch,
) -> Result<Box<dyn Present>, Box<dyn Diagnostic>> {
    let query = request(cli)?;
    let reply = fetch(&search::plan(&query))?;
    let report = search::answer(&query, decode(reply)?)?;
    Ok(Box::new(Search { report }))
}

pub struct Search {
    pub report: SearchReport,
}

/// The kinds of suggested filter that narrow a search: each keeps a subset
/// of what it matches. A `type` filter asks a different question instead,
/// and an unknown kind could go either way.
const NARROWING: &[&str] =
    &["repo", "file", "lang", "author", "commit_date", "symbol_type"];

/// The kind of suggested filter that widens a search, such as
/// `archived:yes`: it brings in repositories the search left out.
const WIDENING: &str = "utility";

/// The suggested filters worth showing, under their label: those that
/// narrow when a limit was hit, and those that widen when nothing matched.
/// A filter already in the query is left out.
fn suggestions(report: &SearchReport) -> Option<(&'static str, Vec<&Filter>)> {
    let widen = report.hits.is_empty();
    if !widen && !report.limit_hit {
        return None;
    }
    let helps = |kind: &str| {
        if widen { kind == WIDENING } else { NARROWING.contains(&kind) }
    };
    let filters: Vec<&Filter> = report
        .filters
        .iter()
        .filter(|filter| helps(filter.kind.value))
        .filter(|filter| !has_token(&report.query, &filter.value))
        .collect();
    let label = if widen { "widen" } else { "narrow" };
    (!filters.is_empty()).then_some((label, filters))
}

/// `n` and `one`, or `many` unless `n` is 1.
fn count(n: u64, one: &str, many: &str) -> String {
    match n {
        1 => format!("1 {one}"),
        n => format!("{n} {many}"),
    }
}

fn search_command(query: &str) -> String {
    command_line(&["sego", "search"], &[query])
}

/// The alert's title and description, on one line.
fn alert_line(Alert { title, description, .. }: &Alert) -> String {
    let mut text = title.clone();
    if let Some(description) = description {
        text.push_str(": ");
        text.push_str(&description.lines().collect::<Vec<_>>().join(" "));
    }
    format!("alert: {}\n", sanitise(&text))
}

/// The notice and the hint for a search that may be missing results.
fn partial(report: &SearchReport) -> Option<Partial> {
    let CutShort { reasons, retry } = report.cut_short.as_ref()?;
    let reason = format!("results may be missing: {}", reasons.join(", "));
    let hints = retry.as_deref().map(search_command).into_iter().collect();
    Some(Partial { reason, hints })
}

/// The summary line, then a line for each thing the reader should know
/// before the results.
fn summary(report: &SearchReport) -> String {
    let SearchReport { match_count, approximate, skipped, .. } = report;
    let matches = match approximate {
        Some(approximate) => format!("{} matches", sanitise(approximate)),
        None => count(*match_count, "match", "matches"),
    };
    let results = report.hits.len() as u64;
    let mut text =
        format!("{matches} • {}\n", count(results, "result", "results"));
    if let Some(alert) = &report.alert {
        text.push_str(&alert_line(alert));
        for query in alert.queries() {
            text.push_str(&hint(&search_command(query)));
        }
    }
    if let Some(Partial { reason, hints }) = partial(report) {
        text.push_str(&notice(&reason));
        for command in hints {
            text.push_str(&hint(&command));
        }
    }
    match skipped.unsupported {
        0 => {}
        1 => text.push_str(&notice(
            "1 result of a kind sego doesn't know is not shown",
        )),
        n => text.push_str(&notice(&format!(
            "{n} results of kinds sego doesn't know are not shown"
        ))),
    }
    if let Some((label, filters)) = suggestions(report) {
        let items: Vec<_> = filters
            .iter()
            .map(|filter| format!("{} ({})", filter.value, filter.count))
            .collect();
        let _ = writeln!(text, "{label}: {}", sanitise(&items.join(" • ")));
    }
    text
}

/// The line a hit is grouped under: its repository at its commit for a
/// file or a directory, its repository for a commit, which names its own
/// commit, and none for a repository.
fn group(hit: &Hit) -> Option<String> {
    match hit {
        Hit::File(FileHit { at, .. }) | Hit::Directory { at, .. } => {
            Some(at.to_string())
        }
        Hit::Commit(CommitHit { at, .. }) => Some(at.repo.as_str().to_owned()),
        Hit::Repository(_) => None,
    }
}

/// Each line of `text` as content, indented two spaces, so that even an
/// empty one doesn't read as the blank line between hits.
fn indented(text: &str) -> String {
    let mut indented = String::new();
    for line in text.lines() {
        let _ = writeln!(indented, "  {}", content_line(line, None));
    }
    indented
}

fn line_entry(line: &Line) -> (u32, Marker, String) {
    let Line { number, text, spans } = line;
    match spans.first() {
        Some(span) => {
            let focus = usize::try_from(span.start).ok();
            (*number, Marker::Match, content_line(text, focus))
        }
        None => (*number, Marker::Context, content_line(text, None)),
    }
}

fn symbol_entry(symbol: &Symbol) -> (u32, Marker, String) {
    let Symbol { line, kind, name, container } = symbol;
    let text = match container {
        Some(container) => format!("{} {name} ({container})", kind.value),
        None => format!("{} {name}", kind.value),
    };
    (*line, Marker::Match, content_line(&text, None))
}

/// A hit's first line, and the lines below it.
fn hit_text(hit: &Hit) -> (String, String) {
    match hit {
        Hit::File(file) => {
            let head = address_line(&[&path_word(&file.path)], &[]);
            if file.binary {
                return (head, "  binary file; lines not shown\n".to_owned());
            }
            let lines: Vec<_> = file.lines.iter().map(line_entry).collect();
            let symbols: Vec<_> =
                file.symbols.iter().map(symbol_entry).collect();
            (head, numbered(&lines) + &numbered(&symbols))
        }
        Hit::Directory { path, .. } => {
            let path = path_word(path);
            (address_line(&[&format!("{path}/")], &[]), String::new())
        }
        Hit::Commit(CommitHit { at, date, author, subject, diff }) => {
            let meta = [date.clone(), author.clone(), subject.clone()];
            let head = address_line(&[at.commit.short()], &meta);
            (head, diff.as_deref().map(indented).unwrap_or_default())
        }
        Hit::Repository(RepoHit { repo, description, stars }) => {
            let stars = count(*stars, "star", "stars");
            let head = address_line(&[repo.as_str()], &[stars]);
            if description.is_empty() {
                return (head, String::new());
            }
            (head, format!("  {}\n", content_line(description, None)))
        }
    }
}

/// The hits under their group lines. A blank line comes before each group
/// line, and between hits when either has lines below it.
fn hits_text(hits: &[Hit]) -> String {
    let mut text = String::new();
    let mut current = None;
    let mut body_before = true;
    for hit in hits {
        let key = group(hit);
        let (head, body) = hit_text(hit);
        if key != current {
            text.push('\n');
            if let Some(key) = &key {
                text.push_str(&address_line(&[key], &[]));
            }
            current = key;
        } else if body_before || !body.is_empty() {
            text.push('\n');
        }
        body_before = !body.is_empty();
        text.push_str(&head);
        text.push_str(&body);
    }
    text
}

impl Present for Search {
    fn outcome(&self) -> Outcome {
        if self.report.hits.is_empty() {
            Outcome::AnsweredNo
        } else {
            Outcome::Answered
        }
    }

    fn text(&self) -> String {
        summary(&self.report) + &hits_text(&self.report.hits)
    }

    fn json(&self) -> String {
        let report = &self.report;
        let Skipped { timed_out, cloning, missing, unsupported } =
            &report.skipped;
        let body = SearchJson {
            query: &report.query,
            match_count: report.match_count,
            approximate: report.approximate.as_deref(),
            limit_hit: report.limit_hit,
            skipped: SkippedJson {
                timed_out,
                cloning,
                missing,
                unsupported: *unsupported,
            },
            filters: suggestions(report).map(|(_, filters)| {
                filters.into_iter().map(FilterJson::new).collect()
            }),
            alert: report.alert.as_ref().map(|alert| AlertJson {
                title: &alert.title,
                description: alert.description.as_deref(),
                hints: alert.queries().map(search_command).collect(),
            }),
            partial: partial(report),
            results: report.hits.iter().map(HitJson::new).collect(),
        };
        document("search", &body)
    }
}

#[derive(Serialize)]
struct SearchJson<'a> {
    query: &'a str,
    match_count: u64,
    approximate: Option<&'a str>,
    limit_hit: bool,
    skipped: SkippedJson<'a>,
    #[serde(skip_serializing_if = "Option::is_none")]
    filters: Option<Vec<FilterJson<'a>>>,
    alert: Option<AlertJson<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    partial: Option<Partial>,
    results: Vec<HitJson<'a>>,
}

#[derive(Serialize)]
struct SkippedJson<'a> {
    timed_out: &'a [String],
    cloning: &'a [String],
    missing: &'a [String],
    unsupported: u64,
}

/// The upstream's value for a field sego calls `other`.
#[derive(Serialize)]
struct KindDetail<'a> {
    kind: &'a str,
}

fn detail(term: &Term) -> Option<KindDetail<'_>> {
    term.raw.as_deref().map(|kind| KindDetail { kind })
}

#[derive(Serialize)]
struct FilterJson<'a> {
    kind: &'a str,
    value: &'a str,
    count: u64,
    limit_hit: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<KindDetail<'a>>,
}

impl<'a> FilterJson<'a> {
    fn new(filter: &'a Filter) -> FilterJson<'a> {
        FilterJson {
            kind: filter.kind.value,
            value: &filter.value,
            count: filter.count,
            limit_hit: filter.limit_hit,
            detail: detail(&filter.kind),
        }
    }
}

#[derive(Serialize)]
struct AlertJson<'a> {
    title: &'a str,
    description: Option<&'a str>,
    hints: Vec<String>,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum HitJson<'a> {
    File {
        repo: &'a str,
        commit: &'a str,
        path: &'a str,
        path_match: bool,
        binary: bool,
        lines: Vec<LineJson<'a>>,
        symbols: Vec<SymbolJson<'a>>,
    },
    Directory {
        repo: &'a str,
        commit: &'a str,
        path: &'a str,
    },
    Commit {
        repo: &'a str,
        commit: &'a str,
        date: &'a str,
        author: &'a str,
        subject: &'a str,
        diff: Option<&'a str>,
    },
    Repository {
        repo: &'a str,
        description: &'a str,
        stars: u64,
    },
}

impl<'a> HitJson<'a> {
    fn new(hit: &'a Hit) -> HitJson<'a> {
        match hit {
            Hit::File(FileHit {
                at,
                path,
                path_match,
                binary,
                lines,
                symbols,
            }) => HitJson::File {
                repo: at.repo.as_str(),
                commit: &at.commit.0,
                path,
                path_match: *path_match,
                binary: *binary,
                lines: lines.iter().map(LineJson::new).collect(),
                symbols: symbols.iter().map(SymbolJson::new).collect(),
            },
            Hit::Directory { at, path } => HitJson::Directory {
                repo: at.repo.as_str(),
                commit: &at.commit.0,
                path,
            },
            Hit::Commit(CommitHit { at, date, author, subject, diff }) => {
                HitJson::Commit {
                    repo: at.repo.as_str(),
                    commit: &at.commit.0,
                    date,
                    author,
                    subject,
                    diff: diff.as_deref(),
                }
            }
            Hit::Repository(RepoHit { repo, description, stars }) => {
                HitJson::Repository {
                    repo: repo.as_str(),
                    description,
                    stars: *stars,
                }
            }
        }
    }
}

#[derive(Serialize)]
struct LineJson<'a> {
    n: u32,
    text: &'a str,
    matches: Vec<SpanJson>,
}

impl<'a> LineJson<'a> {
    fn new(Line { number, text, spans }: &'a Line) -> LineJson<'a> {
        LineJson {
            n: *number,
            text,
            matches: spans
                .iter()
                .map(|span| SpanJson { start: span.start, end: span.end })
                .collect(),
        }
    }
}

/// A match's codepoint offsets: `start` inclusive, `end` exclusive.
#[derive(Serialize)]
struct SpanJson {
    start: u32,
    end: u32,
}

#[derive(Serialize)]
struct SymbolJson<'a> {
    line: u32,
    kind: &'a str,
    name: &'a str,
    container: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<KindDetail<'a>>,
}

impl<'a> SymbolJson<'a> {
    fn new(symbol: &'a Symbol) -> SymbolJson<'a> {
        SymbolJson {
            line: symbol.line,
            kind: symbol.kind.value,
            name: &symbol.name,
            container: symbol.container.as_deref(),
            detail: detail(&symbol.kind),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::escape::CLIP_COLUMNS;
    use crate::sourcegraph::query::UPSTREAM_MAX_TIMEOUT;

    #[test]
    fn help_states_the_clip_width() {
        let help =
            HELP.details.split_whitespace().collect::<Vec<_>>().join(" ");
        assert!(help.contains(&format!("wider than {CLIP_COLUMNS} columns")));
    }

    #[test]
    fn help_states_the_longest_timeout() {
        let help =
            HELP.details.split_whitespace().collect::<Vec<_>>().join(" ");
        let longest = UPSTREAM_MAX_TIMEOUT.as_secs();
        assert!(help.contains(&format!("timeout:, up to {longest}s")));
    }
}
