use std::collections::{HashMap, HashSet};

use serde::Serialize;

mod json;
mod notes;
mod text;

use super::query::{Query, Unit};
use crate::escape::strip_cr;
use crate::sourcegraph::{self, CommitResult, FileMatch, Item, Range, Results, Symbol};

/// An answer: the facts sego derives, then the results it holds.
pub struct Answer {
    query: Query,
    /// `Commits` when the query states it, or when every result is a commit
    /// and the query states no unit other than `Matches`.
    unit: Unit,
    count: u64,
    at_least: bool,
    stopped: bool,
    elapsed_ms: u64,
    /// The distinct repositories among `groups`.
    repos: u64,
    widen: Vec<Widen>,
    alert: Option<Alert>,
    /// The `__typename` of each result of a kind sego doesn't read; they
    /// are not in the JSON.
    unshown: Vec<String>,
    groups: Vec<Group>,
    server: Server,
}

/// The server's own fields, untrusted.
#[derive(Serialize)]
pub struct Server {
    pub match_count: u64,
    pub approximate_count: String,
    pub limit_hit: bool,
    pub timedout: Vec<String>,
}

/// A filter that would widen the search, and the repositories it would add.
#[derive(Serialize)]
pub struct Widen {
    pub filter: String,
    pub repos: u64,
}

#[derive(Serialize)]
pub struct Alert {
    pub title: String,
    pub description: Option<String>,
    pub kind: Option<String>,
    pub proposed: Vec<Proposed>,
}

#[derive(Serialize)]
pub struct Proposed {
    pub description: Option<String>,
    pub query: String,
}

/// What a file group prints in place of lines.
pub enum Marker {
    /// The file is binary.
    Binary,
    /// The file is unindexed, for this reason; only a query that names
    /// `NOT-INDEXED` keeps such a file.
    Unindexed(String),
}

pub enum Group {
    File {
        repo: String,
        commit: String,
        rev: Option<String>,
        path: String,
        marker: Option<Marker>,
        chunks: Vec<Chunk>,
        symbols: Vec<SymbolRow>,
    },
    Commit {
        repo: String,
        commit: String,
        date: String,
        author: String,
        subject: String,
        lines: Vec<String>,
        highlights: Vec<[u32; 3]>,
        /// The match body is a diff, not a commit message.
        diff: bool,
    },
    Repo {
        repo: String,
    },
}

pub struct Chunk {
    pub lines: Vec<LineOut>,
    pub units: u32,
}

pub struct LineOut {
    pub number: u32,
    pub text: String,
    pub matched: bool,
    pub ranges: Vec<[u32; 2]>,
}

pub struct SymbolRow {
    pub line: Option<u32>,
    pub name: String,
    pub kind: String,
    pub container: Option<String>,
}

/// A chunk as the server sent it, before overlapping chunks are merged.
struct RawChunk {
    start: u32,
    lines: Vec<String>,
    ranges: Vec<[u32; 4]>,
}

struct FileAcc {
    repo: String,
    commit: String,
    rev: Option<String>,
    path: String,
    marker: Option<Marker>,
    raw: Vec<RawChunk>,
    symbols: Vec<SymbolRow>,
}

enum Slot {
    File(FileAcc),
    Done(Group),
}

fn count(len: usize) -> u32 {
    u32::try_from(len).unwrap_or(u32::MAX)
}

fn alert(alert: sourcegraph::Alert) -> Alert {
    Alert {
        title: alert.title,
        description: alert.description,
        kind: alert.kind,
        proposed: alert
            .proposed_queries
            .unwrap_or_default()
            .into_iter()
            .map(|p| Proposed { description: p.description, query: p.query })
            .collect(),
    }
}

fn raw_chunk(start: u32, content: &str, ranges: &[Range]) -> RawChunk {
    let content = content.strip_suffix('\n').unwrap_or(content);
    RawChunk {
        start,
        lines: content.split('\n').map(|line| strip_cr(line).0.to_owned()).collect(),
        ranges: ranges
            .iter()
            .map(|r| [r.start.line, r.start.character, r.end.line, r.end.character])
            .collect(),
    }
}

