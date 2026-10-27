use std::collections::{HashMap, HashSet};

use serde::Serialize;

mod json;
mod notes;
mod text;

use super::query::{NOT_INDEXED, Query, Unit};
use crate::escape::strip_cr;
use crate::sourcegraph::{
    self, CommitResult, FileMatch, Item, Range, Results, Symbol, UnindexedFile,
};

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
    /// `None` when it isn't known which files the index skipped.
    unindexed: Option<Vec<UnindexedFile>>,
    select_cut: Option<u64>,
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

pub enum Group {
    File {
        repo: String,
        commit: String,
        rev: Option<String>,
        path: String,
        binary: bool,
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
        /// The diff preview reaches a server limit, so it may be cut: it
        /// shows 5 files, or 3 hunks of one file.
        limited: bool,
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
    binary: bool,
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
            binary: false,
            raw: Vec::new(),
            symbols: Vec::new(),
        }
    }

    fn add(&mut self, file: FileMatch) {
        if self.rev.is_none() {
            self.rev = file.rev_spec.and_then(|r| r.expr);
        }
        for chunk in &file.chunk_matches {
            if chunk.content.contains('\0') {
                self.binary = true;
            }
            self.raw.push(raw_chunk(chunk.content_start.line, &chunk.content, &chunk.ranges));
        }
        self.symbols.extend(file.symbols.into_iter().map(symbol_row));
    }

    fn finish(mut self) -> Group {
        self.symbols.sort_by_key(|s| (s.line.is_none(), s.line));
        let chunks = if self.binary {
            Vec::new()
        } else {
            merge(self.raw).into_iter().map(build_chunk).collect()
        };
        Group::File {
            repo: self.repo,
            commit: self.commit,
            rev: self.rev,
            path: self.path,
            binary: self.binary,
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

/// Sourcegraph's gitserver previews at most this many files of a diff, and
/// this many hunks of each file. The response doesn't say when it cut a
/// preview.
pub const PREVIEW_FILES: u32 = 5;
pub const PREVIEW_HUNKS: u32 = 3;

/// The preview reaches a server limit. A hunk's body is the lines after its
/// header that start with `+`, `-` or a space, until its old and new line
/// counts are spent; any other line that isn't a hunk header names a file.
/// A body can hold fewer lines than its counts, so a header ends it early.
fn preview_limited(body: &[&str]) -> bool {
    let (mut files, mut hunks) = (0, 0);
    let (mut old, mut new) = (0_u32, 0_u32);
    for line in body {
        if let Some(counts) = hunk_counts(line) {
            (old, new) = counts;
            hunks += 1;
            if hunks >= PREVIEW_HUNKS {
                return true;
            }
            continue;
        }
        let in_body = old > 0 || new > 0;
        match line.as_bytes().first() {
            Some(b'+') if in_body => new = new.saturating_sub(1),
            Some(b'-') if in_body => old = old.saturating_sub(1),
            Some(b' ') if in_body => {
                old = old.saturating_sub(1);
                new = new.saturating_sub(1);
            }
            _ => {
                (old, new) = (0, 0);
                hunks = 0;
                files += 1;
                if files >= PREVIEW_FILES {
                    return true;
                }
            }
        }
    }
    false
}

/// The old and new line counts of a hunk header, `@@ -A,B +C,D @@ ...`,
/// the form the server always writes.
fn hunk_counts(line: &str) -> Option<(u32, u32)> {
    let (ranges, _) = line.strip_prefix("@@ -")?.split_once(" @@")?;
    let (old, new) = ranges.split_once(" +")?;
    let lines = |range: &str| range.split_once(',')?.1.parse().ok();
    Some((lines(old)?, lines(new)?))
}

/// The body of each match without its fence lines, and the highlights that
/// land in it, indexed into the pooled `lines`.
fn commit_group(result: CommitResult) -> Group {
    let mut lines: Vec<String> = Vec::new();
    let mut highlights: Vec<[u32; 3]> = Vec::new();
    let mut diff = false;
    let mut limited = false;
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
        limited |= is_diff && preview_limited(&body);
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
        limited,
    }
}

/// One thing a file group prints, and the units it counts.
#[derive(Clone, Copy)]
pub enum FileItem<'a> {
    /// The file is binary: its header is followed by a note.
    Binary,
    /// The file matched by path alone: its header is the whole result.
    Path,
    Chunk(usize, &'a Chunk),
    Symbol(&'a SymbolRow),
}

impl FileItem<'_> {
    pub fn units(&self) -> u32 {
        match self {
            FileItem::Chunk(_, chunk) => chunk.units,
            FileItem::Binary | FileItem::Path | FileItem::Symbol(_) => 1,
        }
    }
}

/// The items of a file in print order: a binary or path marker first, then
/// chunks and symbols that have a line, ordered by line with a chunk before a
/// symbol that starts on the same line. A symbol with no line is not an item.
pub fn file_items<'a>(
    binary: bool,
    chunks: &'a [Chunk],
    symbols: &'a [SymbolRow],
) -> Vec<FileItem<'a>> {
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
    if binary {
        out.push(FileItem::Binary);
    } else if chunks.is_empty() && items.is_empty() {
        out.push(FileItem::Path);
    }
    out.extend(items.into_iter().map(|(.., item)| item));
    out
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
            Group::File { binary, chunks, symbols, .. } => {
                file_items(*binary, chunks, symbols).iter().map(FileItem::units).sum()
            }
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
    /// The answer to `query`: its results, and the side query's listing of
    /// the files the index skipped, when it is complete.
    pub fn new(
        query: Query,
        mut results: Results,
        unindexed: Option<Vec<UnindexedFile>>,
    ) -> Answer {
        let mut slots: Vec<Slot> = Vec::new();
        let mut unshown = Vec::new();
        let mut placeholder_ranges = 0;
        let mut index: HashMap<(String, String, String), usize> = HashMap::new();
        for item in std::mem::take(&mut results.items) {
            match item {
                Item::FileMatch(file) if is_placeholder(&file) => {
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
            unindexed,
            select_cut: answer.select_cut,
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

/// A search that comes back this close to its deadline may have been cut.
const MARGIN_MS: u64 = 1000;

/// Whether an answer that took `elapsed_ms` came back clear of the deadline.
pub fn in_time(elapsed_ms: u64, deadline_ms: u64) -> bool {
    elapsed_ms + MARGIN_MS < deadline_ms
}

/// The server's stand-in for a file the index skipped, listed as if it were
/// a match.
pub fn is_placeholder(file: &FileMatch) -> bool {
    file.symbols.is_empty()
        && matches!(
            file.chunk_matches.as_slice(),
            [chunk] if chunk.content_start.line == 0
                && chunk.content.strip_prefix(NOT_INDEXED).is_some_and(|rest| rest.starts_with(' '))
        )
}

/// What the answer holds, from the request and the response alone.
#[derive(Debug, PartialEq, Eq)]
pub struct Counted {
    pub count: u64,
    pub at_least: bool,
    pub stopped: bool,
    /// `Query::select_cut`, when it makes the answer a lower bound.
    pub select_cut: Option<u64>,
}

/// `placeholder_ranges` is how many ranges the placeholders in `results`
/// contributed to `matchCount`. The cap is compared with the raw
/// `matchCount`, since the server counted the placeholders against it.
/// `commits` is how many commit results there are when the unit is commits:
/// the count, in place of `matchCount`, which counts highlights in truncated
/// diff text. An answer to a `type:diff` query in matches counts those
/// highlights, so its count is a lower bound.
pub fn counted(
    results: &Results,
    query: &Query,
    placeholder_ranges: u64,
    commits: Option<u64>,
) -> Counted {
    let count = commits.unwrap_or_else(|| results.match_count.saturating_sub(placeholder_ranges));
    let in_time = in_time(results.elapsed_milliseconds, query.deadline_ms);
    let capped = results.limit_hit
        && results.match_count > 0
        && query.cap.is_some_and(|cap| results.match_count >= cap);
    // `limitHit` below the cap is not a cut: select:symbol, select:file.owners
    // and select:file.directory set it without one, and a cut by the clock
    // always lands past the deadline.
    let stopped = !capped && (!results.timedout.is_empty() || !in_time);
    // A zero has nothing to be a lower bound of, unless the placeholders
    // used up the cap.
    let short = if count > 0 {
        results.limit_hit
            || stopped
            || query.lower_bound
            || (query.diff && query.unit == Unit::Matches && commits.is_none())
    } else {
        capped
    };
    let select_cut = query.select_cut.filter(|_| count > 0 || query.select_drops);
    Counted { count, at_least: short || select_cut.is_some(), stopped, select_cut }
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use serde_json::{Value, json};

    use super::*;
    use crate::output::print;
    use crate::sourcegraph::Item;

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
        Answer::new(Query::new(query), results, None)
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
        assert!(print(searched, true, &mut out).is_ok(), "write failed");
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
        let s = Answer::new(Query::new("repo:x select:file.directory"), none, None);
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

    /// A diff body of `hunks.len()` files, file `i` with `hunks[i]` hunks.
    fn diff_body(hunks: &[usize]) -> String {
        let hunk = "@@ -1,2 +1,2 @@ x\n ctx\n-old\n+new\n";
        let mut body = "```diff\n".to_owned();
        for (i, &n) in hunks.iter().enumerate() {
            let _ = writeln!(body, "f{i}.go f{i}.go");
            body.push_str(&hunk.repeat(n));
        }
        body.push_str("```");
        body
    }

    fn limited(body: &str) -> bool {
        let s = searched_for("type:diff x", &json!([commit(body, &json!([]))]));
        match &s.groups[0] {
            Group::Commit { limited, .. } => *limited,
            _ => panic!("not a commit"),
        }
    }

    #[test]
    fn a_diff_of_five_files_is_limited() {
        assert!(limited(&diff_body(&[1, 1, 1, 1, 1])));
    }

    #[test]
    fn a_diff_of_four_files_of_two_hunks_is_not_limited() {
        assert!(!limited(&diff_body(&[2, 2, 1, 2])));
    }

    #[test]
    fn files_are_told_from_hunk_lines_by_the_hunk_counts() {
        let hunk = "@@ -1,1 +1,1 @@\n-o\n+n\n";
        let body = format!("```diff\na a\n{hunk}+b +b\n{hunk}-c -c\n{hunk}```");
        assert!(!limited(&body));
        let body =
            format!("```diff\na a\n{hunk}+b +b\n{hunk}-c -c\n{hunk}d d\n{hunk}+e +e\n{hunk}```");
        assert!(limited(&body));
        let body =
            format!("```diff\na a\n{hunk}+b +b\n{hunk}-c -c\n{hunk}d d\n{hunk} e  e\n{hunk}```");
        assert!(limited(&body));
    }

    #[test]
    fn the_lines_of_a_hunk_that_only_deletes_are_not_file_names() {
        let deleted = "a a\n@@ -1,4 +0,0 @@\n-1\n-2\n-3\n-4\n";
        assert!(!limited(&format!("```diff\n{deleted}```")));
    }

    #[test]
    fn a_hunk_counting_more_lines_than_it_shows_ends_at_the_next_header() {
        let short = "@@ -1,9 +1,9 @@\n ctx\n-o\n";
        let hunk = "@@ -1,1 +1,1 @@\n-o\n+n\n";
        assert!(limited(&format!("```diff\na a\n{short}{hunk}{hunk}```")));
        let files = format!("b b\n{hunk}c c\n{hunk}d d\n{hunk}e e\n{hunk}");
        assert!(limited(&format!("```diff\na a\n{short}{files}```")));
        assert!(!limited(&format!("```diff\na a\n{short}b b\n{hunk}```")));
    }

    #[test]
    fn a_hunk_header_has_both_counts() {
        let lines = "@@ -1 +1 @@\n".repeat(3);
        assert!(!limited(&format!("```diff\na a\n{lines}```")));
    }

    #[test]
    fn a_commit_message_is_not_limited() {
        let lines = "@@ a\n@@ b\n@@ c\n".repeat(2);
        assert!(!limited(&format!("```\na\n{lines}b\nc\nd\ne\n```")));
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

    fn at(match_count: u64, query: &str) -> (u64, bool, bool) {
        counted_as(match_count, false, 400, query, 0)
    }

    #[test]
    fn a_capped_answer_of_only_placeholders_is_at_least_even_at_zero() {
        let query = "repo:x binary count:30";
        assert_eq!(counted_as(30, true, 2000, query, 30), (0, true, false));
        assert_eq!(counted_as(30, false, 2000, query, 30), (0, false, false));
        assert_eq!(counted_as(29, true, 2000, query, 29), (0, false, false));
        assert_eq!(counted_as(30, true, 2000, "repo:x binary count:all", 30), (0, false, false));
        assert_eq!(counted_as(0, true, 2000, "repo:x binary count:0", 0), (0, false, false));
    }

    #[test]
    fn a_directory_count_is_a_lower_bound_unless_count_all() {
        let rows = [
            ("repo:x select:file.directory foo", 4, true),
            ("repo:x select:file.directory foo count:30", 30, true),
            ("repo:x select:file.directory foo count:100", 19, true),
            ("repo:x select:file.directory foo count:all", 4, false),
        ];
        for (query, count, at_least) in rows {
            assert_eq!(at(count, query), (count, at_least, false), "{query}");
        }
    }

    #[test]
    fn a_repo_select_wider_than_one_repository_is_a_lower_bound() {
        let rows = [
            ("lang:python foo select:repo count:all timeout:60s", true),
            ("foo select:repo", true),
            ("-repo:x foo select:repo", true),
            ("foo or bar select:repo", true),
            ("foo select:repo.topics", true),
            ("repo:x foo select:repo", true),
            (r"repo:^github\.com/vuejs/ foo select:repo", true),
            (r"repo:^github\.com/gorilla/(mux|handlers)$ foo select:repo", true),
            (r"repo:^github\.com/gorilla/mux.*$ foo select:repo", true),
            (r"repo:^github\.com/gorilla/ repo:mux foo select:repo", true),
            (r"repo:has.placeholder_file(path:go\.mod) foo select:repo", true),
            (
                r"(repo:^github\.com/gorilla/mux$ foo) or (repo:^github\.com/gorilla/handlers$ foo) select:repo",
                true,
            ),
            (r"(repo:^github\.com/gorilla/mux$ foo) or (bar) select:repo", true),
            (r"repo:^github\.com/gorilla/mux$ foo select:repo", false),
            (r"r:^github\.com/gorilla/mux$ foo select:repo", false),
            (r#"repo:"^github\\.com/gorilla/mux$" foo select:repo"#, false),
            (r"repo:^github\.com/gorilla/mux$@v1.8.0 foo select:repo", false),
            (r"repo:^github\.com/gorilla/mux$ foo select:repo.topics", false),
            (r"repo:^github\.com/gorilla/ repo:^github\.com/gorilla/mux$ foo select:repo", false),
            (
                r"(repo:^github\.com/gorilla/mux$ foo) or (repo:^github\.com/gorilla/mux$ bar) select:repo",
                false,
            ),
            ("foo -select:repo", false),
            ("foo select:file", false),
            (r"-repo:^github\.com/gorilla/mux$ foo select:repo", true),
            (r"repo:^github\.com/gorilla/mux$@v1:v2 foo select:repo", false),
            ("repo:^mux$ foo select:repo", false),
            ("repo:^github.com/gorilla/mux$ foo select:repo", false),
            (r"repo:^github\.com/gorilla/my\_mux$ foo select:repo", false),
            (
                r"repo:^github\.com/gorilla/mux$ repo:has.placeholder_file(path:go\.mod) foo select:repo",
                false,
            ),
            (r"repo:^a/b$ repo:^c/d$ foo select:repo", false),
            (r"(repo:^a/b$ foo) or (repo:^c/d$ foo) select:repo", true),
            (r"foo or repo:^a/b$ bar select:repo", true),
            ("repo:^/$ foo select:repo", true),
            (r"repo:^github\.com/$ foo select:repo", true),
            ("repo:^./.$ foo select:repo", true),
            ("repo:^a//b$ foo select:repo", true),
            ("type:repo foo", false),
        ];
        for (query, at_least) in rows {
            assert_eq!(at(247, query), (247, at_least, false), "{query}");
        }
        assert_eq!(at(0, "foo select:repo"), (0, false, false));
        assert_eq!(at(0, r"repo:^a/b$ foo select:repo"), (0, false, false));
    }

    #[test]
    fn a_repo_select_with_more_branches_than_read_is_a_lower_bound() {
        let branch = r"(repo:^a/b$ x)";
        let mut query = [branch; 65].join(" or ");
        query.push_str(" select:repo");
        assert_eq!(at(2, &query), (2, true, false));
        let mut query = [branch; 3].join(" or ");
        query.push_str(" select:repo");
        assert_eq!(at(2, &query), (2, false, false));
    }

    #[test]
    fn a_pinned_select_answer_is_a_lower_bound_unless_count_all() {
        let rows = [
            ("repo:x@v1 Router select:file", true),
            ("repo:x@v1 type:symbol /./ select:symbol.method count:50", true),
            ("repo:x@v1 Router select:file count:all", false),
            ("repo:x Router select:file count:30", false),
            ("repo:x -rev:v1 Router select:file", false),
            (r"repo:^github\.com/x/y$@v1 Router select:repo", false),
            (r"repo:^github\.com/x/y$ rev:v1 Router select:repo.topics", false),
            ("repo:x@HEAD Router select:file", false),
            ("repo:x rev:HEAD Router select:file", false),
            ("repo:x@main Router select:file", true),
        ];
        for (query, at_least) in rows {
            assert_eq!(at(2, query), (2, at_least, false), "{query}");
        }
    }

    #[test]
    fn a_pinned_zero_is_a_lower_bound_only_when_the_select_drops_results() {
        let rows = [
            ("repo:x@v1 type:symbol /./ select:symbol.method", true),
            ("repo:x@v1 type:symbol /./ select:symbol", true),
            ("repo:x@v1 Router select:content", true),
            ("repo:x@v1 Router select:file", false),
            ("repo:x@v1 Router select:file.directory", false),
            (r"repo:^github\.com/x/y$@v1 Router select:repo", false),
            ("repo:x@v1 Router select:content count:all", false),
        ];
        for (query, at_least) in rows {
            assert_eq!(at(0, query), (0, at_least, false), "{query}");
        }
    }

    fn placeholder_file(chunks: &Value, symbols: &Value) -> Value {
        json!({
            "__typename": "FileMatch",
            "repository": {"name": "r/a"},
            "file": {"path": "big.bin", "commit": {"oid": "c1"}},
            "revSpec": null,
            "chunkMatches": chunks,
            "symbols": symbols,
        })
    }

    fn placeholder_chunk(content: &str, line: u32) -> Value {
        json!({"content": content, "contentStart": {"line": line},
           "ranges": []})
    }

    fn is(value: &Value) -> bool {
        let item: Item =
            serde_json::from_value(value.clone()).unwrap_or_else(|e| panic!("bad fixture: {e}"));
        match item {
            Item::FileMatch(file) => is_placeholder(&file),
            _ => panic!("not a file match"),
        }
    }

    #[test]
    fn a_placeholder_is_one_not_indexed_line_at_line_zero_with_no_symbols() {
        let text = "NOT-INDEXED: exceeds the maximum size limit";
        assert!(is(&placeholder_file(&json!([placeholder_chunk(text, 0)]), &json!([]))));
        assert!(!is(&placeholder_file(&json!([placeholder_chunk(text, 3)]), &json!([]))));
        assert!(!is(&placeholder_file(
            &json!([placeholder_chunk("not-indexed: x", 0)]),
            &json!([])
        )));
        assert!(!is(&placeholder_file(&json!([]), &json!([]))));
        assert!(!is(&placeholder_file(
            &json!([placeholder_chunk(text, 0), placeholder_chunk(text, 0)]),
            &json!([])
        )));
        let symbol = json!({"name": "S", "kind": "FUNCTION",
        "containerName": null, "location": null});
        assert!(!is(&placeholder_file(&json!([placeholder_chunk(text, 0)]), &json!([symbol]))));
    }
}
