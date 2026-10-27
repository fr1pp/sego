use std::fmt::{self, Write as _};

use serde::Serialize;

use crate::address::{Repo, pinned, range_rev_field, rev_field, short};
use crate::commands::Revs;
use crate::error::Error;
use crate::escape::shell_word;
use crate::escape::{one_line, render_line};
use crate::output::{Notes, closing_fields, count, date, header, header_line};
use crate::sourcegraph::{Client, CommitSummary, History, Tag};

const ROWS: usize = 150;
#[expect(clippy::cast_possible_truncation, reason = "the row limit is a few hundred")]
const FIRST: u32 = ROWS as u32;
const TAGS_NOTE: &str = "tags not marked";

#[derive(Serialize)]
struct Entry {
    commit: String,
    date: String,
    author: String,
    subject: String,
    parents: Vec<String>,
    /// `None` when the tag list couldn't be read.
    tags: Option<Vec<String>>,
}

/// What a log lists the commits of: a revision, or a range's two ends.
#[derive(Default, Serialize)]
struct At {
    rev: Option<String>,
    commit: Option<String>,
    rev_a: Option<String>,
    rev_b: Option<String>,
    commit_a: Option<String>,
    commit_b: Option<String>,
}

#[derive(Serialize)]
pub struct Answer {
    repo: String,
    #[serde(flatten)]
    at: At,
    path: Option<String>,
    follow: bool,
    behind: Option<u64>,
    commits_total: Option<u64>,
    commits_more: bool,
    commits: Vec<Entry>,
    mirrored_at: Option<String>,
}

impl Answer {
    fn range(&self) -> Option<(&str, &str)> {
        Some((self.at.rev_a.as_deref()?, self.at.rev_b.as_deref()?))
    }

    fn count(&self) -> String {
        let shown = self.commits.len().min(ROWS) as u64;
        match self.commits_total {
            Some(total) if total > ROWS as u64 => {
                format!("{ROWS} of {total} commits shown")
            }
            Some(total) => count(total, "commit", "commits"),
            None if self.commits_more => format!("{shown} commits shown; more exist"),
            None => count(shown, "commit", "commits"),
        }
    }

    /// The fields after the count, before the revision.
    fn facts(&self) -> Vec<String> {
        let mut fields = Vec::new();
        fields.extend(self.behind.filter(|&n| n > 0).map(|n| format!("behind {n}")));
        if self.follow {
            fields.push("follow".to_owned());
        }
        fields
    }
}

impl fmt::Display for Answer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut fields = vec![self.count()];
        fields.extend(self.facts());
        let header = if let Some((a, b)) = self.range() {
            let commit_a = self.at.commit_a.as_deref().unwrap_or_default();
            let commit_b = self.at.commit_b.as_deref().unwrap_or_default();
            let rev = range_rev_field((a, commit_a), "..", (b, commit_b));
            fields.extend(closing_fields(rev, self.mirrored_at.as_deref()));
            let mut address = format!(
                "{}@{}..{}",
                self.repo,
                short(self.at.commit_a.as_deref().unwrap_or_default()),
                short(self.at.commit_b.as_deref().unwrap_or_default())
            );
            if let Some(path) = &self.path {
                address.push(':');
                address.push_str(path);
            }
            header_line(&shell_word(&address), &fields)
        } else {
            let commit = self.at.commit.as_deref().unwrap_or_default();
            let rev = rev_field(self.at.rev.as_ref(), commit);
            fields.extend(closing_fields(rev, self.mirrored_at.as_deref()));
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
    if let Some(tags) = entry.tags.as_ref().filter(|tags| !tags.is_empty()) {
        let tags: Vec<String> = tags.iter().map(|tag| format!("tag: {}", one_line(tag))).collect();
        let _ = write!(line, "({}) ", tags.join(", "));
    }
    line.push_str(&render_line(&entry.subject, None));
    line.truncate(line.trim_end().len());
    line
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    pub repo: Repo,
    pub revs: Revs,
    pub path: Option<String>,
    /// Whether the path is followed across renames, as `git log --follow`.
    pub follow: bool,
    pub json: bool,
}

pub fn run(args: &Args, client: Client, notes: &mut Notes) -> Result<Answer, Error> {
    let path = args.path.as_deref();
    let range = matches!(args.revs, Revs::Range { .. });
    let first = (!range || !args.json).then_some(FIRST);
    let history = client.history(&args.repo, args.revs.revisions(), path, args.follow, first)?;
    Ok(answer(history, args, notes))
}

/// The answer for `history`, its entries marked with the tags that name
/// them.
fn answer(history: History, args: &Args, notes: &mut Notes) -> Answer {
    let mirrored_at = history.lookup().noting_other_host(|note| notes.push(note));
    let tags = match history.tags {
        Ok(tags) => Some(tags),
        Err(error) => {
            notes.push(format!("{TAGS_NOTE}: {error}"));
            None
        }
    };
    let entry = |commit: CommitSummary| Entry {
        tags: tags.as_ref().map(|tags: &Vec<Tag>| {
            tags.iter().filter(|tag| tag.commit == commit.oid).map(|tag| tag.name.clone()).collect()
        }),
        commit: commit.oid,
        date: date(&commit.author_date),
        author: commit.author,
        subject: commit.subject,
        parents: commit.parents,
    };
    let commits = history.commits.items.into_iter().map(entry).collect();
    let at = match (&args.revs, history.base) {
        (Revs::Range { base, head }, Some(base_end)) => At {
            rev_a: Some(base.clone()),
            rev_b: Some(head.clone()),
            commit_a: Some(base_end.commit),
            commit_b: Some(history.head.commit),
            ..At::default()
        },
        (Revs::One(rev), _) => {
            At { rev: rev.clone(), commit: Some(history.head.commit), ..At::default() }
        }
        (Revs::Range { head, .. }, None) => {
            At { rev: Some(head.clone()), commit: Some(history.head.commit), ..At::default() }
        }
    };
    Answer {
        repo: history.head.repo,
        at,
        path: args.path.clone(),
        follow: args.follow,
        behind: history.behind,
        commits_total: history.commits.total,
        commits_more: history.commits.more,
        commits,
        mirrored_at,
    }
}
