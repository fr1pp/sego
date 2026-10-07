use std::fmt::Write as _;
use std::io;
use std::slice;
use std::time::Instant;

use serde::Serialize;
use serde_json::{Value, json};

use crate::address::{
    Address, Rev, is_oid_like, is_plain_path, rev_field, short,
};
use crate::cli::DIFF_SYNOPSIS;
use crate::error::Error;
use crate::escape::render_line;
use crate::output::text::header_line;
use crate::output::{Answer, Notes};
use crate::quote::shell_word;
use crate::upstream::{
    GRAPHQL_PATH, RepoPlan, Root, Transport, Var, View, build_repository,
    classify, locate_repository, missing_revisions,
};

pub mod files;

use files::{FileRow, TEXT_ROWS, files_field, row_text, rows};

const PATCH_INDENT: &str = "    ";

const PAGE_NODES: &str = "{ nodes { oldPath newPath stat { added deleted } \
                          oldFile { ... on GitBlob { commit { oid } } } } }";

#[derive(Serialize)]
pub struct Diffed {
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
}

impl Diffed {
    fn rev_field(&self) -> Option<String> {
        match &self.rev_a {
            Some(a) => (!is_oid_like(a) || !is_oid_like(&self.rev_b))
                .then(|| format!("rev {a}...{}", self.rev_b)),
            None => rev_field(Some(&self.rev_b)),
        }
    }
}

impl Answer for Diffed {
    fn write_json(&self, out: &mut dyn io::Write) -> io::Result<()> {
        crate::output::json::write(self, out)
    }

    fn text(&self) -> String {
        let lead = shell_word(&format!(
            "{}@{}...{}",
            self.repo,
            short(&self.commit_a),
            short(&self.commit_b)
        ))
        .into_owned();
        let printed = self.files.len().min(TEXT_ROWS);
        let mut fields = vec![
            files_field(printed, self.files_total),
            format!("+{} -{}", self.added, self.deleted),
        ];
        fields.extend(
            self.base.as_deref().map(|b| format!("base {}", short(b))),
        );
        fields.extend(self.rev_field());
        let mut out = header_line(&lead, &fields);
        for row in self.files.iter().take(TEXT_ROWS) {
            out.push_str(&row_text(row));
            out.push('\n');
        }
        if let Some(patch) = self.patch.as_deref().filter(|p| !p.is_empty()) {
            out.push('\n');
            for line in patch.lines() {
                out.push_str(PATCH_INDENT);
                out.push_str(&render_line(line, None));
                out.push('\n');
            }
        }
        out
    }
}

/// `Var` names are `'static`. A process runs one command, so the leaked
/// strings are bounded by the operands.
fn numbered(prefix: &str, n: usize) -> &'static str {
    Box::leak(format!("{prefix}{n}").into_boxed_str())
}

fn plan(base: &str, head: &str, paths: &[String], json: bool) -> RepoPlan {
    let plain: Vec<&String> =
        paths.iter().filter(|p| is_plain_path(p)).collect();
    let mut probes = String::new();
    for n in 0..plain.len() {
        let _ = write!(probes, " p{n}: path(path: $path{n}) {{ __typename }}");
    }
    let first = if json {
        String::new()
    } else {
        format!("first: {}, ", TEXT_ROWS + 1)
    };
    let patch = if paths.is_empty() {
        ""
    } else {
        "patch: fileDiffs(paths: $paths) { rawDiff }\n"
    };
    let comparison = format!(
        "comparison(base: $a, head: $b) {{\n\
         all: fileDiffs(first: 0, paths: $paths) {{ totalCount diffStat \
         {{ added deleted }} }}\n\
         page: fileDiffs({first}paths: $paths) {PAGE_NODES}\n\
         {patch}}}\n"
    );

    let mut vars = vec![
        Var { name: "a", ty: "String!", value: json!(base) },
        Var { name: "b", ty: "String!", value: json!(head) },
        Var {
            name: "paths",
            ty: "[String!]",
            value: if paths.is_empty() { Value::Null } else { json!(paths) },
        },
    ];
    for (n, path) in plain.iter().enumerate() {
        vars.push(Var {
            name: numbered("path", n),
            ty: "String!",
            value: json!(path),
        });
    }
    RepoPlan {
        operation: "Diff",
        vars,
        roots: vec![
            Root {
                alias: "a",
                template: format!("commit(rev: $a) {{ oid{probes} }}"),
            },
            Root {
                alias: "b",
                template: format!(
                    "commit(rev: $b) {{ oid parents {{ oid }}{probes} }}"
                ),
            },
            Root { alias: "c", template: comparison },
        ],
        parts: &[],
    }
}

fn lookup_failed() -> Error {
    Error::failed("the revision lookup")
}

fn comparison_failed() -> Error {
    Error::failed("the comparison")
}

/// `None` when the revision names no commit.
fn commit_oid(view: &View<'_>) -> Result<Option<String>, Error> {
    let Some(commit) = view.get(&["commit"]) else {
        return Ok(None);
    };
    commit
        .get("oid")
        .and_then(Value::as_str)
        .map(|oid| Some(oid.to_owned()))
        .ok_or_else(lookup_failed)
}

