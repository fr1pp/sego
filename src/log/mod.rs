use std::fmt::Write as _;
use std::io;
use std::slice;
use std::time::Instant;

use serde::Serialize;
use serde_json::{Value, json};

use crate::address::{
    Address, Dots, Rev, is_plain_path, pinned, range_rev_field, rev_field,
    short,
};
use crate::cli::LOG_SYNOPSIS;
use crate::error::Error;
use crate::escape::{one_line, render_line};
use crate::output::text::{header, header_line};
use crate::output::{Answer, Notes, date};
use crate::quote::shell_word;
use crate::upstream::{
    GRAPHQL_PATH, RepoPlan, Root, Transport, Var, View, build_repository,
    classify, locate_repository, missing_revisions, missing_sides, no_commits,
    revision_lookup_failed, text_at,
};

const ROWS: usize = 150;
const TAG_LIMIT: u64 = 1000;

const PAGE: &str = "nodes { oid subject author { date person { displayName } \
                    } parents { oid } } pageInfo { hasNextPage }";

const TAGS: &str = "gitRefs(type: GIT_TAG, first: 1000) { totalCount nodes \
                    { displayName target { oid } } }";

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
pub struct Logged {
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

fn count_field(n: u64) -> String {
    if n == 1 { "1 commit".to_owned() } else { format!("{n} commits") }
}

impl Logged {
    fn range(&self) -> Option<(&str, &str)> {
        Some((self.rev_a.as_deref()?, self.rev_b.as_deref()?))
    }

    fn count(&self) -> String {
        let shown = self.commits.len().min(ROWS) as u64;
        match self.total {
            Some(total) if total > ROWS as u64 => {
                format!("{ROWS} of {total} commits shown")
            }
            Some(total) => count_field(total),
            None if self.more => format!("{ROWS} commits shown; more exist"),
            None => count_field(shown),
        }
    }
}

impl Answer for Logged {
    fn write_json(&self, out: &mut dyn io::Write) -> io::Result<()> {
        crate::output::json::write(self, out)
    }