fn symbol_row(symbol: Symbol) -> SymbolRow {
    let line = symbol.location.and_then(|l| l.range).and_then(|r| r.start).map(|s| s.line + 1);
    SymbolRow {
        line,
        name: symbol.name,
        kind: symbol_kind(&symbol.kind),
        container: symbol.container_name,
    }
}

impl FileAcc {
    fn new(file: &FileMatch) -> FileAcc {
        FileAcc {
            repo: file.repository.name.clone(),
            commit: file.file.commit.oid.clone(),
            rev: None,
            path: file.file.path.clone(),
            marker: None,
            raw: Vec::new(),
            symbols: Vec::new(),
        }
    }

    fn add(&mut self, file: FileMatch) {
        if self.rev.is_none() {
            self.rev = file.rev_spec.as_ref().and_then(|r| r.expr.clone());
        }
        if let Some(reason) = file.placeholder_reason() {
            self.marker = Some(Marker::Unindexed(reason.to_owned()));
            return;
        }
        for chunk in &file.chunk_matches {
            if chunk.content.contains('\0') && self.marker.is_none() {
                self.marker = Some(Marker::Binary);
            }
            self.raw.push(raw_chunk(chunk.content_start.line, &chunk.content, &chunk.ranges));
        }
        self.symbols.extend(file.symbols.into_iter().map(symbol_row));
    }

    fn finish(mut self) -> Group {
        self.symbols.sort_by_key(|s| (s.line.is_none(), s.line));
        let chunks = if self.marker.is_some() {
            Vec::new()
        } else {
            merge(self.raw).into_iter().map(build_chunk).collect()
        };
        Group::File {
            repo: self.repo,
            commit: self.commit,
            rev: self.rev,
            path: self.path,
            marker: self.marker,
            chunks,
            symbols: self.symbols,
        }
    }
}

/// Sorts chunks by start line and folds each one that starts at or before
/// the previous chunk's last line into it.
fn merge(mut raw: Vec<RawChunk>) -> Vec<RawChunk> {
    raw.sort_by_key(|c| c.start);
    let mut merged: Vec<RawChunk> = Vec::new();
    for chunk in raw {
        let Some(last) = merged.last_mut() else {
            merged.push(chunk);
            continue;
        };
        let end = last.start + count(last.lines.len());
        if chunk.start >= end {
            merged.push(chunk);
            continue;
        }
        let skip = (end - chunk.start) as usize;
        last.lines.extend(chunk.lines.into_iter().skip(skip));
        for range in chunk.ranges {
            if !last.ranges.contains(&range) {
                last.ranges.push(range);
            }
        }
    }
    merged
}

fn build_chunk(raw: RawChunk) -> Chunk {
    let mut lines: Vec<LineOut> = raw
        .lines
        .into_iter()
        .enumerate()
        .map(|(i, text)| LineOut {
            number: raw.start + count(i) + 1,
            text,
            matched: false,
            ranges: Vec::new(),
        })
        .collect();
    let mut units = 0;
    for [sl, sc, el, ec] in raw.ranges {
        if sl >= raw.start && ((sl - raw.start) as usize) < lines.len() {
            units += 1;
        }
        let last = if ec == 0 && el > sl { el - 1 } else { el };
        for line in &mut lines {
            let file_line = line.number - 1;
            if !(sl..=last).contains(&file_line) {
                continue;
            }
            let len = count(line.text.chars().count());
            let from = if file_line == sl { sc } else { 0 };
            let to = if file_line == el { ec } else { len };
            line.matched = true;
            line.ranges.push([from.min(len), to.min(len)]);
        }
    }
    Chunk { lines, units }
}

