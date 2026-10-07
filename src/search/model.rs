use std::collections::HashMap;

use serde::Serialize;

use super::query::Prepared;
use super::response::{
    Alert, CommitResult, DynamicFilter, FileMatch, Item, Named, Range,
    Results, Symbol,
};
use crate::escape::strip_cr;
use crate::output::vocab::{filter_kind, symbol_kind};

pub struct Searched {
    pub query: String,
    pub added: Vec<String>,
    pub pattern_type: String,
    pub match_count: u64,
    pub approximate_count: String,
    pub limit_hit: bool,
    pub repo_count: u64,
    pub elapsed_ms: u64,
    pub deadline_ms: u64,
    pub alert: Option<AlertOut>,
    pub timedout: Vec<String>,
    pub cloning: Vec<String>,
    pub missing: Vec<String>,
    pub filters: Vec<FilterOut>,
    pub groups: Vec<Group>,
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

#[derive(Serialize)]
pub struct FilterOut {
    pub kind: String,
    pub value: String,
    pub label: String,
    pub count: u64,
    pub limit_hit: bool,
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

fn names(named: Vec<Named>) -> Vec<String> {
    named.into_iter().map(|n| n.name).collect()
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

fn filter_out(filter: DynamicFilter) -> FilterOut {
    FilterOut {
        kind: filter_kind(&filter.kind),
        value: filter.value,
        label: filter.label,
        count: filter.count,
        limit_hit: filter.limit_hit,
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
        for file_line in sl..=last {
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

/// The body of each match without its fence lines, and the highlights that
/// land in it, indexed into the pooled `lines`.
fn commit_group(result: CommitResult) -> Group {
    let mut lines: Vec<String> = Vec::new();
    let mut highlights: Vec<[u32; 3]> = Vec::new();
    for found in &result.matches {
        let mut body: Vec<&str> = found.body.text.split('\n').collect();
        if !body.is_empty() {
            body.remove(0);
        }
        if body.last().is_some_and(|l| l.starts_with("```")) {
            body.pop();
        }
        let span = body.len();
        if body.last() == Some(&"") {
            body.pop();
        }
        let offset = lines.len();
        for h in &found.highlights {
            let Some(index) = (h.line as usize).checked_sub(1) else {
                continue;
            };
            if index >= span {
                continue;
            }
            highlights.push([
                count(index + offset),
                h.character,
                h.character + h.length,
            ]);
        }
        lines.extend(body.into_iter().map(|l| strip_cr(l).0.to_owned()));
    }
    let author = &result.commit.author;
    Group::Commit {
        repo: result.commit.repository.name,
        commit: result.commit.oid,
        date: author.date.chars().take(10).collect(),
        author: author
            .person
            .as_ref()
            .and_then(|p| p.display_name.clone())
            .unwrap_or_default(),
        subject: result.commit.subject,
        lines,
        highlights,
    }
}

impl Group {
    fn units(&self) -> u32 {
        match self {
            Group::File { binary, chunks, symbols, .. } => {
                let found = chunks.iter().map(|c| c.units).sum::<u32>()
                    + count(
                        symbols.iter().filter(|s| s.line.is_some()).count(),
                    );
                if *binary || (chunks.is_empty() && symbols.is_empty()) {
                    found + 1
                } else {
                    found
                }
            }
            Group::Commit { highlights, .. } => count(highlights.len()).max(1),
            Group::Repo { .. } => 1,
        }
    }
}

impl Searched {
    pub fn new(results: Results, prepared: &Prepared) -> Searched {
        let mut slots: Vec<Slot> = Vec::new();
        let mut index: HashMap<(String, String, String), usize> =
            HashMap::new();
        for item in results.results {
            match item {
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
                Item::Other(_) => {}
            }
        }
        let groups = slots
            .into_iter()
            .map(|slot| match slot {
                Slot::File(acc) => acc.finish(),
                Slot::Done(group) => group,
            })
            .collect();
        Searched {
            query: prepared.sent.clone(),
            added: prepared.added.clone(),
            pattern_type: prepared.pattern_type.clone(),
            match_count: results.match_count,
            approximate_count: results.approximate_result_count,
            limit_hit: results.limit_hit,
            repo_count: results.repositories_count,
            elapsed_ms: results.elapsed_milliseconds,
            deadline_ms: prepared.deadline_ms,
            alert: results.alert.map(alert_out),
            timedout: names(results.timedout),
            cloning: names(results.cloning),
            missing: names(results.missing),
            filters: results
                .dynamic_filters
                .into_iter()
                .map(filter_out)
                .collect(),
            groups,
        }
    }

    /// Every unit fetched: the budget the text output is limited by.
    pub fn units(&self) -> u32 {
        self.groups.iter().map(Group::units).sum()
    }

    pub fn json(&self) -> impl Serialize + '_ {
        Document {
            query: &self.query,
            added: &self.added,
            pattern_type: &self.pattern_type,
            match_count: self.match_count,
            approximate_count: &self.approximate_count,
            limit_hit: self.limit_hit,
            repo_count: self.repo_count,
            elapsed_ms: self.elapsed_ms,
            deadline_ms: self.deadline_ms,
            alert: self.alert.as_ref(),
            timedout: &self.timedout,
            cloning: &self.cloning,
            missing: &self.missing,
            filters: &self.filters,
            results: self.groups.iter().flat_map(records).collect(),
        }
    }
}

#[derive(Serialize)]
struct Document<'a> {
    query: &'a str,
    added: &'a [String],
    pattern_type: &'a str,
    match_count: u64,
    approximate_count: &'a str,
    limit_hit: bool,
    repo_count: u64,
    elapsed_ms: u64,
    deadline_ms: u64,
    alert: Option<&'a AlertOut>,
    timedout: &'a [String],
    cloning: &'a [String],
    missing: &'a [String],
    filters: &'a [FilterOut],
    results: Vec<Record<'a>>,
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
}

fn records(group: &Group) -> Vec<Record<'_>> {
    match group {
        Group::File { repo, commit, rev, path, binary, chunks, symbols } => {
            let rev = rev.as_deref();
            let file = || PathRecord { repo, commit, rev, path };
            let mut out = Vec::new();
            if *binary {
                out.push(Record::Binary(file()));
            } else if chunks.is_empty() && symbols.is_empty() {
                out.push(Record::Path(file()));
            }
            for (i, chunk) in chunks.iter().enumerate() {
                for line in &chunk.lines {
                    out.push(Record::Line(LineRecord {
                        repo,
                        commit,
                        rev,
                        path,
                        line: line.number,
                        text: &line.text,
                        matched: line.matched,
                        ranges: &line.ranges,
                        chunk: i + 1,
                    }));
                }
            }
            for symbol in symbols {
                out.push(Record::Symbol(SymbolRecord {
                    repo,
                    commit,
                    rev,
                    path,
                    line: symbol.line,
                    name: &symbol.name,
                    symbol_kind: &symbol.kind,
                    container: symbol.container.as_deref(),
                }));
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
        } => vec![Record::Commit(CommitRecord {
            repo,
            commit,
            date,
            author,
            subject,
            lines,
            highlights,
        })],
        Group::Repo { repo } => vec![Record::Repo { repo }],
    }
}
