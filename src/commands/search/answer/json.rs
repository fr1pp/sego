use serde::{Serialize, Serializer};

use super::{Alert, Answer, Chunk, FileItem, Group, Row, Server, SymbolRow, Widen, rows};

impl Serialize for Answer {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.document().serialize(serializer)
    }
}

impl Answer {
    fn document(&self) -> Document<'_> {
        Document {
            query: &self.query.text,
            pattern_type: self.query.pattern_type_name(),
            unit: self.unit.plural(),
            count: self.count,
            at_least: self.at_least,
            stopped: self.stopped,
            elapsed_ms: self.elapsed_ms,
            limit_ms: self.query.deadline_ms,
            widen: &self.widen,
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
    alert: Option<&'a Alert>,
    results: Vec<Record<'a>>,
    server: &'a Server,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Record<'a> {
    Line(LineRecord<'a>),
    Path(PathRecord<'a>),
    Binary(PathRecord<'a>),
    Unindexed(UnindexedRecord<'a>),
    Symbol(SymbolRecord<'a>),
    Commit(CommitRecord<'a>),
    Repo { repo: &'a str },
}

#[derive(Serialize)]
struct UnindexedRecord<'a> {
    #[serde(flatten)]
    file: PathRecord<'a>,
    reason: &'a str,
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

fn records<'a>(group: &'a Group) -> Vec<Record<'a>> {
    match group {
        Group::File { repo, commit, rev, path, chunks, symbols, .. } => {
            let rev = rev.as_deref();
            let file = || PathRecord { repo, commit, rev, path };
            let mut out = Vec::new();
            match group.file_items().first() {
                Some(FileItem::Unindexed(reason)) => {
                    out.push(Record::Unindexed(UnindexedRecord { file: file(), reason }));
                }
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
            let all_chunks: Vec<(usize, &Chunk)> = chunks.iter().enumerate().collect();
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
        Group::Commit { repo, commit, date, author, subject, lines, highlights, .. } => {
            vec![Record::Commit(CommitRecord {
                repo,
                commit,
                date,
                author,
                subject,
                lines,
                highlights,
            })]
        }
        Group::Repo { repo } => vec![Record::Repo { repo }],
    }
}
