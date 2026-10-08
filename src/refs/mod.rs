use std::io;
use std::slice;
use std::time::Instant;

use serde::Serialize;
use serde_json::{Value, json};

use crate::address::{Repo, short};
use crate::error::Error;
use crate::escape::one_line;
use crate::output::text::header_line;
use crate::output::{Answer, Notes, date};
use crate::upstream::{
    GRAPHQL_PATH, RepoPlan, Root, Transport, Var, View, build_repository,
    classify, locate_repository, repository_lookup_failed,
};

const TEXT_ROWS: usize = 100;
/// One more than `TEXT_ROWS`, so the totals show whether rows were cut.
const TEXT_FIRST: u32 = 101;
/// The most that two `owner/name` roots allow under the 500,000 field-unit
/// limit: 4 * 17000 * 7 + 20.
const JSON_FIRST: u32 = 17000;

const PARTS: &[(&str, &str)] = &[
    ("branches", "the branch list"),
    ("all_branches", "the branch list"),
    ("tags", "the tag list"),
    ("all_tags", "the tag list"),
];

const NODES: &str = "{ totalCount nodes { displayName target { oid commit { \
                     committer { date } } } } }";

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

    fn key(self) -> &'static str {
        self.plural()
    }

    fn all_key(self) -> &'static str {
        match self {
            Kind::Branch => "all_branches",
            Kind::Tag => "all_tags",
        }
    }

    fn part(self) -> &'static str {
        match self {
            Kind::Branch => "the branch list",
            Kind::Tag => "the tag list",
        }
    }

    fn unit(self, count: u64) -> &'static str {
        if count == 1 { self.word() } else { self.plural() }
    }
}

#[derive(Serialize)]
pub struct Ref {
    kind: Kind,
    name: String,
    commit: Option<String>,
    date: Option<String>,
    #[serde(skip)]
    default: bool,
}

impl Ref {
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
pub struct Refs {
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
    rows: Vec<Ref>,
}

impl Refs {
    fn fields(&self) -> Vec<String> {
        let mut fields = Vec::new();
        for (kind, total, all) in [
            (Kind::Branch, self.branches_total, self.branches_all),
            (Kind::Tag, self.tags_total, self.tags_all),
        ] {
            let capped = total > TEXT_ROWS as u64;
            match (&self.query, all) {
                (Some(query), Some(all)) => {
                    fields.push(format!(
                        "{total} of {all} {} match \"{query}\"",
                        kind.unit(all)
                    ));
                    if capped {
                        fields.push(format!("{TEXT_ROWS} shown"));
                    }
                }
                _ if capped => fields.push(format!(
                    "{TEXT_ROWS} of {total} {} shown",
                    kind.plural()
                )),
                _ => fields.push(format!("{total} {}", kind.unit(total))),
            }
        }
        fields
    }

    fn printed(&self, kind: Kind) -> impl Iterator<Item = &Ref> {
        self.rows.iter().filter(move |r| r.kind == kind).take(TEXT_ROWS)
    }
}

impl Answer for Refs {
    fn write_json(&self, out: &mut dyn io::Write) -> io::Result<()> {
        crate::output::json::write(self, out)
    }

