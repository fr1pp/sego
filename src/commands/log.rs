use std::fmt::{self, Write as _};

use serde::Serialize;

use crate::address::{Repo, pinned, range_rev_field, rev_field, short};
use crate::error::Error;
use crate::escape::shell_word;
use crate::escape::{one_line, render_line};
use crate::output::{Notes, count, date, header, header_line};
use crate::sourcegraph::{Client, Log, NEWEST_TAGS};

const ROWS: usize = 150;
/// One more than `ROWS`, so a full page shows whether more exist.
#[expect(clippy::cast_possible_truncation, reason = "the row limit is a few hundred")]
const FIRST: u32 = ROWS as u32 + 1;

#[derive(Serialize)]
struct Entry {
    commit: String,
    date: String,
    author: String,
    subject: String,
    parents: Vec<String>,
    tags: Vec<String>,
}

#[derive(Serialize)]
pub struct Answer {
    repo: String,
    rev: Option<String>,
    commit: Option<String>,
    rev_a: Option<String>,
    rev_b: Option<String>,
    commit_a: Option<String>,
    commit_b: Option<String>,
    path: Option<String>,
    total: Option<u64>,
    more: bool,
    commits: Vec<Entry>,
}

impl Answer {
    fn range(&self) -> Option<(&str, &str)> {
        Some((self.rev_a.as_deref()?, self.rev_b.as_deref()?))
    }

    fn count(&self) -> String {
        let shown = self.commits.len().min(ROWS) as u64;
        match self.total {
            Some(total) if total > ROWS as u64 => {
                format!("{ROWS} of {total} commits shown")
            }
            Some(total) => count(total, "commit", "commits"),
            None if self.more => format!("{ROWS} commits shown; more exist"),
            None => count(shown, "commit", "commits"),
        }
    }
}

impl fmt::Display for Answer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut fields = vec![self.count()];
        let header = if let Some((a, b)) = self.range() {
            fields.extend(range_rev_field(a, "..", b));
            let mut address = format!(
                "{}@{}..{}",
                self.repo,
                short(self.commit_a.as_deref().unwrap_or_default()),
                short(self.commit_b.as_deref().unwrap_or_default())
            );
            if let Some(path) = &self.path {
                address.push(':');
                address.push_str(path);
            }
            header_line(&shell_word(&address), &fields)
        } else {
            fields.extend(rev_field(self.rev.as_ref()));
            let commit = self.commit.as_deref().unwrap_or_default();
            header(&pinned(&self.repo, commit, self.path.as_deref()), &fields)
        };
        f.write_str(&header)?;
        for entry in self.commits.iter().take(ROWS) {
            writeln!(f, "{}", row(entry))?;
        }
        Ok(())
    }
}

fn row(entry: &Entry) -> String {
    let mut line = format!(
        "{}  {}  {}  ",
        one_line(&short(&entry.commit)),
        one_line(&entry.date),
        one_line(&entry.author)
    );
    if !entry.tags.is_empty() {
        let tags: Vec<String> =
            entry.tags.iter().map(|tag| format!("tag: {}", one_line(tag))).collect();
        let _ = write!(line, "({}) ", tags.join(", "));
    }
    line.push_str(&render_line(&entry.subject, None));
    line.truncate(line.trim_end().len());
    line
}

/// The commits `log` lists: a revision's history, from the default branch
/// for `None`, or the commits in a range's head and not in its base.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Revs {
    One(Option<String>),
    Range { base: String, head: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    pub repo: Repo,
    pub revs: Revs,
    pub path: Option<String>,
    pub json: bool,
}

pub fn run(args: &Args, client: &Client, notes: &mut Notes) -> Result<Answer, Error> {
    let path = args.path.as_deref();
    match &args.revs {
        Revs::One(rev) => {
            let history = client.history(&args.repo, rev.as_deref(), path, FIRST)?;
            let (commits, more) = listed(history.log, true, notes);
            Ok(Answer {
                repo: history.repo,
                rev: rev.clone(),
                commit: Some(history.commit),
                rev_a: None,
                rev_b: None,
                commit_a: None,
                commit_b: None,
                path: args.path.clone(),
                total: None,
                more,
                commits,
            })
        }
        Revs::Range { base, head } => {
            let first = (!args.json).then_some(FIRST);
            let history = client.history_range(&args.repo, (base, head), path, first)?;
            let (commits, more) = listed(history.log, !args.json, notes);
            Ok(Answer {
                repo: history.repo,
                rev: None,
                commit: None,
                rev_a: Some(base.clone()),
                rev_b: Some(head.clone()),
                commit_a: Some(history.base),
                commit_b: Some(history.head),
                path: args.path.clone(),
                total: history.ahead,
                more,
                commits,
            })
        }
    }
}

/// The entries a log prints, at most `ROWS` when `cut`, each marked with the
/// tags that name it, and whether more exist.
fn listed(log: Log, cut: bool, notes: &mut Notes) -> (Vec<Entry>, bool) {
    let more = log.more || (cut && log.commits.len() > ROWS);
    let mut entries: Vec<Entry> = log
        .commits
        .into_iter()
        .map(|commit| Entry {
            tags: log
                .tags
                .iter()
                .filter(|(_, oid)| *oid == commit.oid)
                .map(|(name, _)| name.clone())
                .collect(),
            commit: commit.oid,
            date: date(&commit.author_date),
            author: commit.author,
            subject: commit.subject,
            parents: commit.parents,
        })
        .collect();
    if cut {
        entries.truncate(ROWS);
    }
    if log.tags_total > NEWEST_TAGS {
        notes.push(format!("tags marked from the {NEWEST_TAGS} newest of {}", log.tags_total));
    }
    (entries, more)
}
