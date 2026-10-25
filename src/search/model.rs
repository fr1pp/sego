use std::collections::{HashMap, HashSet};

use serde::Serialize;

use super::derive::{counted, is_placeholder};
use super::query::{Prepared, Unit};
use super::response::{
    Alert, CommitResult, FileMatch, Item, Range, Results, Symbol,
    UnindexedFile,
};
use crate::escape::strip_cr;
use crate::output::vocab::symbol_kind;

/// An answer: the facts sego derives, then the results it holds.
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent facts about one answer, not a state machine"
)]
pub struct Searched {
    /// The query as written.
    pub query: String,
    pub pattern_type: String,
    /// `Commits` when the query states it, or when every result is a commit
    /// and the query states no unit other than `Matches`.
    pub unit: Unit,
    pub count: u64,
    pub at_least: bool,
    pub stopped: bool,
    pub elapsed_ms: u64,
    pub limit_ms: u64,
    /// The distinct repositories among `groups`.
    pub repos: u64,
    pub widen: Vec<Widen>,
    /// `None` when it isn't known which files the index skipped.
    pub unindexed: Option<Vec<UnindexedFile>>,
    pub pinned: bool,
    pub select_cut: Option<u64>,
    pub content_search: bool,
    pub index_no: bool,
    pub repo_scoped: bool,
    /// The `type:repo` query that lists the repositories named like the
    /// query's `repo:` value (`query::repo_names`).
    pub repo_names: Option<String>,
    /// The note on a `/RE/FLAGS` term, for a zero (`lint::flag_note`).
    pub flag_note: Option<String>,
    pub alert: Option<AlertOut>,
    /// The `__typename` of each result of a kind sego doesn't read; they
    /// are not in the JSON.
    pub unshown: Vec<String>,
    pub groups: Vec<Group>,
    pub server: ServerOut,
}