    fn text(&self) -> String {
        let mut out = header_line(&one_line(&self.repo), &self.fields());
        for kind in [Kind::Branch, Kind::Tag] {
            for row in self.printed(kind) {
                out.push_str(&row.row());
            }
        }
        out
    }
}

fn plan(text: Option<&str>, json: bool) -> RepoPlan {
    let mut template = format!(
        "branches(first: $first, query: $query) {NODES}\n\
         tags(first: $first, query: $query) {NODES}\n"
    );
    if text.is_some() {
        template.push_str(
            "all_branches: branches(first: 1) { totalCount }\n\
             all_tags: tags(first: 1) { totalCount }\n",
        );
    }
    let first = if json { JSON_FIRST } else { TEXT_FIRST };
    RepoPlan {
        operation: "Refs",
        vars: vec![
            Var { name: "first", ty: "Int!", value: json!(first) },
            Var { name: "query", ty: "String", value: json!(text) },
        ],
        roots: vec![Root { alias: "r", template }],
        parts: PARTS,
    }
}

pub fn run(
    repo: &Repo,
    text: Option<&str>,
    json: bool,
    transport: &dyn Transport,
    deadline: Instant,
    _notes: &mut Notes,
) -> Result<Refs, Error> {
    let plan = plan(text, json);
    let request = build_repository(&plan, repo);
    let reply = transport.post(GRAPHQL_PATH, &request.body(), deadline)?;
    let envelope = classify(reply)?;
    let found = locate_repository(repo, &envelope, &plan)?;
    let at = found.at("r");
    let view = View::new(&envelope, slice::from_ref(&at));

    for kind in [Kind::Branch, Kind::Tag] {
        if view.errored_beneath(&[kind.key()])
            || view.errored_beneath(&[kind.all_key()])
        {
            return Err(Error::failed(kind.part()));
        }
    }
    if view.errored_beneath(&["defaultBranch"]) {
        return Err(repository_lookup_failed());
    }
    let default_branch = view
        .get(&["defaultBranch", "displayName"])
        .and_then(Value::as_str)
        .map(str::to_owned);

    let branches = listing(&view, Kind::Branch, default_branch.as_deref())?;
    let tags = listing(&view, Kind::Tag, None)?;
    let all = |kind: Kind| -> Result<Option<u64>, Error> {
        if text.is_none() {
            return Ok(None);
        }
        let total = view.get(&[kind.all_key(), "totalCount"]);
        total
            .and_then(Value::as_u64)
            .map(Some)
            .ok_or_else(|| Error::failed(kind.part()))
    };
    let (branches_all, tags_all) = (all(Kind::Branch)?, all(Kind::Tag)?);
    let more = |listing: &Listing| listing.total > listing.rows.len() as u64;
    let (branches_more, tags_more) = (more(&branches), more(&tags));
    let mut rows = branches.rows;
    rows.extend(tags.rows);
    Ok(Refs {
        repo: found.repo,
        query: text.map(str::to_owned),
        default_branch,
        branches_total: branches.total,
        tags_total: tags.total,
        branches_all,
        tags_all,
        branches_more,
        tags_more,
        rows,
    })
}

struct Listing {
    total: u64,
    rows: Vec<Ref>,
}

fn listing(
    view: &View<'_>,
    kind: Kind,
    default: Option<&str>,
) -> Result<Listing, Error> {
    let failed = || Error::failed(kind.part());
    let total = view
        .get(&[kind.key(), "totalCount"])
        .and_then(Value::as_u64)
        .ok_or_else(failed)?;
    let nodes = view
        .get(&[kind.key(), "nodes"])
        .and_then(Value::as_array)
        .ok_or_else(failed)?;
    let mut rows = Vec::with_capacity(nodes.len());
    for node in nodes {
        let name = node
            .get("displayName")
            .and_then(Value::as_str)
            .ok_or_else(failed)?;
        let target = node.get("target").ok_or_else(failed)?;
        let commit = target.get("commit").filter(|commit| !commit.is_null());
        let oid = match commit {
            Some(_) => Some(
                target
                    .get("oid")
                    .and_then(Value::as_str)
                    .ok_or_else(failed)?
                    .to_owned(),
            ),
            None => None,
        };
        let date = commit
            .and_then(|commit| commit.get("committer"))
            .and_then(|committer| committer.get("date"))
            .and_then(Value::as_str)
            .map(date);
        rows.push(Ref {
            kind,
            name: name.to_owned(),
            commit: oid,
            date,
            default: default == Some(name),
        });
    }
    Ok(Listing { total, rows })
}
