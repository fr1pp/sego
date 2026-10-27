use std::fmt;

use serde::Serialize;

use crate::address::{End, LineRange, Repo, Target, pinned, rev_field};
use crate::cache::{Cache, Entry};
use crate::error::Error;
use crate::escape::strip_cr;
use crate::output::{Marker, Notes, code_line, count, header, submodule_target, submodule_text};
use crate::sourcegraph::{Client, Content, File};

const DEFAULT_WINDOW: u32 = 500;

#[derive(Serialize)]
pub struct Line {
    pub line: u32,
    pub text: String,
}

/// A submodule, as the answer gives it.
#[derive(Serialize)]
pub struct Submodule {
    url: String,
    target: Option<String>,
    commit: String,
}

#[derive(Serialize)]
pub struct Answer {
    pub repo: String,
    pub rev: Option<String>,
    pub commit: String,
    pub path: String,
    pub total_lines: u32,
    pub byte_size: u64,
    pub binary: bool,
    pub crlf: bool,
    pub lfs_size: Option<u64>,
    pub submodule: Option<Submodule>,
    pub start: Option<u32>,
    pub end: Option<u32>,
    pub lines: Vec<Line>,
    pub via_raw: bool,
    #[serde(skip)]
    fields: Vec<String>,
}

impl fmt::Display for Answer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let address = pinned(&self.repo, &self.commit, Some(&self.path));
        f.write_str(&header(&address, &self.fields))?;
        for line in &self.lines {
            f.write_str(&code_line(line.line, Marker::Match, &line.text, None))?;
        }
        Ok(())
    }
}

/// What the caller asked for, as the server is asked and as it is reported.
struct Window {
    start: u32,
    end: Option<u32>,
    shown: Option<LineRange>,
}

impl Window {
    fn new(lines: Option<&LineRange>) -> Window {
        match lines {
            None => Window { start: 1, end: Some(DEFAULT_WINDOW), shown: None },
            Some(range) => Window {
                start: range.start,
                end: match range.end {
                    End::Line(end) => Some(end),
                    End::Open => None,
                },
                shown: Some(*range),
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    pub repo: Repo,
    pub target: Target,
    pub lines: Option<LineRange>,
    pub json: bool,
}

pub fn run(
    args: &Args,
    client: &Client,
    cache: &Cache,
    notes: &mut Notes,
) -> Result<Answer, Error> {
    let window = Window::new(args.lines.as_ref());
    let (at, file) = client.file(&args.repo, &args.target, window.start, window.end)?;
    let path = at.path.clone().unwrap_or_default();
    let mut shown = Answer {
        repo: at.repo.clone(),
        rev: at.rev.clone(),
        commit: at.commit.clone(),
        path: path.clone(),
        total_lines: 0,
        byte_size: 0,
        binary: false,
        crlf: false,
        lfs_size: None,
        submodule: None,
        start: None,
        end: None,
        lines: Vec::new(),
        via_raw: false,
        fields: Vec::new(),
    };
    let rev = rev_field(at.rev.as_ref());
    let blob = match file {
        File::Blob(blob) => blob,
        File::Submodule(module) => {
            let target = submodule_target(&module.url, &module.commit, &at.repo);
            shown
                .fields
                .push(format!("submodule  {}", submodule_text(target.as_deref(), &module.url)));
            shown.fields.extend(rev);
            shown.submodule = Some(Submodule { url: module.url, target, commit: module.commit });
            return Ok(shown);
        }
        File::Directory { entries } => {
            let name = if path.is_empty() { "./" } else { &path };
            return Err(Error::missing(format!(
                "{name} is a directory at {} ({})",
                pinned(&at.repo, &at.commit, None),
                count(entries, "entry", "entries")
            )));
        }
    };
    shown.byte_size = blob.details.byte_size;
    shown.binary = blob.details.binary;
    shown.lfs_size = blob.lfs_size;
    if shown.binary {
        shown.fields.push(format!("binary  {} bytes", shown.byte_size));
        shown.fields.extend(rev);
        return Ok(shown);
    }
    let total = blob.details.total_lines;
    shown.total_lines = total;
    if let Some(size) = shown.lfs_size {
        shown.fields.push(format!("LFS pointer  object {size} bytes"));
    }
    if let Some(content) = blob.content {
        let first = window.start;
        let last = window.end.unwrap_or(total).min(total);
        let content = match content {
            Content::Lines(text) => text,
            Content::OverLimit { raw } => {
                shown.via_raw = true;
                let entry = Entry { repo: &at.repo, commit: &at.commit, path: &path };
                let download = || {
                    client.raw(&raw)?.ok_or_else(|| {
                        let at = pinned(&at.repo, &at.commit, None);
                        Error::missing(format!("{path} not found in {at}"))
                    })
                };
                let (bytes, unavailable) = cache.read_through(entry, shown.byte_size, download)?;
                if let Some(reason) = unavailable {
                    notes.push(format!("cache unavailable: {reason}"));
                }
                lines_from(&String::from_utf8_lossy(&bytes), first)
            }
        };
        let wanted = (last - first + 1) as usize;
        let lines = split_lines(&content).into_iter().take(wanted);
        for (number, text) in (first..).zip(lines) {
            let (text, cr) = strip_cr(text);
            shown.crlf |= cr;
            shown.lines.push(Line { line: number, text: text.to_owned() });
        }
        if shown.lines.len() != wanted {
            return Err(Error::failed("the file content"));
        }
        shown.start = Some(first);
        shown.end = Some(last);
    }
    describe(&mut shown, &window, notes);
    if shown.crlf {
        shown.fields.push("CRLF".to_owned());
    }
    shown.fields.extend(rev);
    Ok(shown)
}

/// The header's range field and the notes about a range that was cut.
fn describe(shown: &mut Answer, window: &Window, notes: &mut Notes) {
    let total = shown.total_lines;
    if let (Some(start), Some(end)) = (shown.start, shown.end) {
        let range = if start == end { start.to_string() } else { format!("{start}-{end}") };
        let rest = if window.shown.is_none() && end < total { " (1- for all)" } else { "" };
        shown.fields.push(format!("lines {range} of {total}{rest}"));
        if let Some(asked) = &window.shown
            && let End::Line(asked_end) = asked.end
            && asked_end > end
        {
            notes.push(format!(
                "lines {} clamped to {range}; the file has {total} lines",
                asked.display()
            ));
        }
        return;
    }
    if total == 0 {
        shown.fields.push("lines 0 of 0".to_owned());
    }
    if let Some(asked) = window.shown.as_ref().map(LineRange::display) {
        let message = format!("no lines {asked}; the file has {total}");
        if total > 0 {
            shown.fields.push(message.clone());
        }
        notes.push(message);
    }
}

/// The text of `whole` from line `first` on, as the server's `content`
/// would give it.
fn lines_from(whole: &str, first: u32) -> String {
    let skip = (first - 1) as usize;
    whole.split_inclusive('\n').skip(skip).collect()
}

fn split_lines(content: &str) -> Vec<&str> {
    let mut lines: Vec<&str> = content.split('\n').collect();
    if lines.last() == Some(&"") {
        lines.pop();
    }
    lines
}