/// The body of each match without its fence lines, and the highlights that
/// land in it, indexed into the pooled `lines`.
fn commit_group(result: CommitResult) -> Group {
    let mut lines: Vec<String> = Vec::new();
    let mut highlights: Vec<[u32; 3]> = Vec::new();
    let mut diff = false;
    for found in &result.matches {
        let is_diff =
            found.body.text.split('\n').next().is_some_and(|fence| strip_cr(fence).0 == "```diff");
        diff |= is_diff;
        let mut body: Vec<&str> =
            found.body.text.split('\n').map(|l| strip_cr(l).0).skip(1).collect();
        if body.last().is_some_and(|l| l.starts_with("```")) {
            body.pop();
        }
        if body.last() == Some(&"") {
            body.pop();
        }
        let offset = lines.len();
        for h in &found.highlights {
            let Some(index) = (h.line as usize).checked_sub(1) else {
                continue;
            };
            if index >= body.len() {
                continue;
            }
            highlights.push([
                count(index + offset),
                h.character,
                h.character.saturating_add(h.length),
            ]);
        }
        lines.extend(body.into_iter().map(str::to_owned));
    }
    let author = &result.commit.author;
    Group::Commit {
        repo: result.commit.repository.name,
        commit: result.commit.oid,
        date: crate::output::date(&author.date),
        author: author.person.as_ref().and_then(|p| p.display_name.clone()).unwrap_or_default(),
        subject: result.commit.subject,
        lines,
        highlights,
        diff,
    }
}

/// One thing a file group prints, and the units it counts.
#[derive(Clone, Copy)]
pub enum FileItem<'a> {
    /// The file is binary: its header is followed by a note.
    Binary,
    /// The file is unindexed: its header is followed by the reason.
    Unindexed(&'a str),
    /// The file matched by path alone: its header is the whole result.
    Path,
    Chunk(usize, &'a Chunk),
    Symbol(&'a SymbolRow),
}

impl FileItem<'_> {
    pub fn units(&self) -> u32 {
        match self {
            FileItem::Chunk(_, chunk) => chunk.units,
            FileItem::Binary | FileItem::Unindexed(_) | FileItem::Path | FileItem::Symbol(_) => 1,
        }
    }
}

impl Group {
    /// The items of a file group in print order: its marker, or a path
    /// marker for a file matched by path alone, first; then chunks and
    /// symbols that have a line, ordered by line with a chunk before a symbol
    /// that starts on the same line. A symbol with no line is not an item,
    /// and a group of another kind has none.
    pub fn file_items(&self) -> Vec<FileItem<'_>> {
        let Group::File { marker, chunks, symbols, .. } = self else {
            return Vec::new();
        };
        let mut items: Vec<(u32, u8, FileItem)> = chunks
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let first = c.lines.first().map_or(0, |l| l.number);
                (first, 0, FileItem::Chunk(i, c))
            })
            .collect();
        items.extend(symbols.iter().filter_map(|s| s.line.map(|l| (l, 1, FileItem::Symbol(s)))));
        items.sort_by_key(|(line, rank, _)| (*line, *rank));
        let mut out = Vec::new();
        match marker {
            Some(Marker::Unindexed(reason)) => out.push(FileItem::Unindexed(reason)),
            Some(Marker::Binary) => out.push(FileItem::Binary),
            None if items.is_empty() => out.push(FileItem::Path),
            None => {}
        }
        out.extend(items.into_iter().map(|(.., item)| item));
        out
    }
}

