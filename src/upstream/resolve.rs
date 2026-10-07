use std::fmt::Write as _;

use serde::Serialize;
use serde_json::{Map, Value};

use super::envelope::Envelope;
use crate::address::{Address, Repo, Rev, valid_revision};
use crate::error::Error;

const NOT_INDEXED: &str = "sourcegraph.com serves indexed repositories from \
    github.com, gitlab.com and a few other hosts; a renamed repository \
    appears only under its new name";
const MIN_ABBREVIATION: usize = 4;
const FULL_OID: usize = 40;

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

pub struct Plan {
    pub operation: &'static str,
    /// A selection on `GitCommit`. Its `$name` placeholders are renamed
    /// `$name_<j>` for candidate `j`.
    pub commit_template: &'static str,
    pub vars: fn(&Candidate) -> Vec<Var>,
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
    let hex = (MIN_ABBREVIATION..FULL_OID).contains(&rev.len())
        && rev.bytes().all(|b| b.is_ascii_hexdigit());
    hex || rev.contains(['~', '^'])
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

fn str_at<'a>(env: &'a Envelope, path: &[&str]) -> Option<&'a str> {
    env.get(path)?.as_str()
}

fn repository_lookup_failed() -> Error {
    Error::upstream("Sourcegraph failed to answer the repository lookup")
}

fn find_root(address: &Address, env: &Envelope) -> Result<usize, Error> {
    let found: Vec<usize> = (0..address.repo.candidates().len())
        .filter(|i| env.get(&[&format!("r{i}")]).is_some())
        .collect();
    let name = written_name(&address.repo);
    let any_root_failed = || {
        (0..address.repo.candidates().len())
            .any(|i| env.error_at(&[&format!("r{i}")]).is_some())
    };
    match found[..] {
        [] => {
            if any_root_failed() {
                return Err(repository_lookup_failed());
            }
            Err(Error::missing(format!("{name} is not on sourcegraph.com"))
                .fact(NOT_INDEXED))
        }
        [_] if any_root_failed() => Err(repository_lookup_failed()),
        [i] => Ok(i),
        [a, b, ..] => {
            let canonical = |i: usize| {
                str_at(env, &[&format!("r{i}"), "name"]).map_or_else(
                    || address.repo.candidates()[i].clone(),
                    str::to_owned,
                )
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
    let i = find_root(address, env)?;
    let r = format!("r{i}");
    let repo = str_at(env, &[&r, "name"])
        .map_or_else(|| address.repo.candidates()[i].clone(), str::to_owned);
    let cands = candidates(address);
    let oid = |j: usize| str_at(env, &[&r, &format!("c{j}"), "oid"]);
    let failed =
        |j: usize| env.error_at(&[r.as_str(), &format!("c{j}")]).is_some();
    let revision_lookup_failed =
        || Error::upstream("Sourcegraph failed to answer the revision lookup");
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