    fn text(&self) -> String {
        let mut fields = vec![self.count()];
        let mut out = if let Some((a, b)) = self.range() {
            fields.extend(range_rev_field(a, b, Dots::Two));
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
        for entry in self.commits.iter().take(ROWS) {
            out.push_str(&row(entry));
            out.push('\n');
        }
        out
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
        let tags: Vec<String> = entry
            .tags
            .iter()
            .map(|tag| format!("tag: {}", one_line(tag)))
            .collect();
        let _ = write!(line, "({}) ", tags.join(", "));
    }
    line.push_str(&render_line(&entry.subject, None));
    line.truncate(line.trim_end().len());
    line
}

fn revision_plan(rev: &str, path: Option<&str>) -> RepoPlan {
    RepoPlan {
        operation: "Log",
        vars: vec![
            Var { name: "rev", ty: "String!", value: json!(rev) },
            Var { name: "path", ty: "String", value: json!(path) },
        ],
        roots: vec![
            Root {
                alias: "r",
                template: format!(
                    "commit(rev: $rev) {{ oid ancestors(first: {}, path: \
                     $path) {{ {PAGE} }} }}",
                    ROWS + 1
                ),
            },
            Root { alias: "t", template: TAGS.to_owned() },
        ],
        parts: &[],
    }
}

fn range_plan(a: &str, b: &str, path: Option<&str>, json: bool) -> RepoPlan {
    let probe = if path.is_some_and(is_plain_path) {
        " x: path(path: $path) { __typename }"
    } else {
        ""
    };
    let first =
        if json { String::new() } else { format!("first: {}, ", ROWS + 1) };
    let mut roots = vec![
        Root {
            alias: "a",
            template: format!("commit(rev: $a) {{ oid{probe} }}"),
        },
        Root {
            alias: "b",
            template: format!("commit(rev: $b) {{ oid{probe} }}"),
        },
    ];
    if path.is_none() {
        roots.push(Root {
            alias: "n",
            template: "commit(rev: $b) { behindAhead(revspec: $a) { ahead } }"
                .to_owned(),
        });
    }
    roots.push(Root {
        alias: "c",
        template: format!(
            "comparison(base: $a, head: $b) {{ commits({first}path: $path) \
             {{ {PAGE} }} }}"
        ),
    });
    roots.push(Root { alias: "t", template: TAGS.to_owned() });
    RepoPlan {
        operation: "Log",
        vars: vec![
            Var { name: "a", ty: "String!", value: json!(a) },
            Var { name: "b", ty: "String!", value: json!(b) },
            Var { name: "path", ty: "String", value: json!(path) },
        ],
        roots,
        parts: &[],
    }
}

/// `None` when a node lacks a field the rows need.
fn entry(node: &Value) -> Option<Entry> {
    let parents = node
        .get("parents")?
        .as_array()?
        .iter()
        .map(|parent| text_at(parent, &["oid"]).map(str::to_owned))
        .collect::<Option<Vec<_>>>()?;
    Some(Entry {
        commit: text_at(node, &["oid"])?.to_owned(),
        date: date(text_at(node, &["author", "date"])?),
        author: text_at(node, &["author", "person", "displayName"])?
            .to_owned(),
        subject: text_at(node, &["subject"])?.to_owned(),
        parents,
        tags: Vec::new(),
    })
}

/// The entries of a page, with the page's `hasNextPage`.
fn page(page: Option<&Value>) -> Option<(Vec<Entry>, bool)> {
    let page = page?;
    let more = page.get("pageInfo")?.get("hasNextPage")?.as_bool()?;
    let entries = page
        .get("nodes")?
        .as_array()?
        .iter()
        .map(entry)
        .collect::<Option<Vec<_>>>()?;
    Some((entries, more))
}

/// Marks each entry with the tags that point at it, in server order.
fn mark(
    entries: &mut [Entry],
    t: &View<'_>,
    notes: &mut Notes,
) -> Result<(), Error> {
    let tag_list_failed = || Error::failed("the tag list");
    if t.errored_beneath(&[]) {
        return Err(tag_list_failed());
    }
    let refs = t.get(&["gitRefs"]).ok_or_else(tag_list_failed)?;
    let total = refs
        .get("totalCount")
        .and_then(Value::as_u64)
        .ok_or_else(tag_list_failed)?;
    let named = refs
        .get("nodes")
        .and_then(Value::as_array)
        .ok_or_else(tag_list_failed)?;
    let tags: Vec<(&str, &str)> = named
        .iter()
        .filter_map(|node| {
            Some((
                text_at(node, &["displayName"])?,
                text_at(node, &["target", "oid"])?,
            ))
        })
        .collect();
    for entry in entries {
        entry.tags = tags
            .iter()
            .filter(|(_, oid)| *oid == entry.commit)
            .map(|(name, _)| (*name).to_owned())
            .collect();
    }
    if total > TAG_LIMIT {
        notes.push(format!(
            "tags marked from the {TAG_LIMIT} newest of {total}"
        ));
    }
    Ok(())
}

pub fn run(
    address: &Address,
    path: Option<&str>,
    json: bool,
    transport: &dyn Transport,
    deadline: Instant,
    notes: &mut Notes,
) -> Result<Logged, Error> {
    match &address.rev {
        Rev::Default => {
            revision(address, None, path, transport, deadline, notes)
        }
        Rev::One(rev) => {
            revision(address, Some(rev), path, transport, deadline, notes)
        }
        Rev::Range { base, head, .. } => range(
            (address, base, head),
            path,
            json,
            transport,
            deadline,
            notes,
        ),
        Rev::Split(_) => Err(Error::usage(
            "log needs a revision or a range, like REPO@REV or REPO@A..B",
            LOG_SYNOPSIS,
        )),
    }
}

fn revision(
    address: &Address,
    rev: Option<&String>,
    path: Option<&str>,
    transport: &dyn Transport,
    deadline: Instant,
    notes: &mut Notes,
) -> Result<Logged, Error> {
    let sent = rev.map_or("HEAD", String::as_str);
    let plan = revision_plan(sent, path);
    let request = build_repository(&plan, &address.repo);
    let reply = transport.post(GRAPHQL_PATH, &request.body(), deadline)?;
    let envelope = classify(reply)?;
    let found = locate_repository(&address.repo, &envelope, &plan)?;
    let repo = &found.repo;
    let (at_r, at_t) = (found.at("r"), found.at("t"));
    let r = View::new(&envelope, slice::from_ref(&at_r));
    let t = View::new(&envelope, slice::from_ref(&at_t));
    let list_failed = || Error::failed("the commit list");

    if r.errored_beneath(&["commit", "ancestors"]) {
        return Err(list_failed());
    }
    if r.errored_beneath(&["commit"]) {
        return Err(revision_lookup_failed());
    }
    let Some(commit) = r.get(&["commit"]) else {
        return Err(no_commits(repo, &r)
            .unwrap_or_else(|| missing_revisions(repo, &[("", sent)])));
    };
    let oid = text_at(commit, &["oid"]).ok_or_else(revision_lookup_failed)?;
    let (mut commits, has_next) =
        page(commit.get("ancestors")).ok_or_else(list_failed)?;
    if let Some(path) = path.filter(|p| is_plain_path(p))
        && commits.is_empty()
    {
        return Err(Error::missing(format!(
            "{path} not found in {repo}@{}",
            short(oid)
        )));
    }
    let more = has_next || commits.len() > ROWS;
    commits.truncate(ROWS);
    mark(&mut commits, &t, notes)?;
    Ok(Logged {
        repo: repo.clone(),
        rev: rev.cloned(),
        commit: Some(oid.to_owned()),
        rev_a: None,
        rev_b: None,
        commit_a: None,
        commit_b: None,
        path: path.map(str::to_owned),
        total: None,
        more,
        commits,
    })
}

fn range(
    (address, rev_a, rev_b): (&Address, &str, &str),
    path: Option<&str>,
    json: bool,
    transport: &dyn Transport,
    deadline: Instant,
    notes: &mut Notes,
) -> Result<Logged, Error> {
    let plan = range_plan(rev_a, rev_b, path, json);
    let request = build_repository(&plan, &address.repo);
    let reply = transport.post(GRAPHQL_PATH, &request.body(), deadline)?;
    let envelope = classify(reply)?;
    let found = locate_repository(&address.repo, &envelope, &plan)?;
    let repo = &found.repo;
    let (at_a, at_b) = (found.at("a"), found.at("b"));
    let (at_n, at_c, at_t) = (found.at("n"), found.at("c"), found.at("t"));
    let base = View::new(&envelope, slice::from_ref(&at_a));
    let head = View::new(&envelope, slice::from_ref(&at_b));
    let count = View::new(&envelope, slice::from_ref(&at_n));
    let compared = View::new(&envelope, slice::from_ref(&at_c));
    let tagged = View::new(&envelope, slice::from_ref(&at_t));
    let comparison_failed = || Error::failed("the comparison");

    if base.errored_beneath(&["commit"]) || head.errored_beneath(&["commit"]) {
        return Err(revision_lookup_failed());
    }
    let oid = |side: &View<'_>| -> Result<Option<String>, Error> {
        side.get(&["commit"])
            .map(|commit| {
                text_at(commit, &["oid"])
                    .map(str::to_owned)
                    .ok_or_else(revision_lookup_failed)
            })
            .transpose()
    };
    let (commit_a, commit_b) = (oid(&base)?, oid(&head)?);
    if commit_a.is_none()
        && commit_b.is_none()
        && let Some(error) = no_commits(repo, &base)
    {
        return Err(error);
    }
    let (Some(commit_a), Some(commit_b)) = (commit_a, commit_b) else {
        return Err(missing_sides(repo, (rev_a, &base), (rev_b, &head)));
    };

