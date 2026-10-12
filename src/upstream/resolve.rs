use std::fmt::Write as _;

use serde::Serialize;
use serde_json::{Map, Value};

use super::envelope::{Envelope, Seg};
use super::view::View;
use crate::address::{
    Address, FULL_OID, Repo, Rev, is_oid_like, valid_revision,
};
use crate::error::Error;

const NOT_INDEXED: &str = "sourcegraph.com serves indexed repositories from \
    github.com, gitlab.com and a few other hosts; a renamed repository \
    appears only under its new name";

pub struct Var {
    pub name: &'static str,
    pub ty: &'static str,
    pub value: Value,
}

/// One revision/path reading of an address.
pub struct Candidate {
    pub rev: String,
    pub path: Option<String>,
}

/// The variables a plan sends for one candidate.
pub type Vars = Box<dyn Fn(&Candidate) -> Vec<Var>>;

pub struct Plan {
    pub operation: &'static str,
    /// A selection on `GitCommit`. Its `$name` placeholders are renamed
    /// `$name_<j>` for candidate `j`.
    pub commit_template: &'static str,
    pub vars: Vars,
}

/// One selection on `Repository`, asked under its own top-level alias.
pub struct Root {
    pub alias: &'static str,
    pub template: String,
}

pub struct RepoPlan {
    pub operation: &'static str,
    /// Declared once, after the `$repo<i>` variables, and shared by every
    /// root.
    pub vars: Vec<Var>,
    /// The first root establishes whether the repository exists; a failure
    /// in any other is for the command to read.
    pub roots: Vec<Root>,
    /// The second key of an error path under the establishing root, and the
    /// part of the answer it names.
    pub parts: &'static [(&'static str, &'static str)],
}

#[derive(Debug, PartialEq, Eq)]
pub struct RepoAt {
    /// The canonical name from the response.
    pub repo: String,
    /// The index of the candidate that exists.
    pub index: usize,
}

impl RepoAt {
    /// The response key of a root: `<alias><index>`, such as `c0`.
    pub fn at(&self, alias: &str) -> String {
        format!("{alias}{}", self.index)
    }
}

/// `side` is `""`, `"base"` or `"head"`; `missing` holds one or two
/// `(side, revision)` pairs.
pub fn missing_revisions(repo: &str, missing: &[(&str, &str)]) -> Error {
    let message = match missing {
        [("", rev)] => {
            format!("revision {rev} not found in {repo}")
        }
        [(side, rev)] => format!("{side} revision {rev} not found in {repo}"),
        _ => {
            let revs: Vec<&str> =
                missing.iter().map(|(_, rev)| *rev).collect();
            format!("revisions {} not found in {repo}", revs.join(" and "))
        }
    };
    Error::missing(message)
}

/// The error for a null `commit` under `view`, which must also select
/// `defaultBranch`: a repository with a default branch lacks the revision,
/// so this is `None`; one without has no commits at all.
pub fn no_commits(repo: &str, view: &View<'_>) -> Option<Error> {
    if view.get(&["defaultBranch"]).is_some() {
        return None;
    }
    Some(if view.errored_beneath(&["defaultBranch"]) {
        repository_lookup_failed()
    } else {
        Error::missing(format!("{repo} has no commits"))
    })
}

