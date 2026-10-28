use std::fmt;

use serde::Serialize;

use crate::address::{Repo, range_rev_field, rev_field, short};
use crate::commands::Revs;
use crate::error::Error;
use crate::escape::render_line;
use crate::escape::shell_word;
use crate::output::{
    FileRow, Notes, closing_fields, file_row, files_field, header_line, row_text, rows_asked,
};
use crate::pointer;
use crate::sourcegraph::Client;

const PATCH_INDENT: &str = "    ";

#[derive(Serialize)]
pub struct Answer {
    repo: String,
    rev_a: Option<String>,
    rev_b: String,
    commit_a: String,
    commit_b: String,
    base: Option<String>,
    files_total: Option<u64>,
    files_more: bool,
    added: u64,
    deleted: u64,
    files: Vec<FileRow>,
    paths: Vec<String>,
    patch: Option<String>,
    mirrored_at: Option<String>,
}

impl Answer {
    fn rev_field(&self) -> Option<String> {
        match &self.rev_a {
            Some(a) => range_rev_field((a, &self.commit_a), "...", (&self.rev_b, &self.commit_b)),
            None => rev_field(Some(&self.rev_b), &self.commit_b),
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
            files_field(printed, self.files_total.unwrap_or(self.files.len() as u64)),
            format!("+{} -{}", self.added, self.deleted),
        ];
        if self.paths.len() > 1 {
            let words: Vec<_> = self.paths.iter().map(|path| shell_word(path)).collect();
            fields.push(format!("paths {}", words.join(" ")));
        }
        fields.extend(self.base.as_deref().map(|b| format!("base {}", short(b))));
        fields.extend(closing_fields(self.rev_field(), self.mirrored_at.as_deref()));
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    pub repo: Repo,
    pub revs: Revs,
    pub paths: Vec<String>,
    pub json: bool,
}

pub fn run(args: &Args, client: Client, notes: &mut Notes) -> Result<Answer, Error> {
    let (rev_a, head) = match &args.revs {
        Revs::One(rev) => (None, rev.as_deref().unwrap_or("HEAD")),
        Revs::Range { base, head } => (Some(base.clone()), head.as_str()),
    };
    let first = rows_asked(args.json, TEXT_ROWS);
    let comparison = client.comparison(&args.repo, args.revs.revisions(), &args.paths, first)?;
    let mirrored_at = comparison.lookup().noting_other_host(|note| notes.push(note));
    let root_commit = || {
        Error::not_found(format!("{head} is a root commit: Sourcegraph has no diff for it"))
            .fact(pointer::root_tree(comparison.head.at()))
    };
    let Some(changes) = comparison.changes else {
        return Err(root_commit());
    };
    let commit_a = match (comparison.base, &comparison.from) {
        (Some(base), _) => base.commit,
        (None, Some(parent)) => parent.clone(),
        (None, None) => return Err(root_commit()),
    };
    Ok(Answer {
        repo: comparison.head.repo,
        rev_a,
        rev_b: head.to_owned(),
        commit_a,
        commit_b: comparison.head.commit,
        base: comparison.from,
        files_total: changes.files.total,
        files_more: changes.files.more,
        added: changes.added,
        deleted: changes.deleted,
        files: changes.files.items.into_iter().map(file_row).collect(),
        paths: args.paths.clone(),
        patch: comparison.patch,
        mirrored_at,
    })
}

const TEXT_ROWS: usize = 200;
