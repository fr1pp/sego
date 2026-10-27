use std::fmt;

use serde::Serialize;

use crate::address::{Repo, short};
use crate::error::Error;
use crate::escape::one_line;
use crate::output::{count, date, header_line};
use crate::sourcegraph::{Client, RefList, Refs};

const TEXT_ROWS: usize = 100;
/// One more than `TEXT_ROWS`, so the totals show whether rows were cut.
#[expect(clippy::cast_possible_truncation, reason = "the row limit is a few hundred")]
const TEXT_FIRST: u32 = TEXT_ROWS as u32 + 1;

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
enum Kind {
    Branch,
    Tag,
}

impl Kind {
    fn word(self) -> &'static str {
        match self {
            Kind::Branch => "branch",
            Kind::Tag => "tag",
        }
    }

    fn plural(self) -> &'static str {
        match self {
            Kind::Branch => "branches",
            Kind::Tag => "tags",
        }
    }
}

#[derive(Serialize)]
struct Row {
    kind: Kind,
    name: String,
    commit: Option<String>,
    date: Option<String>,
    #[serde(skip)]
    default: bool,
}

impl Row {
    fn row(&self) -> String {
        let commit = self.commit.as_deref().map(short);
        let mut row = format!(
            "{:<6}  {:<12}  {:<10}  {}",
            self.kind.word(),
            one_line(commit.as_deref().unwrap_or("-")),
            one_line(self.date.as_deref().unwrap_or("-")),
            one_line(&self.name)
        );
        if self.default {
            row.push_str("  (default)");
        }
        row.push('\n');
        row
    }
}

#[derive(Serialize)]
pub struct Answer {
    repo: String,
    query: Option<String>,
    default_branch: Option<String>,
    branches_total: u64,
    tags_total: u64,
    branches_all: Option<u64>,
    tags_all: Option<u64>,
    branches_more: bool,
    tags_more: bool,
    #[serde(rename = "refs")]
    rows: Vec<Row>,
}

impl Answer {
    fn fields(&self) -> Vec<String> {
        let mut fields = Vec::new();
        for (kind, total, all) in [
            (Kind::Branch, self.branches_total, self.branches_all),
            (Kind::Tag, self.tags_total, self.tags_all),
        ] {
            let capped = total > TEXT_ROWS as u64;
            match (&self.query, all) {
                (Some(query), Some(all)) => {
                    let all = count(all, kind.word(), kind.plural());
                    fields.push(format!("{total} of {all} match \"{query}\""));
                    if capped {
                        fields.push(format!("{TEXT_ROWS} shown"));
                    }
                }
                _ if capped => {
                    fields.push(format!("{TEXT_ROWS} of {total} {} shown", kind.plural()));
                }
                _ => fields.push(count(total, kind.word(), kind.plural())),
            }
        }
        fields
    }

    fn printed(&self, kind: Kind) -> impl Iterator<Item = &Row> {
        self.rows.iter().filter(move |r| r.kind == kind).take(TEXT_ROWS)
    }
}

impl fmt::Display for Answer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&header_line(&one_line(&self.repo), &self.fields()))?;
        for kind in [Kind::Branch, Kind::Tag] {
            for row in self.printed(kind) {
                f.write_str(&row.row())?;
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    pub repo: Repo,
    pub text: Option<String>,
    pub json: bool,
}

pub fn run(args: &Args, client: &Client) -> Result<Answer, Error> {
    let first = (!args.json).then_some(TEXT_FIRST);
    let refs = client.refs(&args.repo, args.text.as_deref(), first)?;
    Ok(answer(refs, args.text.clone()))
}

fn answer(refs: Refs, query: Option<String>) -> Answer {
    let default = refs.default_branch.as_deref();
    let rows = |kind: Kind, list: &RefList| -> Vec<Row> {
        list.refs
            .iter()
            .map(|r| Row {
                kind,
                name: r.name.clone(),
                commit: r.commit.clone(),
                date: r.committed.as_deref().map(date),
                default: kind == Kind::Branch && default == Some(r.name.as_str()),
            })
            .collect()
    };
    let more = |list: &RefList| list.total > list.refs.len() as u64;
    let mut all = rows(Kind::Branch, &refs.branches);
    all.extend(rows(Kind::Tag, &refs.tags));
    Answer {
        query,
        default_branch: refs.default_branch.clone(),
        branches_total: refs.branches.total,
        tags_total: refs.tags.total,
        branches_all: refs.branches.unfiltered,
        tags_all: refs.tags.unfiltered,
        branches_more: more(&refs.branches),
        tags_more: more(&refs.tags),
        rows: all,
        repo: refs.repo,
    }
}
