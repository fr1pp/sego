use std::fmt;

use serde::Serialize;

use crate::address::{End, LineRange, Repo, Target, pinned, rev_field, short};
use crate::commands::{AddressKind, ls};
use crate::error::Error;
use crate::escape::{one_line, render_line, strip_cr, utf8_escaped};
use crate::output::{
    FileRow, Marker, Notes, base64, closing_fields, code_line, count, date, file_row, files_field,
    header, row_text, rows_asked, submodule_address, submodule_text,
};
use crate::pointer;
use crate::sourcegraph::{Client, Comparison, Content, File, Resolved, Window};

const DEFAULT_WINDOW: u32 = 500;
const TEXT_ROWS: usize = 200;
const UTF8_NOTE: &str = "U+FFFD may stand for bytes that aren't UTF-8";

#[derive(Serialize)]
pub struct Line {
    #[serde(rename = "line")]
    pub number: u32,
    /// The line, CRs included, with U+FFFD for bytes that aren't UTF-8.
    pub text: String,
    /// The line's bytes in base64, when they aren't UTF-8.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes: Option<String>,
    #[serde(skip)]
    shown: String,
}

/// A submodule, as the answer gives it.
#[derive(Serialize)]
pub struct Submodule {
    url: Option<String>,
    address: Option<String>,
    commit: String,
}

/// What `show` prints: a file or a submodule, or the listing of a directory.
#[derive(Serialize)]
#[serde(untagged)]
pub enum Shown {
    File(Answer),
    Listing(ls::Answer),
}

impl fmt::Display for Shown {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Shown::File(answer) => answer.fmt(f),
            Shown::Listing(answer) => answer.fmt(f),
        }
    }
}

#[derive(Serialize)]
pub struct Answer {
    pub repo: String,
    pub rev: Option<String>,
    pub commit: String,
    pub path: String,
    pub kind: AddressKind,
    pub total_lines: u32,
    pub byte_size: u64,
    pub binary: bool,
    pub crlf: bool,
    /// `None` when Sourcegraph's text held U+FFFD and the raw read failed.
    pub utf8: Option<bool>,
    pub lfs_size: Option<u64>,
    pub submodule: Option<Submodule>,
    pub start: Option<u32>,
    pub end: Option<u32>,
    pub lines: Vec<Line>,
    pub mirrored_at: Option<String>,
    #[serde(skip)]
    fields: Vec<String>,
}

impl fmt::Display for Answer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let address = pinned(&self.repo, &self.commit, Some(&self.path));
        f.write_str(&header(&address, &self.fields))?;
        for line in &self.lines {
            f.write_str(&code_line(line.number, Marker::Match, &line.shown, None))?;
        }
        Ok(())
    }
}

/// What the caller asked for, as the server is asked and as it is reported.
struct Asked {
    window: Window,
    shown: Option<LineRange>,
}

