use std::fmt::{self, Write as _};

use super::{Answer, Chunk, FileItem, Group, PREVIEW_FILES, PREVIEW_HUNKS, Row, SymbolRow, rows};
use crate::address::pinned;
use crate::commands::search::query::Unit;
use crate::escape::{escape, one_line, render_line};
use crate::output::{Marker, code_line, count, header};
use crate::sourcegraph::NOT_INDEXED;

pub const TEXT_UNITS: u32 = 100;

impl fmt::Display for Answer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (results, printed, groups) = self.results();
        let shown = if self.unit == Unit::Commits {
            let commits = |groups: &[Group]| {
                groups.iter().filter(|g| matches!(g, Group::Commit { .. })).count()
            };
            (commits(&self.groups[..groups]), commits(&self.groups))
        } else {
            (printed as usize, self.units() as usize)
        };
        f.write_str(&self.summary(shown))?;
        if let Some(line) = self.widen_line() {
            f.write_str(&line)?;
        }
        if !results.is_empty() {
            writeln!(f)?;
            f.write_str(&results)?;
        }
        Ok(())
    }
}

impl Answer {
    /// `shown` is what text printed and what it holds, in the unit.
    fn summary(&self, shown: (usize, usize)) -> String {
        let (printed, held) = shown;
        let plus = if self.at_least { "+" } else { "" };
        let counted = if printed < held {
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
        format!("{}\nquery  {}\n", fields.join("  "), escape(&self.query.text))
    }

    fn widen_line(&self) -> Option<String> {
        let zero = self.is_zero() && !self.at_least;
        if self.widen.is_empty() || !(zero || self.query.repo_scoped) {
            return None;
        }
        let values: Vec<String> = self
            .widen
            .iter()
            .map(|w| format!("{} ({})", one_line(&w.filter), count(w.repos, "repo", "repos")))
            .collect();
        Some(format!("widen  {}\n", values.join("  ")))
    }

    /// The results text limits, the units printed and the groups printed.
    fn results(&self) -> (String, u32, usize) {
        let mut out = String::new();
        let mut printed = 0;
        let mut groups = 0;
        let mut after_lines = false;
        for group in &self.groups {
            if printed >= TEXT_UNITS {
                break;
            }
            if after_lines {
                out.push('\n');
            }
            groups += 1;
            let header_end;
            match group {
                Group::File { repo, commit, path, .. } => {
                    let shown = if path.is_empty() { "./" } else { path };
                    out.push_str(&header(&pinned(repo, commit, Some(shown)), &[]));
                    header_end = out.len();
                    printed += file_body(&mut out, &group.file_items(), printed);
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
                    limited,
                } => {
                    let address = pinned(repo, commit, None);
                    out.push_str(&header(
                        &address,
                        &[date.clone(), author.clone(), subject.clone()],
                    ));
                    header_end = out.len();
                    commit_body(&mut out, lines, highlights, *diff);
                    if *limited {
                        let _ = writeln!(
                            out,
                            "    preview limited to {PREVIEW_FILES} files and \
                             {PREVIEW_HUNKS} hunks a file; sego diff \
                             {address} lists every changed file"
                        );
                    }
                    printed += group.units();
                }
                Group::Repo { repo } => {
                    out.push_str(&one_line(repo));
                    out.push('\n');
                    header_end = out.len();
                    printed += group.units();
                }
            }
            after_lines = out.len() > header_end;
        }
        (out, printed, groups)
    }
}

/// Prints the commit message lines a highlight falls on with one line of
/// context either side, `--` between gaps; a diff prints every line.
fn commit_body(out: &mut String, lines: &[String], highlights: &[[u32; 3]], diff: bool) {
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
        let focus = highlights.iter().filter(|h| h[0] as usize == i).map(|h| h[1] as usize).min();
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
            FileItem::Unindexed(reason) => {
                let _ = writeln!(out, "{NOT_INDEXED}: {}", escape(reason));
            }
            FileItem::Path => {}
            FileItem::Chunk(index, chunk) => chunks.push((*index, chunk)),
            FileItem::Symbol(symbol) => symbols.push(symbol),
        }
    }
    let ordered = rows(&chunks, &symbols);

    let mut previous: Option<(usize, u32)> = None;
    for row in ordered {
        match row {
            Row::Line(index, line) => {
                if previous
                    .is_some_and(|(chunk, number)| chunk != index && number + 1 != line.number)
                {
                    out.push_str("--\n");
                }
                let (marker, focus) = if line.matched {
                    let focus = line.ranges.iter().map(|r| r[0] as usize).min();
                    (Marker::Match, focus)
                } else {
                    (Marker::Context, None)
                };
                out.push_str(&code_line(line.number, marker, &line.text, focus));
                previous = Some((index, line.number));
            }
            Row::Symbol(symbol) => {
                let _ = writeln!(
                    out,
                    "{}:{}  {}",
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

/// `ms` in seconds, rounded to one decimal place.
pub fn tenths(ms: u64) -> String {
    let tenths = (ms + 50) / 100;
    format!("{}.{}", tenths / 10, tenths % 10)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tenths_rounds_to_one_decimal_place() {
        assert_eq!(tenths(1449), "1.4");
        assert_eq!(tenths(21_300), "21.3");
        assert_eq!(tenths(15), "0.0");
        assert_eq!(tenths(50), "0.1");
    }
}
