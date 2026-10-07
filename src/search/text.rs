use std::fmt::Write as _;

use super::lint::tokenize;
use super::model::{Chunk, FilterOut, Group, Searched, SymbolRow};
use super::query::seconds;
use crate::address::pinned;
use crate::escape::{escape, one_line, render_line};
use crate::output::Marker;
use crate::output::text::{code_line, gutter_width, header};

pub const TEXT_UNITS: u32 = 100;

const NARROW_VALUES: usize = 5;

enum Item<'a> {
    Chunk(&'a Chunk),
    Symbol(&'a SymbolRow, u32),
}

impl Item<'_> {
    fn key(&self) -> (u32, u8) {
        match self {
            Item::Chunk(c) => (c.lines.first().map_or(0, |l| l.number), 0),
            Item::Symbol(_, line) => (*line, 1),
        }
    }

    fn units(&self) -> u32 {
        match self {
            Item::Chunk(c) => c.units,
            Item::Symbol(..) => 1,
        }
    }

    fn last_line(&self) -> u32 {
        match self {
            Item::Chunk(c) => c.lines.last().map_or(0, |l| l.number),
            Item::Symbol(_, line) => *line,
        }
    }
}

impl Searched {
    pub fn text(&self) -> String {
        let (results, printed) = self.results();
        let mut out = self.summary(printed);
        if let Some(line) = self.filter_line() {
            out.push_str(&line);
        }
        if !results.is_empty() {
            out.push('\n');
            out.push_str(&results);
        }
        out
    }

    fn summary(&self, printed: u32) -> String {
        let count = if self.approximate_count == self.match_count.to_string() {
            self.match_count.to_string()
        } else {
            self.approximate_count.clone()
        };
        let counted = if printed < self.units() {
            format!("{printed} of {count} matches")
        } else if count == "1" {
            format!("{count} match")
        } else {
            format!("{count} matches")
        };
        let repos = if self.repo_count == 1 { "repo" } else { "repos" };
        let tenths = (self.elapsed_ms + 50) / 100;
        let mut out = format!(
            "{counted}  {} {repos}  {}.{} s, deadline {} s\n",
            self.repo_count,
            tenths / 10,
            tenths % 10,
            seconds(self.deadline_ms),
        );
        let _ = write!(out, "query  {}", escape(&self.query));
        if !self.added.is_empty() {
            let _ = write!(
                out,
                "  ({} added by sego)",
                escape(&self.added.join(" "))
            );
        }
        out.push('\n');
        out
    }

    fn filter_line(&self) -> Option<String> {
        let (label, values): (&str, Vec<&FilterOut>) =
            if self.groups.is_empty() {
                let utility =
                    self.filters.iter().filter(|f| f.kind == "utility");
                ("widen", utility.collect())
            } else {
                let written: Vec<String> = tokenize(&self.query)
                    .iter()
                    .map(|t| t.raw().to_lowercase())
                    .collect();
                let narrowing = self.filters.iter().filter(|f| {
                    f.kind != "utility"
                        && f.count != self.match_count
                        && !written.contains(&f.value.to_lowercase())
                });
                ("narrow", narrowing.take(NARROW_VALUES).collect())
            };
        if values.is_empty() {
            return None;
        }
        let values: Vec<String> = values
            .iter()
            .map(|f| format!("{} {}", one_line(&f.value), f.count))
            .collect();
        Some(format!("{label}  {}\n", values.join("  ")))
    }

    fn results(&self) -> (String, u32) {
        let mut out = String::new();
        let mut printed = 0;
        for group in &self.groups {
            if printed >= TEXT_UNITS {
                break;
            }
            match group {
                Group::File {
                    repo,
                    commit,
                    path,
                    binary,
                    chunks,
                    symbols,
                    ..
                } => {
                    let shown = if path.is_empty() { "./" } else { path };
                    out.push_str(&header(
                        &pinned(repo, commit, Some(shown)),
                        &[],
                    ));
                    if *binary {
                        out.push_str("binary file; lines not shown\n");
                        printed += 1;
                    } else if chunks.is_empty() && symbols.is_empty() {
                        printed += 1;
                    } else {
                        printed +=
                            file_body(&mut out, chunks, symbols, printed);
                    }
                }
                Group::Commit {
                    repo,
                    commit,
                    date,
                    author,
                    subject,
                    lines,
                    highlights,
                } => {
                    out.push_str(&header(
                        &pinned(repo, commit, None),
                        &[date.clone(), author.clone(), subject.clone()],
                    ));
                    for (i, line) in lines.iter().enumerate() {
                        let focus = highlights
                            .iter()
                            .find(|h| h[0] as usize == i)
                            .map(|h| h[1] as usize);
                        out.push_str("    ");
                        out.push_str(&render_line(line, focus));
                        out.push('\n');
                    }
                    printed += u32::try_from(highlights.len())
                        .unwrap_or(u32::MAX)
                        .max(1);
                }
                Group::Repo { repo } => {
                    out.push_str(&one_line(repo));
                    out.push('\n');
                    printed += 1;
                }
            }
        }
        (out, printed)
    }
}

/// Prints the chunks and line-bearing symbols of one file that fit the
/// budget left after `before` units; returns the units printed.
fn file_body(
    out: &mut String,
    chunks: &[Chunk],
    symbols: &[SymbolRow],
    before: u32,
) -> u32 {
    let mut items: Vec<Item> = chunks.iter().map(Item::Chunk).collect();
    items.extend(
        symbols.iter().filter_map(|s| s.line.map(|l| Item::Symbol(s, l))),
    );
    items.sort_by_key(Item::key);

    let mut taken = 0;
    let mut count = 0;
    for item in &items {
        if before + taken >= TEXT_UNITS {
            break;
        }
        taken += item.units();
        count += 1;
    }
    let items = &items[..count];
    let width =
        gutter_width(items.iter().map(Item::last_line).max().unwrap_or(0));

    let mut previous: Option<&Chunk> = None;
    for item in items {
        match item {
            Item::Chunk(chunk) => {
                if let Some(prev) = previous {
                    let end = prev.lines.last().map_or(0, |l| l.number);
                    let start = chunk.lines.first().map_or(0, |l| l.number);
                    if end + 1 != start {
                        out.push_str("--\n");
                    }
                }
                for line in &chunk.lines {
                    let (marker, focus) = if line.matched {
                        let focus = line.ranges.first().map(|r| r[0] as usize);
                        (Marker::Match, focus)
                    } else {
                        (Marker::Context, None)
                    };
                    out.push_str(&code_line(
                        line.number,
                        width,
                        marker,
                        &line.text,
                        focus,
                    ));
                }
                previous = Some(chunk);
            }
            Item::Symbol(symbol, line) => {
                let _ = writeln!(
                    out,
                    "{line:>width$}:{}  {}",
                    escape(&symbol.kind),
                    one_line(&symbol.name),
                );
                previous = None;
            }
        }
    }
    taken
}