/// One printed line of a file: a chunk line (with its chunk's index) or a
/// symbol row.
pub enum Row<'a> {
    Line(usize, &'a LineOut),
    Symbol(&'a SymbolRow),
}

/// Merges chunk lines and symbols by line, a chunk line before a symbol row
/// on the same line. Symbols with no line come last.
pub fn rows<'a>(chunks: &[(usize, &'a Chunk)], symbols: &[&'a SymbolRow]) -> Vec<Row<'a>> {
    let mut symbols = symbols.to_vec();
    symbols.sort_by_key(|s| (s.line.is_none(), s.line));
    let mut pending = symbols.into_iter().peekable();
    let mut out = Vec::new();
    for (index, chunk) in chunks {
        for line in &chunk.lines {
            while let Some(symbol) = pending.next_if(|s| s.line.is_some_and(|l| l < line.number)) {
                out.push(Row::Symbol(symbol));
            }
            out.push(Row::Line(*index, line));
        }
    }
    out.extend(pending.map(Row::Symbol));
    out
}

impl Group {
    pub fn repo(&self) -> &str {
        match self {
            Group::File { repo, .. } | Group::Commit { repo, .. } | Group::Repo { repo } => repo,
        }
    }

    /// The units this group counts: what the text limit and the count line
    /// are measured in.
    pub fn units(&self) -> u32 {
        match self {
            Group::File { .. } => self.file_items().iter().map(FileItem::units).sum(),
            Group::Commit { highlights, .. } => count(highlights.len()).max(1),
            Group::Repo { .. } => 1,
        }
    }
}

/// The query's unit, or the one its results decide.
fn unit_of(query: &Query, groups: &[Group]) -> Unit {
    let all = |kind: fn(&Group) -> bool| !groups.is_empty() && groups.iter().all(kind);
    if query.by_results && all(|group| matches!(group, Group::Repo { .. })) {
        Unit::Repos
    } else if (query.by_results || query.unit == Unit::Matches)
        && all(|group| matches!(group, Group::Commit { .. }))
    {
        Unit::Commits
    } else {
        query.unit
    }
}

impl Answer {
    /// The answer to `query` from its results.
    pub fn new(query: Query, mut results: Results) -> Answer {
        let mut slots: Vec<Slot> = Vec::new();
        let mut unshown = Vec::new();
        let mut placeholder_ranges = 0;
        let mut index: HashMap<(String, String, String), usize> = HashMap::new();
        for item in std::mem::take(&mut results.items) {
            match item {
                Item::FileMatch(file)
                    if !query.lists_placeholders && file.placeholder_reason().is_some() =>
                {
                    placeholder_ranges += file
                        .chunk_matches
                        .iter()
                        .map(|chunk| chunk.ranges.len() as u64)
                        .sum::<u64>();
                }
                Item::FileMatch(file) => {
                    let key = (
                        file.repository.name.clone(),
                        file.file.commit.oid.clone(),
                        file.file.path.clone(),
                    );
                    let at = *index.entry(key).or_insert_with(|| {
                        slots.push(Slot::File(FileAcc::new(&file)));
                        slots.len() - 1
                    });
                    if let Slot::File(acc) = &mut slots[at] {
                        acc.add(file);
                    }
                }
                Item::CommitSearchResult(result) => {
                    slots.push(Slot::Done(commit_group(result)));
                }
                Item::Repository(named) => {
                    slots.push(Slot::Done(Group::Repo { repo: named.name }));
                }
                Item::Other(kind) => unshown.push(kind),
            }
        }
        let groups: Vec<Group> = slots
            .into_iter()
            .map(|slot| match slot {
                Slot::File(acc) => acc.finish(),
                Slot::Done(group) => group,
            })
            .collect();
        let unit = unit_of(&query, &groups);
        let commits = (unit == Unit::Commits).then(|| {
            groups.iter().filter(|group| matches!(group, Group::Commit { .. })).count() as u64
        });
        let answer = counted(&results, &query, placeholder_ranges, commits);
        let repos = groups.iter().map(Group::repo).collect::<HashSet<_>>();
        let widen = results
            .dynamic_filters
            .into_iter()
            .filter(|f| f.kind.eq_ignore_ascii_case("utility"))
            .map(|f| Widen { filter: f.value, repos: f.count })
            .collect();
        Answer {
            unit,
            count: answer.count,
            at_least: answer.at_least,
            stopped: answer.stopped,
            elapsed_ms: results.elapsed_milliseconds,
            repos: repos.len() as u64,
            widen,
            alert: results.alert.map(alert),
            unshown,
            groups,
            server: Server {
                match_count: results.match_count,
                approximate_count: results.approximate_result_count,
                limit_hit: results.limit_hit,
                timedout: results.timedout.into_iter().map(|n| n.name).collect(),
            },
            query,
        }
    }

    /// Nothing was found: no matches counted, and no results held, of any
    /// kind.
    pub fn is_zero(&self) -> bool {
        self.count == 0 && self.groups.is_empty() && self.unshown.is_empty()
    }

    /// Every unit fetched: the budget the text output is limited by.
    pub fn units(&self) -> u32 {
        self.groups.iter().map(Group::units).sum()
    }
}

/// A `SymbolKind` value as sego names it: lower-cased, with the two
/// compound names split by an underscore.
pub fn symbol_kind(server: &str) -> String {
    match server {
        "ENUMMEMBER" => "enum_member".to_owned(),
        "TYPEPARAMETER" => "type_parameter".to_owned(),
        other => other.to_lowercase(),
    }
}

/// What the answer holds, from the request and the response alone.
#[derive(Debug, PartialEq, Eq)]
pub struct Counted {
    pub count: u64,
    pub at_least: bool,
    pub stopped: bool,
}

/// `placeholder_ranges` is how many ranges the placeholders in `results`
/// contributed to `matchCount`. `commits` is how many commit results there are
/// when the unit is commits: the count, in place of `matchCount`, which counts
/// highlights in truncated diff text.
pub fn counted(
    results: &Results,
    query: &Query,
    placeholder_ranges: u64,
    commits: Option<u64>,
) -> Counted {
    let count = commits.unwrap_or_else(|| results.match_count.saturating_sub(placeholder_ranges));
    // A search stops at its deadline, so one answered at or past it was cut.
    let stopped = !results.timedout.is_empty() || results.elapsed_milliseconds >= query.deadline_ms;
    // The raw `matchCount`, since a cut can leave nothing but placeholders.
    // A stopped zero says it stopped; agents read `0+` as doubt about the term.
    let at_least = results.match_count > 0 && (results.limit_hit || (stopped && count > 0));
    Counted { count, at_least, stopped }
}

#[cfg(test)]
mod tests {

    use serde_json::{Value, json};

    use super::*;
    use crate::output::{Notes, print};

    fn searched(items: &Value) -> Answer {
        searched_for("foo", items)
    }

    fn searched_for(query: &str, items: &Value) -> Answer {
        let Ok(results) = serde_json::from_value::<Results>(json!({
            "matchCount": 5,
            "approximateResultCount": "5",
            "limitHit": false,
            "repositoriesCount": 3,
            "timedout": [{"name": "t/x"}],
            "cloning": [],
            "missing": [],
            "alert": null,
            "elapsedMilliseconds": 12,
            "dynamicFilters": [],
            "results": items,
        })) else {
            panic!("results do not parse")
        };
        Answer::new(Query::new(query), results)
    }

    fn chunk(content: &str, start: u32, ranges: &Value) -> Value {
        json!({"content": content, "contentStart": {"line": start},
           "ranges": ranges})
    }

    fn range(sl: u32, sc: u32, el: u32, ec: u32) -> Value {
        json!({"start": {"line": sl, "character": sc},
           "end": {"line": el, "character": ec}})
    }

    fn file(path: &str, chunks: &[Value], symbols: &[Value]) -> Value {
        json!({
            "__typename": "FileMatch",
            "repository": {"name": "r/a"},
            "file": {"path": path, "commit": {"oid": "c1"}},
            "revSpec": {"expr": "main"},
            "chunkMatches": chunks,
            "symbols": symbols,
        })
    }

    fn symbol(name: &str, line: Option<u32>) -> Value {
        let location = line.map(|l| json!({"range": {"start": {"line": l}}}));
        json!({"name": name, "kind": "FUNCTION", "containerName": null,
           "location": location})
    }

    fn commit(body: &str, highlights: &Value) -> Value {
        json!({
            "__typename": "CommitSearchResult",
            "commit": {"oid": "c2", "subject": "fix", "repository": {"name": "r2"},
                       "author": {"date": "2024-05-06T07:08:09Z",
                                  "person": {"displayName": "Ann"}}},
            "matches": [{"body": {"text": body}, "highlights": highlights}],
        })
    }

    fn only_chunks(searched: &Answer) -> &[Chunk] {
        match &searched.groups[0] {
            Group::File { chunks, .. } => chunks,
            _ => panic!("not a file"),
        }
    }

    fn json_of(searched: &Answer) -> String {
        let mut out = Vec::new();
        assert!(print(searched, true, &Notes::new(), &mut out).is_ok(), "write failed");
        String::from_utf8_lossy(&out).into_owned()
    }

    fn marked(chunk: &Chunk) -> Vec<(u32, bool)> {
        chunk.lines.iter().map(|l| (l.number, l.matched)).collect()
    }

    #[test]
    fn a_chunk_marks_the_lines_its_ranges_touch() {
        let s = searched(&json!([file(
            "a.go",
            &[chunk("a\nb\nc", 41, &json!([range(42, 0, 42, 1)]))],
            &[]
        )]));
        let chunks = only_chunks(&s);
        assert_eq!(chunks.len(), 1);
        assert_eq!(marked(&chunks[0]), [(42, false), (43, true), (44, false)]);
        assert_eq!(chunks[0].units, 1);

        let s = searched(&json!([file(
            "a.go",
            &[chunk("a\nb\nc", 41, &json!([range(41, 0, 42, 1)]))],
            &[]
        )]));
        assert_eq!(marked(&only_chunks(&s)[0]), [(42, true), (43, true), (44, false)]);

        let s = searched(&json!([file(
            "a.go",
            &[chunk("a\nb\nc", 41, &json!([range(41, 0, 42, 0)]))],
            &[]
        )]));
        assert_eq!(marked(&only_chunks(&s)[0]), [(42, true), (43, false), (44, false)]);

        let s = searched(&json!([file(
            "a.go",
            &[chunk("a\nb\nc", 41, &json!([range(42, 0, 42, 0)]))],
            &[]
        )]));
        assert_eq!(marked(&only_chunks(&s)[0]), [(42, false), (43, true), (44, false)]);

        let s = searched(&json!([file(
            "a.go",
            &[chunk("a\nb\nc\nd\n", 41, &json!([range(44, 0, 44, 1)]))],
            &[]
        )]));
        assert_eq!(
            marked(&only_chunks(&s)[0]),
            [(42, false), (43, false), (44, false), (45, true)]
        );

        let s = searched(&json!([file(
            "a.go",
            &[chunk("a\nb\n", 41, &json!([range(43, 0, 43, 0)]))],
            &[]
        )]));
        let chunk = &only_chunks(&s)[0];
        assert_eq!(marked(chunk), [(42, false), (43, false)]);
        assert_eq!(chunk.units, 0);
    }

    #[test]
    fn an_answer_counts_the_units_of_each_kind_of_group() {
        let s = searched(&json!([
            file(
                "a.go",
                &[chunk("a\nb", 0, &json!([range(0, 0, 0, 1), range(1, 0, 1, 1)]))],
                &[symbol("F", Some(0)), symbol("G", Some(1)), symbol("H", None)]
            ),
            file("b.go", &[], &[]),
            file("c.bin", &[chunk("\0", 0, &json!([]))], &[]),
            json!({"__typename": "Repository", "name": "r3"}),
            commit(
                "```\nx\ny\n```",
                &json!([
                    {"line": 1, "character": 0, "length": 1},
                    {"line": 2, "character": 0, "length": 1},
                ])
            ),
            commit("```\nx\n```", &json!([])),
            json!({"__typename": "Unknown"}),
        ]));
        assert_eq!(s.units(), 2 + 2 + 1 + 1 + 1 + 2 + 1);
    }

    #[test]
    fn a_patternless_query_of_repository_results_counts_repos() {
        let repos = json!([
            {"__typename": "Repository", "name": "r/a"},
            {"__typename": "Repository", "name": "r/b"},
        ]);
        let query = r"repo:^github\.com/gorilla/mux$";
        assert_eq!(searched_for(query, &repos).unit, Unit::Repos);
        assert_eq!(searched_for("foo", &repos).unit, Unit::Matches);
        assert_eq!(searched_for("repo:x select:file", &repos).unit, Unit::Files);

        let files = json!([file("a.go", &[], &[])]);
        assert_eq!(searched_for(query, &files).unit, Unit::Files);

        let mixed = json!([repos[0], file("a.go", &[], &[])]);
        assert_eq!(searched_for(query, &mixed).unit, Unit::Files);

        assert_eq!(searched_for(query, &json!([])).unit, Unit::Files);

        let directories = json!([file("a/", &[], &[]), file("b/", &[], &[])]);
        let s = searched_for("repo:x select:file.directory", &directories);
        assert_eq!((s.unit, s.count), (Unit::Directories, 5));
        assert!(json_of(&s).contains(r#""unit":"directories","count":5,"#));
        let Ok(none) = serde_json::from_value::<Results>(json!({
            "matchCount": 0, "approximateResultCount": "0", "limitHit": false,
            "timedout": [], "alert": null, "elapsedMilliseconds": 12,
            "dynamicFilters": [], "results": [],
        })) else {
            panic!("results do not parse")
        };
        let s = Answer::new(Query::new("repo:x select:file.directory"), none);
        assert_eq!((s.unit, s.count), (Unit::Directories, 0));
        assert!(json_of(&s).contains(r#""unit":"directories","count":0,"#));
    }

    #[test]
    fn an_answer_of_commit_results_counts_commits() {
        let commits = json!([
            commit("```diff\nx\n+y\n```", &json!([{"line": 1, "character": 0, "length": 1}]),),
            commit("```\nx\n```", &json!([])),
        ]);
        for query in [
            "repo:x type:diff y",
            "repo:x type:commit",
            "repo:x type:diff select:commit.diff.added y",
            "repo:x select:commit",
        ] {
            let s = searched_for(query, &commits);
            assert_eq!((s.unit, s.count), (Unit::Commits, 2), "{query}");
            assert_eq!(s.server.match_count, 5, "{query}");
            let out = json_of(&s);
            assert!(out.contains(r#""unit":"commits","count":2,"#), "{out}");
        }
        let s = searched_for("repo:x type:diff select:repo y", &commits);
        assert_eq!((s.unit, s.count), (Unit::Repos, 5));

        let s = searched_for("(repo:x type:diff y) or z", &commits);
        assert_eq!((s.unit, s.count), (Unit::Commits, 2));

        let mixed = json!([commits[0], file("a.go", &[], &[])]);
        let s = searched_for("(repo:x type:diff y) or z", &mixed);
        assert_eq!((s.unit, s.count), (Unit::Matches, 5));
        let s = searched_for("repo:x type:diff y", &mixed);
        assert_eq!((s.unit, s.count), (Unit::Commits, 1));

        let s = searched_for("repo:x type:diff y", &json!([]));
        assert_eq!((s.unit, s.count), (Unit::Commits, 0));
        assert!(json_of(&s).contains(r#""unit":"commits","count":0,"#));
    }

    #[test]
    fn symbol_kinds_are_lower_cased_and_the_compound_ones_split() {
        let kinds = [
            "UNKNOWN",
            "FILE",
            "MODULE",
            "NAMESPACE",
            "PACKAGE",
            "CLASS",
            "METHOD",
            "PROPERTY",
            "FIELD",
            "CONSTRUCTOR",
            "ENUM",
            "INTERFACE",
            "FUNCTION",
            "VARIABLE",
            "CONSTANT",
            "STRING",
            "NUMBER",
            "BOOLEAN",
            "ARRAY",
            "OBJECT",
            "KEY",
            "NULL",
            "STRUCT",
            "EVENT",
            "OPERATOR",
        ];
        for kind in kinds {
            assert_eq!(symbol_kind(kind), kind.to_lowercase());
        }
        assert_eq!(symbol_kind("ENUMMEMBER"), "enum_member");
        assert_eq!(symbol_kind("TYPEPARAMETER"), "type_parameter");
        assert_eq!(symbol_kind("NEWKIND"), "newkind");
    }

    /// `(count, at_least, stopped)` of an answer with `match_count` matches, none
    /// of them in `results`, of which `placeholder_ranges` are placeholders'.
    fn counted_as(
        match_count: u64,
        limit_hit: bool,
        elapsed_ms: u64,
        query: &str,
        placeholder_ranges: u64,
    ) -> (u64, bool, bool) {
        let results: Results = serde_json::from_value(json!({
            "matchCount": match_count,
            "approximateResultCount": match_count.to_string(),
            "limitHit": limit_hit,
            "repositoriesCount": 0,
            "timedout": [],
            "cloning": [],
            "missing": [],
            "alert": null,
            "elapsedMilliseconds": elapsed_ms,
            "dynamicFilters": [],
            "results": [],
        }))
        .unwrap_or_else(|e| panic!("bad fixture: {e}"));
        let got = counted(&results, &Query::new(query), placeholder_ranges, None);
        (got.count, got.at_least, got.stopped)
    }

    #[test]
    fn an_answer_is_stopped_once_its_deadline_has_passed() {
        let query = "repo:x foo timeout:2s";
        assert_eq!(counted_as(17, false, 1999, query, 0), (17, false, false));
        assert_eq!(counted_as(17, false, 1001, query, 0), (17, false, false));
        assert_eq!(counted_as(17, false, 2000, query, 0), (17, true, true));
        assert_eq!(counted_as(17, false, 2200, query, 0), (17, true, true));
    }

    #[test]
    fn an_answer_is_at_least_when_the_server_reports_a_cut() {
        let rows = [
            (17, true, 17, true),
            (17, false, 17, false),
            (0, true, 0, false),
            (0, false, 0, false),
        ];
        for (match_count, limit_hit, count, at_least) in rows {
            let got = counted_as(match_count, limit_hit, 500, "repo:x foo", 0);
            assert_eq!(got, (count, at_least, false), "{match_count} {limit_hit}");
        }
    }

    #[test]
    fn a_limit_hit_answer_past_its_deadline_is_stopped() {
        assert_eq!(counted_as(30, true, 2000, "repo:x foo timeout:2s", 0), (30, true, true));
    }

    #[test]
    fn an_answer_of_only_placeholders_is_at_least_when_the_server_reports_a_cut() {
        let query = "repo:x binary";
        assert_eq!(counted_as(30, true, 500, query, 30), (0, true, false));
        assert_eq!(counted_as(30, false, 500, query, 30), (0, false, false));
    }

    #[test]
    fn a_stopped_zero_is_not_at_least() {
        let query = "repo:x binary timeout:2s";
        assert_eq!(counted_as(3, false, 2000, query, 3), (0, false, true));
    }

    #[test]
    fn what_the_query_selects_does_not_change_what_the_server_reported() {
        let rows = [
            "repo:x select:file.directory foo",
            "foo select:repo",
            "repo:x@v1 Router select:file",
            "repo:x type:diff foo",
        ];
        for query in rows {
            assert_eq!(counted_as(4, false, 500, query, 0), (4, false, false), "{query}");
        }
    }
}