/// The server's own fields, untrusted.
#[derive(Serialize)]
pub struct ServerOut {
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
pub struct AlertOut {
    pub title: String,
    pub description: Option<String>,
    pub kind: Option<String>,
    pub proposed: Vec<ProposedOut>,
}

#[derive(Serialize)]
pub struct ProposedOut {
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

fn alert_out(alert: Alert) -> AlertOut {
    AlertOut {
        title: alert.title,
        description: alert.description,
        kind: alert.kind,
        proposed: alert
            .proposed_queries
            .unwrap_or_default()
            .into_iter()
            .map(|p| ProposedOut {
                description: p.description,
                query: p.query,
            })
            .collect(),
    }
}

fn raw_chunk(start: u32, content: &str, ranges: &[Range]) -> RawChunk {
    let content = content.strip_suffix('\n').unwrap_or(content);
    RawChunk {
        start,
        lines: content
            .split('\n')
            .map(|line| strip_cr(line).0.to_owned())
            .collect(),
        ranges: ranges
            .iter()
            .map(|r| {
                [r.start.line, r.start.character, r.end.line, r.end.character]
            })
            .collect(),
    }
}

fn symbol_row(symbol: Symbol) -> SymbolRow {
    let line = symbol
        .location
        .and_then(|l| l.range)
        .and_then(|r| r.start)
        .map(|s| s.line + 1);
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
            self.raw.push(raw_chunk(
                chunk.content_start.line,
                &chunk.content,
                &chunk.ranges,
            ));
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
        let first = sl.max(raw.start);
        let end = last.min(raw.start + count(lines.len()) - 1);
        for file_line in first..=end {
            let Some(index) = file_line.checked_sub(raw.start) else {
                continue;
            };
            let Some(line) = lines.get_mut(index as usize) else {
                continue;
            };
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
pub(crate) const PREVIEW_FILES: u32 = 5;
pub(crate) const PREVIEW_HUNKS: u32 = 3;

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
        let is_diff = found
            .body
            .text
            .split('\n')
            .next()
            .is_some_and(|fence| strip_cr(fence).0 == "```diff");
        diff |= is_diff;
        let mut body: Vec<&str> = found
            .body
            .text
            .split('\n')
            .map(|l| strip_cr(l).0)
            .skip(1)
            .collect();
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
        author: author
            .person
            .as_ref()
            .and_then(|p| p.display_name.clone())
            .unwrap_or_default(),
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
    items.extend(
        symbols
            .iter()
            .filter_map(|s| s.line.map(|l| (l, 1, FileItem::Symbol(s)))),
    );
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
pub fn rows<'a>(
    chunks: &[(usize, &'a Chunk)],
    symbols: &[&'a SymbolRow],
) -> Vec<Row<'a>> {
    let mut symbols = symbols.to_vec();
    symbols.sort_by_key(|s| (s.line.is_none(), s.line));
    let mut pending = symbols.into_iter().peekable();
    let mut out = Vec::new();
    for (index, chunk) in chunks {
        for line in &chunk.lines {
            while let Some(symbol) =
                pending.next_if(|s| s.line.is_some_and(|l| l < line.number))
            {
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
            Group::File { repo, .. }
            | Group::Commit { repo, .. }
            | Group::Repo { repo } => repo,
        }
    }

    /// The units this group counts: what the text limit and the count line
    /// are measured in.
    pub fn units(&self) -> u32 {
        match self {
            Group::File { binary, chunks, symbols, .. } => {
                file_items(*binary, chunks, symbols)
                    .iter()
                    .map(FileItem::units)
                    .sum()
            }
            Group::Commit { highlights, .. } => count(highlights.len()).max(1),
            Group::Repo { .. } => 1,
        }
    }
}

/// The query's unit, or the one its results decide.
fn unit_of(prepared: &Prepared, groups: &[Group]) -> Unit {
    let all = |kind: fn(&Group) -> bool| {
        !groups.is_empty() && groups.iter().all(kind)
    };
    if prepared.by_results && all(|group| matches!(group, Group::Repo { .. }))
    {
        Unit::Repos
    } else if (prepared.by_results || prepared.unit == Unit::Matches)
        && all(|group| matches!(group, Group::Commit { .. }))
    {
        Unit::Commits
    } else {
        prepared.unit
    }
}

impl Searched {
    pub fn new(mut results: Results, prepared: &Prepared) -> Searched {
        let mut slots: Vec<Slot> = Vec::new();
        let mut unshown = Vec::new();
        let mut placeholder_ranges = 0;
        let mut index: HashMap<(String, String, String), usize> =
            HashMap::new();
        for item in std::mem::take(&mut results.results) {
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
        let unit = unit_of(prepared, &groups);
        let commits = (unit == Unit::Commits).then(|| {
            groups
                .iter()
                .filter(|group| matches!(group, Group::Commit { .. }))
                .count() as u64
        });
        let answer = counted(&results, prepared, placeholder_ranges, commits);
        let repos = groups.iter().map(Group::repo).collect::<HashSet<_>>();
        Searched {
            query: prepared.query.clone(),
            pattern_type: prepared.pattern_type.clone(),
            unit,
            count: answer.count,
            at_least: answer.at_least,
            stopped: answer.stopped,
            elapsed_ms: results.elapsed_milliseconds,
            limit_ms: prepared.deadline_ms,
            repos: repos.len() as u64,
            widen: results
                .dynamic_filters
                .into_iter()
                .filter(|f| f.kind.eq_ignore_ascii_case("utility"))
                .map(|f| Widen { filter: f.value, repos: f.count })
                .collect(),
            unindexed: None,
            pinned: prepared.pinned,
            select_cut: answer.select_cut,
            content_search: prepared.content_search,
            index_no: prepared.index_no,
            repo_scoped: prepared.repo_scoped,
            repo_names: prepared.repo_names.clone(),
            flag_note: prepared.flag_note.clone(),
            alert: results.alert.map(alert_out),
            unshown,
            groups,
            server: ServerOut {
                match_count: results.match_count,
                approximate_count: results.approximate_result_count,
                limit_hit: results.limit_hit,
                timedout: results
                    .timedout
                    .into_iter()
                    .map(|n| n.name)
                    .collect(),
            },
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

    pub fn json(&self) -> impl Serialize + '_ {
        Document {
            query: &self.query,
            pattern_type: &self.pattern_type,
            unit: self.unit.plural(),
            count: self.count,
            at_least: self.at_least,
            stopped: self.stopped,
            elapsed_ms: self.elapsed_ms,
            limit_ms: self.limit_ms,
            widen: &self.widen,
            unindexed: self.unindexed.as_deref(),
            alert: self.alert.as_ref(),
            results: self.groups.iter().flat_map(records).collect(),
            server: &self.server,
        }
    }
}

#[derive(Serialize)]
struct Document<'a> {
    query: &'a str,
    pattern_type: &'a str,
    unit: &'a str,
    count: u64,
    at_least: bool,
    stopped: bool,
    elapsed_ms: u64,
    limit_ms: u64,
    widen: &'a [Widen],
    unindexed: Option<&'a [UnindexedFile]>,
    alert: Option<&'a AlertOut>,
    results: Vec<Record<'a>>,
    server: &'a ServerOut,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Record<'a> {
    Line(LineRecord<'a>),
    Path(PathRecord<'a>),
    Binary(PathRecord<'a>),
    Symbol(SymbolRecord<'a>),
    Commit(CommitRecord<'a>),
    Repo { repo: &'a str },
}

#[derive(Serialize)]
struct LineRecord<'a> {
    repo: &'a str,
    commit: &'a str,
    rev: Option<&'a str>,
    path: &'a str,
    line: u32,
    text: &'a str,
    #[serde(rename = "match")]
    matched: bool,
    ranges: &'a [[u32; 2]],
    chunk: usize,
}

#[derive(Serialize)]
struct PathRecord<'a> {
    repo: &'a str,
    commit: &'a str,
    rev: Option<&'a str>,
    path: &'a str,
}

#[derive(Serialize)]
struct SymbolRecord<'a> {
    repo: &'a str,
    commit: &'a str,
    rev: Option<&'a str>,
    path: &'a str,
    line: Option<u32>,
    name: &'a str,
    symbol_kind: &'a str,
    container: Option<&'a str>,
}

#[derive(Serialize)]
struct CommitRecord<'a> {
    repo: &'a str,
    commit: &'a str,
    date: &'a str,
    author: &'a str,
    subject: &'a str,
    lines: &'a [String],
    highlights: &'a [[u32; 3]],
    preview_limited: bool,
}

fn records<'a>(group: &'a Group) -> Vec<Record<'a>> {
    match group {
        Group::File { repo, commit, rev, path, binary, chunks, symbols } => {
            let rev = rev.as_deref();
            let file = || PathRecord { repo, commit, rev, path };
            let mut out = Vec::new();
            match file_items(*binary, chunks, symbols).first() {
                Some(FileItem::Binary) => out.push(Record::Binary(file())),
                Some(FileItem::Path) => out.push(Record::Path(file())),
                _ => {}
            }
            let symbol_record = |symbol: &'a SymbolRow| {
                Record::Symbol(SymbolRecord {
                    repo,
                    commit,
                    rev,
                    path,
                    line: symbol.line,
                    name: &symbol.name,
                    symbol_kind: &symbol.kind,
                    container: symbol.container.as_deref(),
                })
            };
            let all_chunks: Vec<(usize, &Chunk)> =
                chunks.iter().enumerate().collect();
            let all_symbols: Vec<&SymbolRow> = symbols.iter().collect();
            for row in rows(&all_chunks, &all_symbols) {
                out.push(match row {
                    Row::Symbol(symbol) => symbol_record(symbol),
                    Row::Line(index, line) => Record::Line(LineRecord {
                        repo,
                        commit,
                        rev,
                        path,
                        line: line.number,
                        text: &line.text,
                        matched: line.matched,
                        ranges: &line.ranges,
                        chunk: index + 1,
                    }),
                });
            }
            out
        }
        Group::Commit {
            repo,
            commit,
            date,
            author,
            subject,
            lines,
            highlights,
            limited,
            ..
        } => vec![Record::Commit(CommitRecord {
            repo,
            commit,
            date,
            author,
            subject,
            lines,
            highlights,
            preview_limited: *limited,
        })],
        Group::Repo { repo } => vec![Record::Repo { repo }],
    }
}
