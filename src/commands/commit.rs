use std::fmt;

use serde::Serialize;

use crate::address::{Repo, pinned, rev_field, short};
use crate::commands::diff::{FileRow, TEXT_ROWS, file_row, files_field, row_text};
use crate::error::Error;
use crate::escape::{render_line, strip_cr};
use crate::output::header;
use crate::output::{Notes, date};
use crate::sourcegraph::{Client, Commit};

const MESSAGE_INDENT: &str = "    ";
const ROOT_NOTE: &str = "root commit: Sourcegraph has no diff for it";

#[derive(Serialize)]
pub struct Answer {
    repo: String,
    rev: String,
    commit: String,
    author: String,
    author_date: String,
    committer: Option<String>,
    committer_date: Option<String>,
    parents: Vec<String>,
    subject: String,
    message: String,
    files: Vec<FileRow>,
    files_total: Option<u64>,
    added: Option<u64>,
    deleted: Option<u64>,
}

impl Answer {
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
        fields.extend(rev_field(Some(&self.rev)));
        fields
    }
}

impl fmt::Display for Answer {
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
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    pub repo: Repo,
    pub rev: String,
    pub json: bool,
}

pub fn run(args: &Args, client: &Client, notes: &mut Notes) -> Result<Answer, Error> {
    let commit = client.commit(&args.repo, &args.rev)?;
    if commit.changes.is_none() {
        notes.push(ROOT_NOTE);
    }
    Ok(answer(commit, &args.rev))
}

fn answer(commit: Commit, rev: &str) -> Answer {
    let (files, files_total, added, deleted) = match commit.changes {
        Some(changes) => (
            changes.files.into_iter().map(file_row).collect(),
            Some(changes.total),
            Some(changes.added),
            Some(changes.deleted),
        ),
        None => (Vec::new(), None, None, None),
    };
    Answer {
        repo: commit.repo,
        rev: rev.to_owned(),
        commit: commit.oid,
        author: commit.author,
        author_date: date(&commit.author_date),
        committer: commit.committer,
        committer_date: commit.committer_date.as_deref().map(date),
        parents: commit.parents,
        subject: commit.subject,
        message: commit.message.replace("\r\n", "\n"),
        files,
        files_total,
        added,
        deleted,
    }
}