    let commit_list = (!compared.errored_beneath(&[]))
        .then(|| compared.get(&["comparison", "commits"]))
        .flatten();
    if let Some(path) = path.filter(|p| is_plain_path(p))
        && base.get(&["commit", "x"]).is_none()
        && head.get(&["commit", "x"]).is_none()
        && commit_list
            .and_then(|l| l.get("nodes")?.as_array())
            .is_some_and(Vec::is_empty)
    {
        return Err(Error::missing(format!(
            "{path} not found in {repo}@{} or @{}",
            short(&commit_a),
            short(&commit_b)
        )));
    }

    let (mut commits, has_next) =
        page(commit_list).ok_or_else(comparison_failed)?;
    let total = if path.is_none() {
        if count.errored_beneath(&[]) {
            return Err(Error::failed("the commit count"));
        }
        let ahead = count
            .get(&["commit", "behindAhead", "ahead"])
            .and_then(Value::as_u64);
        Some(ahead.ok_or_else(|| Error::failed("the commit count"))?)
    } else {
        None
    };
    let more = has_next || (!json && commits.len() > ROWS);
    if !json {
        commits.truncate(ROWS);
    }
    mark(&mut commits, &tagged, notes)?;
    Ok(Logged {
        repo: repo.clone(),
        rev: None,
        commit: None,
        rev_a: Some(rev_a.to_owned()),
        rev_b: Some(rev_b.to_owned()),
        commit_a: Some(commit_a),
        commit_b: Some(commit_b),
        path: path.map(str::to_owned),
        total,
        more,
        commits,
    })
}
