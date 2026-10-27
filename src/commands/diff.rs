use std::fmt;

use serde::Serialize;

use crate::address::{Repo, range_rev_field, rev_field, short};
use crate::error::Error;
use crate::escape::render_line;
use crate::escape::shell_word;
use crate::output::{count, header_line};
use crate::sourcegraph::{Change, Client};

const PATCH_INDENT: &str = "    ";

#[derive(Serialize)]
pub struct Answer {
    repo: String,
    rev_a: Option<String>,
    rev_b: String,
    commit_a: String,
    commit_b: String,
    base: Option<String>,
    files_total: u64,
    added: u64,
    deleted: u64,
    files: Vec<FileRow>,
    patch: Option<String>,
    #[serde(skip)]
    paths: Vec<String>,
}

impl Answer {
    fn rev_field(&self) -> Option<String> {
        match &self.rev_a {
            Some(a) => range_rev_field(a, "...", &self.rev_b),
            None => rev_field(Some(&self.rev_b)),
        }
    }
}

impl fmt::Display for Answer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut address =
            format!("{}@{}...{}", self.repo, short(&self.commit_a), short(&self.commit_b));
        if let [path] = self.paths.as_slice() {
            address.push(':');
            address.push_str(path);
        }
        let lead = shell_word(&address).into_owned();
        let printed = self.files.len().min(TEXT_ROWS);
        let mut fields = vec![
            files_field(printed, self.files_total),
            format!("+{} -{}", self.added, self.deleted),
        ];
        if self.paths.len() > 1 {
            let words: Vec<_> = self.paths.iter().map(|path| shell_word(path)).collect();
            fields.push(format!("paths {}", words.join(" ")));
        }
        fields.extend(self.base.as_deref().map(|b| format!("base {}", short(b))));
        fields.extend(self.rev_field());
        f.write_str(&header_line(&lead, &fields))?;
        for row in self.files.iter().take(TEXT_ROWS) {
            writeln!(f, "{}", row_text(row))?;
        }
        if let Some(patch) = self.patch.as_deref().filter(|p| !p.is_empty()) {
            writeln!(f)?;
            for line in patch.lines() {
                writeln!(f, "{PATCH_INDENT}{}", render_line(line, None))?;
            }
        }
        Ok(())
    }
}

/// What `diff` compares: a revision with its first parent, or the head of a
/// range with its base.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Revs {
    One(String),
    Range { base: String, head: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    pub repo: Repo,
    pub revs: Revs,
    pub paths: Vec<String>,
    pub json: bool,
}

pub fn run(args: &Args, client: &Client) -> Result<Answer, Error> {
    let (base, head) = match &args.revs {
        Revs::One(rev) => (None, rev),
        Revs::Range { base, head } => (Some(base.as_str()), head),
    };
    let first = (!args.json).then_some(TEXT_FIRST);
    let comparison = client.comparison(&args.repo, base, head, &args.paths, first)?;
    Ok(Answer {
        repo: comparison.repo,
        rev_a: base.map(str::to_owned),
        rev_b: head.clone(),
        commit_a: comparison.base,
        commit_b: comparison.head,
        base: comparison.merge_base,
        files_total: comparison.changes.total,
        added: comparison.changes.added,
        deleted: comparison.changes.deleted,
        files: comparison.changes.files.into_iter().map(file_row).collect(),
        patch: comparison.patch,
        paths: args.paths.clone(),
    })
}

pub const TEXT_ROWS: usize = 200;
/// One more than `TEXT_ROWS`, so the totals show whether rows were cut.
#[expect(clippy::cast_possible_truncation, reason = "the row limit is a few hundred")]
const TEXT_FIRST: u32 = TEXT_ROWS as u32 + 1;

#[derive(Serialize)]
pub struct FileRow {
    /// `A`, `D`, `R` or `M`. A type, mode or submodule change is an `M`.
    pub status: char,
    pub old_path: Option<String>,
    pub new_path: Option<String>,
    pub added: u64,
    pub deleted: u64,
}

fn status(old: Option<&String>, new: Option<&String>) -> char {
    match (old, new) {
        (None, _) => 'A',
        (_, None) => 'D',
        (Some(old), Some(new)) if old != new => 'R',
        _ => 'M',
    }
}

pub fn file_row(change: Change) -> FileRow {
    FileRow {
        status: status(change.old_path.as_ref(), change.new_path.as_ref()),
        old_path: change.old_path,
        new_path: change.new_path,
        added: change.added,
        deleted: change.deleted,
    }
}

/// `M  +2 -2  path`, or `R  +1 -1  old -> new`, without a line break.
pub fn row_text(row: &FileRow) -> String {
    let word = |path: &Option<String>| shell_word(path.as_deref().unwrap_or_default()).into_owned();
    let path = match row.status {
        'R' => format!("{} -> {}", word(&row.old_path), word(&row.new_path)),
        'D' => word(&row.old_path),
        _ => word(&row.new_path),
    };
    format!("{}  +{} -{}  {path}", row.status, row.added, row.deleted)
}

/// `shown` rows are printed out of `total` files.
pub fn files_field(shown: usize, total: u64) -> String {
    if (shown as u64) < total {
        return format!("{shown} of {total} files shown");
    }
    count(total, "file", "files")
}