/// The revisions of a range that name no commit, each given with the view
/// that read its `commit`.
pub fn missing_sides(
    repo: &str,
    (base, base_view): (&str, &View<'_>),
    (head, head_view): (&str, &View<'_>),
) -> Error {
    let mut missing = Vec::new();
    if base_view.get(&["commit"]).is_none() {
        missing.push(("base", base));
    }
    if head_view.get(&["commit"]).is_none() {
        missing.push(("head", head));
    }
    missing_revisions(repo, &missing)
}

pub struct Request {
    pub operation: String,
    pub document: String,
    pub variables: Map<String, Value>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Body<'a> {
    query: &'a str,
    variables: &'a Map<String, Value>,
    operation_name: &'a str,
}

impl Request {
    /// # Panics
    ///
    /// Never in practice: the body is strings and JSON values.
    pub fn body(&self) -> Vec<u8> {
        let body = Body {
            query: &self.document,
            variables: &self.variables,
            operation_name: &self.operation,
        };
        serde_json::to_vec(&body).unwrap_or_else(|e| panic!("{e}"))
    }
}

/// Ranges have no candidates: commands reject them before resolving.
/// Split prefixes that are not valid revisions are never sent.
pub fn candidates(address: &Address) -> Vec<Candidate> {
    match &address.rev {
        Rev::Default => vec![Candidate {
            rev: "HEAD".to_owned(),
            path: address.path.clone(),
        }],
        Rev::One(rev) => {
            vec![Candidate { rev: rev.clone(), path: address.path.clone() }]
        }
        Rev::Range { .. } => Vec::new(),
        Rev::Split(segs) => (1..=segs.len())
            .map(|k| Candidate {
                rev: segs[..k].join("/"),
                path: Some(segs[k..].join("/")).filter(|p| !p.is_empty()),
            })
            .filter(|c| valid_revision(&c.rev))
            .collect(),
    }
}

fn is_split(address: &Address) -> bool {
    matches!(address.rev, Rev::Split(_))
}

/// A prefix that could be an abbreviated oid or a revision expression, so
/// it counts as a revision only when a ref of that name exists.
fn ambiguous(rev: &str) -> bool {
    let abbreviated = is_oid_like(rev) && rev.len() < FULL_OID;
    abbreviated || rev.contains(['~', '^'])
}

fn rename(template: &str, name: &str, suffix: usize) -> String {
    let needle = format!("${name}");
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(at) = rest.find(&needle) {
        let after = &rest[at + needle.len()..];
        out.push_str(&rest[..at]);
        out.push_str(&needle);
        if !after.starts_with(|c: char| c.is_alphanumeric() || c == '_') {
            let _ = write!(out, "_{suffix}");
        }
        rest = after;
    }
    out.push_str(rest);
    out
}

/// # Panics
///
/// On a range address, which commands reject before resolving.
pub fn build(plan: &Plan, address: &Address) -> Request {
    assert!(
        !matches!(address.rev, Rev::Range { .. }),
        "build does not take a range address"
    );
    let repos = address.repo.candidates();
    let cands = candidates(address);
    let split = is_split(address);

    let mut declared: Vec<(String, &'static str)> = Vec::new();
    let mut variables = Map::new();
    for (i, repo) in repos.iter().enumerate() {
        declared.push((format!("repo{i}"), "String!"));
        variables.insert(format!("repo{i}"), Value::String(repo.clone()));
    }
    for (j, cand) in cands.iter().enumerate() {
        declared.push((format!("rev{j}"), "String!"));
        variables.insert(format!("rev{j}"), Value::String(cand.rev.clone()));
    }
    let mut selections = Vec::new();
    for (j, cand) in cands.iter().enumerate() {
        let mut vars = (plan.vars)(cand);
        if split && !vars.iter().any(|v| v.name == "path") {
            vars.push(Var {
                name: "path",
                ty: "String!",
                value: Value::String(cand.path.clone().unwrap_or_default()),
            });
        }
        let mut template = plan.commit_template.to_owned();
        for var in vars {
            template = rename(&template, var.name, j);
            let name = format!("{}_{j}", var.name);
            declared.push((name.clone(), var.ty));
            variables.insert(name, var.value);
        }
        selections.push(template);
    }

    let mut document = format!("query {}(", plan.operation);
    for (n, (name, ty)) in declared.iter().enumerate() {
        if n > 0 {
            document.push_str(", ");
        }
        let _ = write!(document, "${name}: {ty}");
    }
    document.push_str(") {\n");
    for i in 0..repos.len() {
        let _ = writeln!(document, "  r{i}: repository(name: $repo{i}) {{");
        document.push_str("    name\n    defaultBranch { displayName }\n");
        for (j, selection) in selections.iter().enumerate() {
            let _ = writeln!(document, "    c{j}: commit(rev: $rev{j}) {{");
            document.push_str("      oid\n");
            let _ = writeln!(document, "      {selection}");
            if split {
                let _ = writeln!(
                    document,
                    "      x{j}: path(path: $path_{j}) {{ __typename }}"
                );
            }
            document.push_str("    }\n");
            if split && ambiguous(&cands[j].rev) {
                let _ = writeln!(
                    document,
                    "    g{j}: gitRefs(query: $rev{j}, first: 5) {{ nodes \
                     {{ displayName }} }}"
                );
            }
        }
        document.push_str("  }\n");
    }
    document.push_str("}\n");

    Request { operation: plan.operation.to_owned(), document, variables }
}

pub fn build_repository(plan: &RepoPlan, repo: &Repo) -> Request {
    let repos = repo.candidates();
    let mut declared: Vec<String> =
        (0..repos.len()).map(|i| format!("$repo{i}: String!")).collect();
    declared.extend(
        plan.vars.iter().map(|var| format!("${}: {}", var.name, var.ty)),
    );
    let mut variables = Map::new();
    for (i, name) in repos.iter().enumerate() {
        variables.insert(format!("repo{i}"), Value::String(name.clone()));
    }
    for var in &plan.vars {
        variables.insert(var.name.to_owned(), var.value.clone());
    }

    let mut document =
        format!("query {}({}) {{\n", plan.operation, declared.join(", "));
    for i in 0..repos.len() {
        for root in &plan.roots {
            let _ = writeln!(
                document,
                "  {}{i}: repository(name: $repo{i}) {{",
                root.alias
            );
            document.push_str("    name\n    defaultBranch { displayName }\n");
            for line in root.template.lines() {
                let _ = writeln!(document, "    {line}");
            }
            document.push_str("  }\n");
        }
    }
    document.push_str("}\n");

    Request { operation: plan.operation.to_owned(), document, variables }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Located {
    /// The canonical name from the response.
    pub repo: String,
    pub commit: String,
    /// As asked; `None` for the default branch.
    pub rev: Option<String>,
    pub path: Option<String>,
    /// The alias path to the commit, such as `["r0", "c0"]`.
    pub at: Vec<String>,
}

fn written_name(repo: &Repo) -> &str {
    match repo {
        Repo::Exact(name) | Repo::OwnerName(name) => name,
    }
}

/// The canonical repository name at the root `root`.
fn root_name<'a>(env: &'a Envelope, root: &str) -> Option<&'a str> {
    env.get(&[root, "name"])?.as_str()
}

pub fn repository_lookup_failed() -> Error {
    Error::failed("the repository lookup")
}

pub fn revision_lookup_failed() -> Error {
    Error::failed("the revision lookup")
}

/// A root that came back null with an error at or beneath it: the lookup
/// failed, so the repository is not known to be missing.
fn failed_root(
    env: &Envelope,
    alias: &str,
    roots: usize,
    parts: &[(&str, &str)],
) -> Option<Error> {
    let failed = |root: &str| {
        env.get(&[root]).is_none()
            && env.errors.iter().any(
                |e| matches!(e.path.first(), Some(Seg::Key(k)) if k == root),
            )
    };
    let names: Vec<String> =
        (0..roots).map(|i| format!("{alias}{i}")).collect();
    let error = env.errors.iter().find(|e| {
        matches!(e.path.first(), Some(Seg::Key(k))
            if names.contains(k) && failed(k))
    })?;
    let part = match error.path.get(1) {
        Some(Seg::Key(key)) => parts
            .iter()
            .find_map(|(name, part)| (name == key).then_some(*part)),
        _ => None,
    };
    Some(part.map_or_else(repository_lookup_failed, Error::failed))
}

/// `alias` names the root whose null-or-present answer decides whether the
/// repository exists; errors under any other root are not read here.
fn find_root(
    repo: &Repo,
    env: &Envelope,
    alias: &str,
    parts: &[(&str, &str)],
) -> Result<usize, Error> {
    let candidates = repo.candidates();
    let found: Vec<usize> = (0..candidates.len())
        .filter(|i| env.get(&[&format!("{alias}{i}")]).is_some())
        .collect();
    let name = written_name(repo);
    match found[..] {
        [] => {
            if let Some(error) =
                failed_root(env, alias, candidates.len(), parts)
            {
                return Err(error);
            }
            Err(Error::missing(format!("{name} is not on {}", env.host))
                .fact(NOT_INDEXED))
        }
        [i] => match failed_root(env, alias, candidates.len(), parts) {
            Some(error) => Err(error),
            None => Ok(i),
        },
        [a, b, ..] => {
            let canonical = |i: usize| {
                root_name(env, &format!("{alias}{i}"))
                    .map_or_else(|| candidates[i].clone(), str::to_owned)
            };
            Err(Error::missing(format!(
                "{name} exists as {} and {}",
                canonical(a),
                canonical(b)
            )))
        }
    }
}

pub fn locate(address: &Address, env: &Envelope) -> Result<Located, Error> {
    let i = find_root(&address.repo, env, "r", &[])?;
    let r = format!("r{i}");
    let repo = root_name(env, &r)
        .map_or_else(|| address.repo.candidates()[i].clone(), str::to_owned);
    let cands = candidates(address);
    let oid = |j: usize| env.get(&[&r, &format!("c{j}"), "oid"])?.as_str();
    let failed = |j: usize| {
        oid(j).is_none()
            && env.error_beneath(&[r.as_str(), &format!("c{j}")]).is_some()
    };
    let branch_failed = || env.error_at(&[&r, "defaultBranch"]).is_some();
    let no_commits = || env.get(&[&r, "defaultBranch"]).is_none();
    let located = |j: usize, commit: &str| Located {
        repo: repo.clone(),
        commit: commit.to_owned(),
        rev: match address.rev {
            Rev::Default => None,
            _ => Some(cands[j].rev.clone()),
        },
        path: cands[j].path.clone(),
        at: vec![r.clone(), format!("c{j}")],
    };

    let Rev::Split(segs) = &address.rev else {
        let Some(commit) = oid(0) else {
            if failed(0) {
                return Err(revision_lookup_failed());
            }
            if branch_failed() {
                return Err(repository_lookup_failed());
            }
            let message = if no_commits() {
                format!("{repo} has no commits")
            } else {
                format!("revision {} not found in {repo}", cands[0].rev)
            };
            return Err(Error::missing(message));
        };
        return Ok(located(0, commit));
    };

    let resolves = |j: usize| {
        let (c, x) = (format!("c{j}"), format!("x{j}"));
        let x = [r.as_str(), c.as_str(), x.as_str()];
        oid(j).is_some()
            && (env.get(&x).is_some() || env.error_at(&x).is_some())
    };
    let names_a_ref = |j: usize| {
        let nodes = env
            .get(&[&r, &format!("g{j}"), "nodes"])
            .and_then(|v| v.as_array());
        nodes.is_some_and(|nodes| {
            nodes.iter().any(|node| {
                node.get("displayName").and_then(|d| d.as_str())
                    == Some(cands[j].rev.as_str())
            })
        })
    };
    let resolving: Vec<usize> =
        (0..cands.len()).filter(|&j| resolves(j)).collect();
    let best = resolving
        .iter()
        .rev()
        .find(|&&j| !ambiguous(&cands[j].rev) || names_a_ref(j))
        .or_else(|| resolving.last());
    match best.and_then(|&j| oid(j).map(|commit| (j, commit))) {
        Some((j, commit)) => Ok(located(j, commit)),
        None if (0..cands.len()).any(failed) => Err(revision_lookup_failed()),
        None if branch_failed() => Err(repository_lookup_failed()),
        None if no_commits() => {
            Err(Error::missing(format!("{repo} has no commits")))
        }
        None => Err(Error::missing(format!(
            "no revision and path in {} resolve in {repo}",
            segs.join("/")
        ))),
    }
}

/// `query <operation>($a: T, ...) { <selection> }`, with the variables
/// passed separately.
pub fn top_level(
    operation: &'static str,
    vars: Vec<Var>,
    selection: &str,
) -> Request {
    let declared: Vec<String> =
        vars.iter().map(|var| format!("${}: {}", var.name, var.ty)).collect();
    let header = if declared.is_empty() {
        operation.to_owned()
    } else {
        format!("{operation}({})", declared.join(", "))
    };
    let mut body = String::new();
    for line in selection.lines() {
        let _ = writeln!(body, "  {line}");
    }
    let document = format!("query {header} {{\n{body}}}\n");
    let variables =
        vars.into_iter().map(|var| (var.name.to_owned(), var.value)).collect();
    Request { operation: operation.to_owned(), document, variables }
}

/// The candidate that exists, judged by the plan's first root. An empty
/// repository is found: the caller reads `defaultBranch` itself.
///
/// # Panics
///
/// On a plan with no roots.
pub fn locate_repository(
    repo: &Repo,
    env: &Envelope,
    plan: &RepoPlan,
) -> Result<RepoAt, Error> {
    let establishing = plan
        .roots
        .first()
        .unwrap_or_else(|| panic!("a repository plan needs a root"));
    let alias = establishing.alias;
    let index = find_root(repo, env, alias, plan.parts)?;
    let name = root_name(env, &format!("{alias}{index}"))
        .map_or_else(|| repo.candidates()[index].clone(), str::to_owned);
    Ok(RepoAt { repo: name, index })
}
