use std::fmt::Write as _;
use std::io;

use super::model::{
    Chunk, FileItem, Group, Row, Searched, SymbolRow, file_items, rows,
};
use super::query::{Unit, tenths};
use crate::address::pinned;
use crate::escape::{escape, one_line, render_line};
use crate::output::text::{code_line, gutter_width, header};
use crate::output::{Answer, Marker, json};

pub const TEXT_UNITS: u32 = 100;

impl Answer for Searched {
    fn write_json(&self, out: &mut dyn io::Write) -> io::Result<()> {
        json::write(&self.json(), out)
    }

    fn text(&self) -> String {
        let (results, printed) = self.results();
        let mut out = self.summary(printed);
        if let Some(line) = self.widen_line() {
            out.push_str(&line);
        }
        if !results.is_empty() {
            out.push('\n');
            out.push_str(&results);
        }
        out
    }
}

impl Searched {
    fn summary(&self, printed: u32) -> String {
        let plus = if self.at_least { "+" } else { "" };
        let counted = if printed < self.units() {
            format!("{printed} of {}{plus} {}", self.count, self.unit.plural())
        } else if self.count == 1 && !self.at_least {
            format!("1 {}", self.unit.singular())
        } else {
            format!("{}{plus} {}", self.count, self.unit.plural())
        };
        let time = tenths(self.elapsed_ms);
        let mut fields = vec![counted];
        match self.repos {
            _ if self.unit == Unit::Repos => {}
            0 => {}
            1 => fields.push("1 repo".to_owned()),
            n => fields.push(format!("{n} repos")),
        }
        fields.push(if self.stopped {
            format!("stopped at {time} s")
        } else {
            format!("{time} s")
        });
        format!("{}\nquery  {}\n", fields.join("  "), escape(&self.query))
    }

    fn widen_line(&self) -> Option<String> {
        if !self.is_zero() || self.at_least || self.widen.is_empty() {
            return None;
        }
        let values: Vec<String> = self
            .widen
            .iter()
            .map(|w| {
                let repos = if w.repos == 1 { "repo" } else { "repos" };
                format!("{} ({} {repos})", one_line(&w.filter), w.repos)
            })
            .collect();
        Some(format!("widen  {}\n", values.join("  ")))
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
                    printed += file_body(
                        &mut out,
                        &file_items(*binary, chunks, symbols),
                        printed,
                    );
                }
                Group::Commit {
                    repo,
                    commit,
                    date,
                    author,
                    subject,
                    lines,
                    highlights,
                    diff,
                } => {
                    out.push_str(&header(
                        &pinned(repo, commit, None),
                        &[date.clone(), author.clone(), subject.clone()],
                    ));
                    commit_body(&mut out, lines, highlights, *diff);
                    printed += group.units();
                }
                Group::Repo { repo } => {
                    out.push_str(&one_line(repo));
                    out.push('\n');
                    printed += group.units();
                }
            }
        }
        (out, printed)
    }
}

/// Prints the commit message lines a highlight falls on with one line of
/// context either side, `--` between gaps; a diff prints every line.
fn commit_body(
    out: &mut String,
    lines: &[String],
    highlights: &[[u32; 3]],
    diff: bool,
) {
    let mut shown = vec![diff; lines.len()];
    for h in highlights {
        let at = h[0] as usize;
        let from = at.saturating_sub(1);
        let to = (at + 1).min(lines.len().saturating_sub(1));
        for flag in shown.iter_mut().take(to + 1).skip(from) {
            *flag = true;
        }
    }
    let mut previous: Option<usize> = None;
    for (i, line) in lines.iter().enumerate().filter(|(i, _)| shown[*i]) {
        if previous.is_some_and(|p| p + 1 != i) {
            out.push_str("--\n");
        }
        previous = Some(i);
        let focus = highlights
            .iter()
            .filter(|h| h[0] as usize == i)
            .map(|h| h[1] as usize)
            .min();
        out.push_str("    ");
        out.push_str(&render_line(line, focus));
        out.push('\n');
    }
}

/// Prints the items of one file that fit the budget left after `before`
/// units; returns the units printed.
fn file_body(out: &mut String, items: &[FileItem], before: u32) -> u32 {
    let mut taken = 0;
    let mut chosen = Vec::new();
    for item in items {
        if before + taken >= TEXT_UNITS {
            break;
        }
        taken += item.units();
        chosen.push(*item);
    }

    let mut chunks: Vec<(usize, &Chunk)> = Vec::new();
    let mut symbols: Vec<&SymbolRow> = Vec::new();
    for item in &chosen {
        match item {
            FileItem::Binary => out.push_str("binary file; lines not shown\n"),
            FileItem::Path => {}
            FileItem::Chunk(index, chunk) => chunks.push((*index, chunk)),
            FileItem::Symbol(symbol) => symbols.push(symbol),
        }
    }
    let ordered = rows(&chunks, &symbols);
    let last = ordered
        .iter()
        .map(|row| match row {
            Row::Line(_, line) => line.number,
            Row::Symbol(symbol) => symbol.line.unwrap_or(0),
        })
        .max()
        .unwrap_or(0);
    let width = gutter_width(last);

    let mut previous: Option<(usize, u32)> = None;
    for row in ordered {
        match row {
            Row::Line(index, line) => {
                if previous.is_some_and(|(chunk, number)| {
                    chunk != index && number + 1 != line.number
                }) {
                    out.push_str("--\n");
                }
                let (marker, focus) = if line.matched {
                    let focus =
                        line.ranges.iter().map(|r| r[0] as usize).min();
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
                previous = Some((index, line.number));
            }
            Row::Symbol(symbol) => {
                let _ = writeln!(
                    out,
                    "{:>width$}:{}  {}",
                    symbol.line.unwrap_or(0),
                    escape(&symbol.kind),
                    one_line(&symbol.name),
                );
                previous = None;
            }
        }
    }
    taken
}