fn is_root_commit(b: &View<'_>) -> bool {
    b.get(&["commit", "parents"])
        .and_then(Value::as_array)
        .is_some_and(Vec::is_empty)
}

/// The first file that existed before names the commit the comparison
/// started from; a file added in the range has no earlier version.
fn merge_base(nodes: &[Value]) -> Option<String> {
    nodes.iter().find_map(|node| {
        node.get("oldFile")?
            .get("commit")?
            .get("oid")?
            .as_str()
            .map(str::to_owned)
    })
}

pub fn run(
    address: &Address,
    paths: &[String],
    json: bool,
    transport: &dyn Transport,
    deadline: Instant,
    _notes: &mut Notes,
) -> Result<Diffed, Error> {
    let (rev_a, rev_b, base) = match &address.rev {
        Rev::One(rev) => (None, rev.clone(), format!("{rev}^1")),
        Rev::Range { base, head, .. } => {
            (Some(base.clone()), head.clone(), base.clone())
        }
        Rev::Default | Rev::Split(_) => {
            return Err(Error::usage(
                "diff needs a revision or a range, like REPO@REV or \
                 REPO@A...B",
                DIFF_SYNOPSIS,
            ));
        }
    };
    let plan = plan(&base, &rev_b, paths, json);
    let request = build_repository(&plan, &address.repo);
    let reply = transport.post(GRAPHQL_PATH, &request.body(), deadline)?;
    let envelope = classify(reply)?;
    let found = locate_repository(&address.repo, &envelope, &plan)?;
    let repo = &found.repo;
    let (at_a, at_b, at_c) = (found.at("a"), found.at("b"), found.at("c"));
    let a = View::new(&envelope, slice::from_ref(&at_a));
    let b = View::new(&envelope, slice::from_ref(&at_b));
    let c = View::new(&envelope, slice::from_ref(&at_c));

    if a.errored_beneath(&["commit"]) || b.errored_beneath(&["commit"]) {
        return Err(lookup_failed());
    }
    let (commit_a, commit_b) = (commit_oid(&a)?, commit_oid(&b)?);
    if commit_a.is_none()
        && commit_b.is_none()
        && a.get(&["defaultBranch"]).is_none()
    {
        if a.errored_beneath(&["defaultBranch"]) {
            return Err(Error::failed("the repository lookup"));
        }
        return Err(Error::missing(format!("{repo} has no commits")));
    }
    if rev_a.is_none()
        && commit_a.is_none()
        && commit_b.is_some()
        && is_root_commit(&b)
    {
        return Err(Error::missing(format!(
            "{rev_b} is a root commit: Sourcegraph has no diff for it"
        )));
    }
    let (Some(commit_a), Some(commit_b)) =
        (commit_a.as_deref(), commit_b.as_deref())
    else {
        return Err(missing_sides(repo, rev_a.as_deref(), &rev_b, &a, &b));
    };

    for (n, path) in paths.iter().filter(|p| is_plain_path(p)).enumerate() {
        let probe = format!("p{n}");
        let keys = ["commit", probe.as_str()];
        if a.get(&keys).is_none() && b.get(&keys).is_none() {
            return Err(Error::missing(format!(
                "{path} not found in {repo}@{} or @{}",
                short(commit_a),
                short(commit_b)
            )));
        }
    }

    if c.errored_beneath(&[]) {
        return Err(comparison_failed());
    }
    let count = |keys: &[&str]| {
        c.get(keys).and_then(Value::as_u64).ok_or_else(comparison_failed)
    };
    let files_total = count(&["comparison", "all", "totalCount"])?;
    let added = count(&["comparison", "all", "diffStat", "added"])?;
    let deleted = count(&["comparison", "all", "diffStat", "deleted"])?;
    let nodes = c
        .get(&["comparison", "page", "nodes"])
        .and_then(Value::as_array)
        .ok_or_else(comparison_failed)?;
    let patch = if paths.is_empty() {
        None
    } else {
        let raw = c
            .get(&["comparison", "patch", "rawDiff"])
            .and_then(Value::as_str)
            .ok_or_else(comparison_failed)?;
        Some(raw.to_owned())
    };

    Ok(Diffed {
        repo: found.repo.clone(),
        rev_a,
        rev_b,
        commit_a: commit_a.to_owned(),
        commit_b: commit_b.to_owned(),
        base: merge_base(nodes),
        files_total,
        added,
        deleted,
        files: rows(nodes),
        patch,
    })
}

fn missing_sides(
    repo: &str,
    rev_a: Option<&str>,
    rev_b: &str,
    a: &View<'_>,
    b: &View<'_>,
) -> Error {
    let (no_a, no_b) =
        (a.get(&["commit"]).is_none(), b.get(&["commit"]).is_none());
    match rev_a {
        None => missing_revisions(repo, &[("", rev_b)]),
        Some(rev_a) => {
            let mut missing = Vec::new();
            if no_a {
                missing.push(("base", rev_a));
            }
            if no_b {
                missing.push(("head", rev_b));
            }
            missing_revisions(repo, &missing)
        }
    }
}