impl Asked {
    fn new(lines: Option<&LineRange>) -> Asked {
        match lines {
            None => Asked { window: Window { start: 1, end: Some(DEFAULT_WINDOW) }, shown: None },
            Some(range) => Asked {
                window: Window {
                    start: range.start,
                    end: match range.end {
                        End::Line(end) => Some(end),
                        End::Open => None,
                    },
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

pub fn run(args: &Args, client: Client, notes: &mut Notes) -> Result<Shown, Error> {
    let asked = Asked::new(args.lines.as_ref());
    let (at, file) = client.file(&args.repo, &args.target, Some(asked.window), false)?;
    let path = at.path.clone().unwrap_or_default();
    let blob = match file {
        File::Blob(blob) => blob,
        File::Directory(_) if asked.shown.is_some() => {
            let name = if path.is_empty() { "./" } else { &path };
            return Err(Error::usage_from_response(format!(
                "{name} is a directory at {}; LINES apply to a file",
                pinned(&at.repo, &at.commit, None)
            )));
        }
        File::Directory(_) => return Ok(Shown::Listing(ls::answer(at, file, false, notes))),
        File::Submodule { submodule, gitmodules } => {
            let url = submodule.url.as_deref().unwrap_or_default();
            let address = submodule_address(url, &submodule.commit, &at.repo);
            let mut shown = Answer::new(at, path, AddressKind::Submodule, notes);
            if let Err(error) = gitmodules {
                notes.push(format!("{}: {error}", ls::GITMODULES_NOTE));
            }
            shown.fields.push(format!("submodule  {}", submodule_text(address.as_deref(), url)));
            shown.finish_fields();
            shown.submodule =
                Some(Submodule { url: submodule.url, address, commit: submodule.commit });
            return Ok(Shown::File(shown));
        }
    };
    let mut shown = Answer::new(at, path, AddressKind::File, notes);
    shown.byte_size = blob.byte_size;
    shown.binary = blob.binary;
    shown.lfs_size = blob.lfs_size;
    if shown.binary {
        shown.fields.push(format!("binary  {} bytes", shown.byte_size));
        shown.finish_fields();
        return Ok(Shown::File(shown));
    }
    shown.total_lines = blob.total_lines;
    if let Some(size) = shown.lfs_size {
        shown.fields.push(format!("LFS pointer  object {size} bytes"));
    }
    if let Some(content) = blob.content {
        shown.read(content, asked.window.start, notes);
    }
    describe(&mut shown, &asked);
    if shown.crlf {
        shown.fields.push("CRLF".to_owned());
    }
    if shown.utf8 == Some(false) {
        shown.fields.push("not UTF-8".to_owned());
    }
    shown.finish_fields();
    Ok(Shown::File(shown))
}

impl Answer {
    /// The answer for `path` at `at`, whose lookup it takes in.
    fn new(at: Resolved, path: String, kind: AddressKind, notes: &mut Notes) -> Answer {
        Answer {
            mirrored_at: at.lookup.noting_other_host(|note| notes.push(note)),
            repo: at.repo,
            rev: at.rev,
            commit: at.commit,
            path,
            kind,
            total_lines: 0,
            byte_size: 0,
            binary: false,
            crlf: false,
            utf8: Some(true),
            lfs_size: None,
            submodule: None,
            start: None,
            end: None,
            lines: Vec::new(),
            fields: Vec::new(),
        }
    }

    /// The fields every answer ends with: the revision and the mirror's time.
    fn finish_fields(&mut self) {
        let rev = rev_field(self.rev.as_ref(), &self.commit);
        self.fields.extend(closing_fields(rev, self.mirrored_at.as_deref()));
    }

    /// Takes the lines of `content`, the first numbered `first`. When the
    /// header says CRLF, the CR of each line's terminator is left out of the
    /// text; every other CR shows.
    fn read(&mut self, content: Content, first: u32, notes: &mut Notes) {
        self.crlf = content.crlf;
        self.utf8 = content.utf8.as_ref().ok().copied();
        if let Err(error) = &content.utf8 {
            notes.push(format!("{UTF8_NOTE}: {error}"));
        }
        if let Some(reason) = &content.cache_unavailable {
            notes.push(format!("cache unavailable: {reason}"));
        }
        let count = content.lines.len();
        for (number, line) in (first..).zip(content.lines) {
            let shown = if content.crlf && line.terminated {
                line.bytes.strip_suffix(b"\r").unwrap_or(&line.bytes)
            } else {
                &line.bytes
            };
            let shown = utf8_escaped(shown).into_owned();
            let bytes = std::str::from_utf8(&line.bytes).is_err().then(|| base64(&line.bytes));
            let text = String::from_utf8_lossy(&line.bytes).into_owned();
            self.lines.push(Line { number, text, bytes, shown });
        }
        if count > 0 {
            self.start = Some(first);
            self.end = Some(first + u32::try_from(count).unwrap_or(u32::MAX) - 1);
        }
    }
}

/// The header's range field.
fn describe(shown: &mut Answer, asked: &Asked) {
    let total = shown.total_lines;
    if let (Some(start), Some(end)) = (shown.start, shown.end) {
        let range = if start == end { start.to_string() } else { format!("{start}-{end}") };
        let rest = if asked.shown.is_none() && end < total { " (1- for all)" } else { "" };
        shown.fields.push(format!("lines {range} of {total}{rest}"));
        return;
    }
    if total == 0 {
        shown.fields.push("lines 0 of 0".to_owned());
    } else if let Some(wanted) = asked.shown.as_ref().map(LineRange::display) {
        shown.fields.push(format!("no lines {wanted}; the file has {total}"));
    }
}

const MESSAGE_INDENT: &str = "    ";
const ROOT_NOTE: &str = "root commit: Sourcegraph has no diff for it";
const TAGS_NOTE: &str = "tags not listed";
const TAG_ROWS: usize = 100;

#[derive(Serialize)]
pub struct CommitAnswer {
    repo: String,
    rev: String,
    commit: String,
    kind: AddressKind,
    author: String,
    author_date: String,
    committer: Option<String>,
    committer_date: Option<String>,
    parents: Vec<String>,
    subject: String,
    message: String,
    files: Vec<FileRow>,
    files_total: Option<u64>,
    files_more: bool,
    added: Option<u64>,
    deleted: Option<u64>,
    /// The tags that hold the commit, newest created first; `None` when they
    /// couldn't be read.
    tags: Option<Vec<String>>,
    tags_total: Option<u64>,
    mirrored_at: Option<String>,
}

impl CommitAnswer {
    fn fields(&self) -> Vec<String> {
        let mut fields = Vec::new();
        fields.push(self.author_date.clone());
        fields.push(self.author.clone());
        fields.push(if self.parents.is_empty() {
            "parents none".to_owned()
        } else {
            let shorts: Vec<String> = self.parents.iter().map(|oid| short(oid)).collect();
            format!("parents {}", shorts.join(" "))
        });
        if let (Some(total), Some(added), Some(deleted)) =
            (self.files_total, self.added, self.deleted)
        {
            let printed = self.files.len().min(TEXT_ROWS);
            fields.push(files_field(printed, total));
            fields.push(format!("+{added} -{deleted}"));
        }
        if self.parents.len() > 1 {
            fields.push("diff against first parent".to_owned());
        }
        if let Some(total) = self.tags_total {
            fields.push(match total {
                0 => "no tags".to_owned(),
                total if total > TAG_ROWS as u64 => format!("{TAG_ROWS} of {total} tags shown"),
                total => count(total, "tag", "tags"),
            });
        }
        let rev = rev_field(Some(&self.rev), &self.commit);
        fields.extend(closing_fields(rev, self.mirrored_at.as_deref()));
        fields
    }
}

impl fmt::Display for CommitAnswer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let address = pinned(&self.repo, &self.commit, None);
        f.write_str(&header(&address, &self.fields()))?;
        let mut lines: Vec<&str> = self.message.split('\n').collect();
        if lines.last() == Some(&"") {
            lines.pop();
        }
        for line in lines {
            let (line, _) = strip_cr(line);
            if line.trim_matches([' ', '\t']).is_empty() {
                writeln!(f)?;
            } else {
                writeln!(f, "{MESSAGE_INDENT}{}", render_line(line, None))?;
            }
        }
        if !self.files.is_empty() {
            writeln!(f)?;
            for row in self.files.iter().take(TEXT_ROWS) {
                writeln!(f, "{}", row_text(row))?;
            }
        }
        if let Some(tags) = self.tags.as_ref().filter(|tags| !tags.is_empty()) {
            writeln!(f)?;
            for tag in tags.iter().take(TAG_ROWS) {
                writeln!(f, "tag  {}", one_line(tag))?;
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitArgs {
    pub repo: Repo,
    pub rev: String,
    pub json: bool,
}

/// `show REPO@REV`: the commit, its message, the files it changed and the
/// tags that hold it.
pub fn run_commit(
    args: &CommitArgs,
    client: Client,
    notes: &mut Notes,
) -> Result<CommitAnswer, Error> {
    let first = rows_asked(args.json, TEXT_ROWS);
    let comparison = client.commit(&args.repo, &args.rev, first)?;
    Ok(commit_answer(comparison, &args.rev, notes))
}

fn commit_answer(comparison: Comparison, rev: &str, notes: &mut Notes) -> CommitAnswer {
    let mirrored_at = comparison.lookup().noting_other_host(|note| notes.push(note));
    if comparison.changes.is_none() {
        let tree = pointer::root_tree(comparison.head.at());
        notes.push(format!("{ROOT_NOTE}; {tree}"));
    }
    let (files, files_total, files_more, added, deleted) = match comparison.changes {
        Some(changes) => (
            changes.files.items.into_iter().map(file_row).collect(),
            changes.files.total,
            changes.files.more,
            Some(changes.added),
            Some(changes.deleted),
        ),
        None => (Vec::new(), None, false, None, None),
    };
    let commit = comparison.commit.unwrap_or_default();
    let tags = match commit.tags {
        Some(Ok(tags)) => Some(tags),
        Some(Err(error)) => {
            notes.push(format!("{TAGS_NOTE}: {error}"));
            None
        }
        None => None,
    };
    CommitAnswer {
        repo: comparison.head.repo,
        rev: rev.to_owned(),
        commit: comparison.head.commit,
        kind: AddressKind::Commit,
        author: commit.author,
        author_date: date(&commit.author_date),
        committer: commit.committer,
        committer_date: commit.committer_date.as_deref().map(date),
        parents: commit.parents,
        subject: commit.subject,
        message: commit.message.replace("\r\n", "\n"),
        files,
        files_total,
        files_more,
        added,
        deleted,
        tags_total: tags.as_ref().map(|tags| tags.len() as u64),
        tags,
        mirrored_at,
    }
}
