use serde_json::{Map, Value, json};

use super::graphql::{Envelope, GqlError, Segment, View};
use crate::address::{FULL_OID, Repo, Target, is_oid_like, valid_revision};
use crate::error::Error;

const NOT_INDEXED: &str = "sourcegraph.com serves indexed repositories from \
    github.com, gitlab.com and a few other hosts; a renamed repository \
    appears only under its new name";

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
            let revs: Vec<&str> = missing.iter().map(|(_, rev)| *rev).collect();
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

/// One revision and path that an address may name.
pub struct Candidate {
    pub rev: String,
    pub path: Option<String>,
}

/// The revision and path `target` names: one for a revision and a path,
/// and for the segments of a web URL each split into a revision and a path.
/// Split prefixes that are not valid revisions are never sent.
pub fn candidates(target: &Target) -> Vec<Candidate> {
    match target {
        Target::At { rev, path } => vec![Candidate {
            rev: rev.clone().unwrap_or_else(|| "HEAD".to_owned()),
            path: path.clone(),
        }],
        Target::Split(segs) => (1..=segs.len())
            .map(|k| Candidate {
                rev: segs[..k].join("/"),
                path: Some(segs[k..].join("/")).filter(|p| !p.is_empty()),
            })
            .filter(|c| valid_revision(&c.rev))
            .collect(),
    }
}

/// A prefix that could be an abbreviated oid or a revision expression, so
/// it counts as a revision only when a ref of that name exists.
pub fn ambiguous(rev: &str) -> bool {
    let abbreviated = is_oid_like(rev) && rev.len() < FULL_OID;
    abbreviated || rev.contains(['~', '^'])
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

/// The variables naming `repo`: `$repo0`, and `$repo1`, the gitlab.com
/// candidate of `owner/name`, which `$gitlab` switches on. A `String!` is
/// sent even where its part is skipped, so `$repo1` is empty for an exact
/// name.
pub fn repository_variables(repo: &Repo) -> Map<String, Value> {
    let names = repo.candidates();
    let mut variables = Map::new();
    variables.insert("repo0".to_owned(), json!(names[0]));
    variables.insert("repo1".to_owned(), json!(names.get(1).map_or("", String::as_str)));
    variables.insert("gitlab".to_owned(), json!(names.len() > 1));
    variables
}

pub fn repository_lookup_failed() -> Error {
    Error::failed("the repository lookup")
}

/// Whether the errors at or beneath a revision's lookup say it failed. One
/// that says the revision isn't there answers the lookup, as a null commit
/// does: the server sends it when another part of the same request was
/// fetching the repository, and the fetch didn't bring the revision.
pub fn lookup_failed(errors: &[&GqlError]) -> bool {
    errors.iter().any(|error| !error.message.starts_with("revision not found"))
}

pub fn revision_lookup_failed() -> Error {
    Error::failed("the revision lookup")
}

/// A root that came back null with an error at or beneath it: the lookup
/// failed, so the repository is not known to be missing.
fn failed_root(env: &Envelope, alias: &str, roots: usize, parts: &[(&str, &str)]) -> Option<Error> {
    let failed = |root: &str| {
        env.get(&[root]).is_none()
            && env
                .errors
                .iter()
                .any(|e| matches!(e.path.first(), Some(Segment::Key(k)) if k == root))
    };
    let names: Vec<String> = (0..roots).map(|i| format!("{alias}{i}")).collect();
    let error = env.errors.iter().find(|e| {
        matches!(e.path.first(), Some(Segment::Key(k))
            if names.contains(k) && failed(k))
    })?;
    let part = match error.path.get(1) {
        Some(Segment::Key(key)) => {
            parts.iter().find_map(|(name, part)| (name == key).then_some(*part))
        }
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
    let found: Vec<usize> =
        (0..candidates.len()).filter(|i| env.get(&[&format!("{alias}{i}")]).is_some()).collect();
    let name = written_name(repo);
    match found[..] {
        [] => {
            if let Some(error) = failed_root(env, alias, candidates.len(), parts) {
                return Err(error);
            }
            Err(Error::missing(format!("{name} is not on {}", env.host)).fact(NOT_INDEXED))
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
            Err(Error::missing(format!("{name} exists as {} and {}", canonical(a), canonical(b))))
        }
    }
}

pub fn locate(asked: &Repo, target: &Target, env: &Envelope) -> Result<Located, Error> {
    let i = find_root(asked, env, "r", &[])?;
    let r = format!("r{i}");
    let repo = root_name(env, &r).map_or_else(|| asked.candidates()[i].clone(), str::to_owned);
    let cands = candidates(target);
    let oid = |j: usize| env.get(&[&r, &format!("c{j}"), "oid"])?.as_str();
    let failed =
        |j: usize| oid(j).is_none() && lookup_failed(&env.errors_beneath(&[&r, &format!("c{j}")]));
    let branch_failed = || env.error_at(&[&r, "defaultBranch"]).is_some();
    let no_commits = || env.get(&[&r, "defaultBranch"]).is_none();
    let located = |j: usize, commit: &str| Located {
        repo: repo.clone(),
        commit: commit.to_owned(),
        rev: match target {
            Target::At { rev: None, .. } => None,
            _ => Some(cands[j].rev.clone()),
        },
        path: cands[j].path.clone(),
        at: vec![r.clone(), format!("c{j}")],
    };

    let Target::Split(segs) = target else {
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
        oid(j).is_some() && (env.get(&x).is_some() || env.error_at(&x).is_some())
    };
    let names_a_ref = |j: usize| {
        let nodes = env.get(&[&r, &format!("g{j}"), "nodes"]).and_then(|v| v.as_array());
        nodes.is_some_and(|nodes| {
            nodes.iter().any(|node| {
                node.get("displayName").and_then(|d| d.as_str()) == Some(cands[j].rev.as_str())
            })
        })
    };
    let resolving: Vec<usize> = (0..cands.len()).filter(|&j| resolves(j)).collect();
    let best = resolving
        .iter()
        .rev()
        .find(|&&j| !ambiguous(&cands[j].rev) || names_a_ref(j))
        .or_else(|| resolving.last());
    match best.and_then(|&j| oid(j).map(|commit| (j, commit))) {
        Some((j, commit)) => Ok(located(j, commit)),
        None if (0..cands.len()).any(failed) => Err(revision_lookup_failed()),
        None if branch_failed() => Err(repository_lookup_failed()),
        None if no_commits() => Err(Error::missing(format!("{repo} has no commits"))),
        None => Err(Error::missing(format!(
            "no revision and path in {} resolve in {repo}",
            segs.join("/")
        ))),
    }
}

/// The candidate that exists, judged by the root `alias`, the first a
/// repository plan asks. An empty repository is found: the caller reads
/// `defaultBranch` itself. `parts` names the part of the answer each key under
/// the root stands for, for a failed lookup.
pub fn locate_repository(
    repo: &Repo,
    env: &Envelope,
    alias: &str,
    parts: &[(&str, &str)],
) -> Result<RepoAt, Error> {
    let index = find_root(repo, env, alias, parts)?;
    let name = root_name(env, &format!("{alias}{index}"))
        .map_or_else(|| repo.candidates()[index].clone(), str::to_owned);
    Ok(RepoAt { repo: name, index })
}

#[cfg(test)]
mod tests {
    use super::*;

    use serde_json::{Value, json};

    use crate::address::{Repo, Target};
    use crate::error::report;
    use crate::sourcegraph::Response;
    use crate::sourcegraph::graphql::{Envelope, classify};

    const SHA: &str = "98cb6bf42e086f6af920b965c38cacc07402d51b";

    fn exact(name: &str, rev: Option<&str>, path: Option<&str>) -> (Repo, Target) {
        let target = Target::At { rev: rev.map(str::to_owned), path: path.map(str::to_owned) };
        (Repo::Exact(name.to_owned()), target)
    }

    fn owner_name(name: &str, rev: Option<&str>, path: Option<&str>) -> (Repo, Target) {
        let target = Target::At { rev: rev.map(str::to_owned), path: path.map(str::to_owned) };
        (Repo::OwnerName(name.to_owned()), target)
    }

    fn split(name: &str, segments: &[&str]) -> (Repo, Target) {
        let segments = segments.iter().map(ToString::to_string).collect();
        (Repo::Exact(name.to_owned()), Target::Split(segments))
    }

    #[test]
    fn a_slashed_url_revision_has_a_candidate_per_split() {
        let a = split("github.com/bminor/glibc", &["release", "2.34", "master", "NEWS"]);
        let got: Vec<(String, Option<String>)> =
            candidates(&a.1).into_iter().map(|c| (c.rev, c.path)).collect();
        let want = [
            ("release", Some("2.34/master/NEWS")),
            ("release/2.34", Some("master/NEWS")),
            ("release/2.34/master", Some("NEWS")),
            ("release/2.34/master/NEWS", None),
        ]
        .map(|(r, p)| (r.to_owned(), p.map(str::to_owned)));
        assert_eq!(got, want);
    }

    #[test]
    fn an_address_without_a_split_has_one_candidate_and_a_range_has_none() {
        let default = candidates(&exact("github.com/a/b", None, Some("x")).1);
        assert_eq!(default.len(), 1);
        assert_eq!(default[0].rev, "HEAD");
        assert_eq!(default[0].path.as_deref(), Some("x"));

        let one = candidates(&exact("github.com/a/b", Some("v1"), None).1);
        assert_eq!(one.len(), 1);
        assert_eq!(one[0].rev, "v1");
        assert_eq!(one[0].path, None);
    }

    #[test]
    fn invalid_split_prefixes_are_skipped() {
        let a = split("github.com/a/b", &["-x", "a", "b"]);
        assert!(candidates(&a.1).is_empty());

        let a = split("github.com/a/b", &["ok", "bad name", "f"]);
        let got: Vec<String> = candidates(&a.1).into_iter().map(|c| c.rev).collect();
        assert_eq!(got, ["ok"]);

        let a = split("github.com/a/b", &["-x"]);
        assert!(candidates(&a.1).is_empty());
    }

    /// `https://github.com/bminor/glibc/blob/release/2.34/master/NEWS`.
    fn glibc_news() -> (Repo, Target) {
        split("github.com/bminor/glibc", &["release", "2.34", "master", "NEWS"])
    }

    const LOOKUP: &str = "Sourcegraph failed to answer the repository lookup";

    const REVISION_LOOKUP: &str = "Sourcegraph failed to answer the revision lookup";

    fn envelope_on(host: &str, data: &Value, errors: &[Value]) -> Envelope {
        let body = json!({"data": data, "errors": errors});
        classify(Response::of(200, Some("application/json"), body.to_string()), host)
            .unwrap_or_else(|e| panic!("{e}"))
    }

    fn envelope(data: &Value, errors: &[Value]) -> Envelope {
        envelope_on("sourcegraph.com", data, errors)
    }

    fn failing(path: &Value) -> Value {
        json!({"message": "internal git failure", "path": path})
    }

    /// A repository root with its name and default branch, plus the fields of
    /// `rest`.
    fn repository(name: &str, branch: Option<&str>, rest: &Value) -> Value {
        let mut root = json!({
            "name": name,
            "defaultBranch": branch.map(|b| json!({"displayName": b})),
        });
        if let (Some(root), Some(rest)) = (root.as_object_mut(), rest.as_object()) {
            root.extend(rest.clone());
        }
        root
    }

    fn mux(rest: &Value) -> Value {
        repository("github.com/gorilla/mux", Some("main"), rest)
    }

    fn glibc(rest: &Value) -> Value {
        repository("github.com/bminor/glibc", Some("master"), rest)
    }

    /// A split candidate that resolves, with the kind of path at `x`.
    fn resolved(x: &str, kind: Option<&str>) -> Value {
        json!({"oid": SHA, x: kind.map(|k| json!({"__typename": k}))})
    }

    fn found() -> Envelope {
        envelope(&json!({"r0": mux(&json!({"c0": {"oid": SHA}})), "r1": null}), &[])
    }

    fn root_error() -> Envelope {
        envelope(&json!({"r0": null}), &[failing(&json!(["r0"]))])
    }

    fn located(envelope: &Envelope, (repo, target): (Repo, Target)) -> Located {
        locate(&repo, &target, envelope).unwrap_or_else(|e| panic!("{}", report(&e)))
    }

    fn refused(envelope: &Envelope, (repo, target): (Repo, Target)) -> String {
        match locate(&repo, &target, envelope) {
            Ok(found) => panic!("located {found:?}"),
            Err(error) => report(&error),
        }
    }

    fn at(parts: &[&str]) -> Vec<String> {
        parts.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn located_reads_the_repository_commit_revision_and_path_or_names_what_is_missing() {
        assert_eq!(
            located(&found(), owner_name("gorilla/mux", Some("v1.8.0"), Some("mux.go"))),
            Located {
                repo: "github.com/gorilla/mux".to_owned(),
                commit: SHA.to_owned(),
                rev: Some("v1.8.0".to_owned()),
                path: Some("mux.go".to_owned()),
                at: at(&["r0", "c0"]),
            }
        );
        assert_eq!(
            located(&found(), owner_name("gorilla/mux", None, Some("mux.go"))).rev,
            None,
            "the default branch carries no rev"
        );

        let no_repo = envelope(&json!({"r0": null}), &[]);
        let blocked = refused(&no_repo, exact("github.com/facebook/react", None, None));
        assert!(
            blocked.contains("github.com/facebook/react is not on sourcegraph.com"),
            "{blocked}"
        );
        assert!(
            blocked.contains(
                "sourcegraph.com serves indexed repositories from github.com, \
             gitlab.com and a few other hosts; a renamed repository appears \
             only under its new name"
            ),
            "{blocked}"
        );
        assert!(
            refused(&no_repo, owner_name("gorilla/mux", None, None))
                .contains("gorilla/mux is not on sourcegraph.com")
        );

        let elsewhere = envelope_on("h:1", &json!({"r0": null}), &[]);
        let elsewhere = refused(&elsewhere, exact("github.com/x/y", None, None));
        assert!(elsewhere.contains("github.com/x/y is not on h:1"), "{elsewhere}");
        assert!(
            elsewhere.contains("sourcegraph.com serves indexed repositories from"),
            "{elsewhere}"
        );

        let both_hosts = envelope(
            &json!({
                "r0": mux(&json!({"c0": {"oid": SHA}})),
                "r1": repository(
                    "gitlab.com/gorilla/mux",
                    Some("master"),
                    &json!({"c0": {"oid": SHA}}),
                ),
            }),
            &[],
        );
        assert!(refused(&both_hosts, owner_name("gorilla/mux", None, None)).contains(
            "gorilla/mux exists as github.com/gorilla/mux and \
         gitlab.com/gorilla/mux"
        ));
        let no_rev = envelope(&json!({"r0": mux(&json!({"c0": null})), "r1": null}), &[]);
        assert!(
            refused(&no_rev, owner_name("gorilla/mux", Some("v9.9"), None))
                .contains("revision v9.9 not found in github.com/gorilla/mux")
        );
        let empty_repo = envelope(
            &json!({"r0": repository(
                "github.com/ascoders/blog",
                None,
                &json!({"c0": null}),
            )}),
            &[],
        );
        assert!(
            refused(&empty_repo, exact("github.com/ascoders/blog", None, None))
                .contains("github.com/ascoders/blog has no commits")
        );
    }

    #[test]
    fn a_slashed_url_revision_is_located_at_the_split_that_resolves() {
        let split = envelope(
            &json!({"r0": glibc(&json!({
                "c0": resolved("x0", None),
                "g0": null,
                "c1": resolved("x1", None),
                "c2": resolved("x2", Some("GitBlob")),
                "c3": null,
            }))}),
            &[],
        );
        assert_eq!(
            located(&split, glibc_news()),
            Located {
                repo: "github.com/bminor/glibc".to_owned(),
                commit: SHA.to_owned(),
                rev: Some("release/2.34/master".to_owned()),
                path: Some("NEWS".to_owned()),
                at: at(&["r0", "c2"]),
            }
        );

        let errored = envelope(
            &json!({"r0": glibc(&json!({
                "c0": resolved("x0", None),
                "c1": null,
                "c2": resolved("x2", None),
                "c3": null,
            }))}),
            &[json!({"message": "internal", "path": ["r0", "c2", "x2"]})],
        );
        let found = located(&errored, glibc_news());
        assert_eq!(found.rev.as_deref(), Some("release/2.34/master"));
        assert_eq!(found.path.as_deref(), Some("NEWS"));

        let none = envelope(
            &json!({"r0": glibc(&json!({
                "c0": resolved("x0", None),
                "c1": null,
                "c2": null,
                "c3": null,
            }))}),
            &[],
        );
        let none = refused(&none, glibc_news());
        assert!(
            none.contains(
                "no revision and path in release/2.34/master/NEWS resolve in \
             github.com/bminor/glibc"
            ),
            "{none}"
        );
    }

    #[test]
    fn hex_and_tilde_split_prefixes_are_located_by_their_ref_lookups() {
        let prefix = envelope(
            &json!({"r0": repository("github.com/o/r", Some("main"), &json!({
                "c0": resolved("x0", Some("GitBlob")),
                "c1": resolved("x1", Some("GitTree")),
                "g1": {"nodes": []},
            }))}),
            &[],
        );
        let found = located(&prefix, split("github.com/o/r", &["main", "v~1"]));
        assert_eq!(found.rev.as_deref(), Some("main"));
        assert_eq!(found.path.as_deref(), Some("v~1"));
        assert_eq!(found.at, at(&["r0", "c0"]));

        let alone = envelope(
            &json!({"r0": repository("github.com/o/r", Some("main"), &json!({
                "c0": resolved("x0", Some("GitBlob")),
                "g0": {"nodes": []},
                "c1": null,
            }))}),
            &[],
        );
        let found = located(&alone, split("github.com/o/r", &["abcd1234", "f.go"]));
        assert_eq!(found.rev.as_deref(), Some("abcd1234"));
        assert_eq!(found.path.as_deref(), Some("f.go"));
    }

    #[test]
    fn a_revision_the_server_says_is_not_found_reads_as_one_whose_commit_is_null() {
        let waited = |path: Value| {
            json!({
                "message": "revision not found after waiting for a fetch already in progress: repo 1",
                "path": path,
            })
        };
        let exact_data = json!({"r0": mux(&json!({"c0": null}))});
        let missing = exact("github.com/gorilla/mux", Some("v1"), None);
        assert_eq!(
            refused(&envelope(&exact_data, &[waited(json!(["r0", "c0"]))]), missing.clone()),
            refused(&envelope(&exact_data, &[]), missing)
        );
        let split_data = json!({"r0": glibc(&json!({
            "c0": resolved("x0", None),
            "c1": null,
            "c2": resolved("x2", Some("GitBlob")),
            "c3": null,
        }))});
        assert_eq!(
            located(&envelope(&split_data, &[waited(json!(["r0", "c1"]))]), glibc_news()),
            located(&envelope(&split_data, &[]), glibc_news())
        );
    }

    #[test]
    fn an_error_is_a_failed_lookup_not_a_missing_repository_or_revision() {
        let root = refused(&root_error(), exact("github.com/gorilla/mux", None, None));
        assert!(root.contains(LOOKUP), "{root}");
        assert!(!root.contains("internal git failure"), "{root}");

        let commit_error =
            envelope(&json!({"r0": mux(&json!({"c0": null}))}), &[failing(&json!(["r0", "c0"]))]);
        let commit = refused(&commit_error, exact("github.com/gorilla/mux", Some("v1"), None));
        assert!(commit.contains(REVISION_LOOKUP), "{commit}");
        assert!(!commit.contains("internal git failure"), "{commit}");

        let split_error = envelope(
            &json!({"r0": glibc(&json!({
                "c0": null, "c1": null, "c2": null, "c3": null,
            }))}),
            &[failing(&json!(["r0", "c1"]))],
        );
        let split = refused(&split_error, glibc_news());
        assert!(split.contains(REVISION_LOOKUP), "{split}");

        let resolved_beside_error = envelope(
            &json!({"r0": glibc(&json!({
                "c0": resolved("x0", None),
                "c1": null,
                "c2": resolved("x2", Some("GitBlob")),
                "c3": null,
            }))}),
            &[failing(&json!(["r0", "c1"]))],
        );
        let found = located(&resolved_beside_error, glibc_news());
        assert_eq!(found.rev.as_deref(), Some("release/2.34/master"));
    }

    #[test]
    fn a_repository_without_commits_is_named_through_a_split_address() {
        let empty = envelope(
            &json!({"r0": repository(
                "github.com/bminor/glibc",
                None,
                &json!({"c0": null, "c1": null, "c2": null, "c3": null}),
            )}),
            &[],
        );
        let blocked = refused(&empty, glibc_news());
        assert!(blocked.contains("github.com/bminor/glibc has no commits"));
    }

    #[test]
    fn canonical_name_falls_back_to_the_candidate() {
        let unnamed = || {
            json!({
                "defaultBranch": {"displayName": "main"},
                "c0": {"oid": SHA},
            })
        };
        let no_name = envelope(&json!({"r0": unnamed(), "r1": unnamed()}), &[]);
        assert_eq!(
            located(&no_name, exact("github.com/gorilla/mux", Some("v1"), None)).repo,
            "github.com/gorilla/mux"
        );
        assert!(refused(&no_name, owner_name("gorilla/mux", None, None)).contains(
            "gorilla/mux exists as github.com/gorilla/mux and \
         gitlab.com/gorilla/mux"
        ));
    }

    #[test]
    fn an_errored_name_falls_back() {
        let errored = envelope(
            &json!({"r0": repository(
                "github.com/X/Y",
                Some("main"),
                &json!({"c0": {"oid": SHA}}),
            )}),
            &[failing(&json!(["r0", "name"]))],
        );
        assert_eq!(
            located(&errored, exact("github.com/x/y", Some("v1"), None)).repo,
            "github.com/x/y"
        );
    }

    #[test]
    fn an_errored_oid_is_unresolved() {
        let errored = envelope(
            &json!({"r0": mux(&json!({"c0": {"oid": SHA}}))}),
            &[failing(&json!(["r0", "c0", "oid"]))],
        );
        let blocked = refused(&errored, exact("github.com/gorilla/mux", Some("v1"), None));
        assert!(blocked.contains(REVISION_LOOKUP), "{blocked}");
        assert!(!blocked.contains("not found"), "{blocked}");
    }

    #[test]
    fn an_error_inside_a_null_commit_failed() {
        let errored = envelope(
            &json!({"r0": mux(&json!({"c0": null}))}),
            &[failing(&json!(["r0", "c0", "oid"]))],
        );
        let blocked = refused(&errored, exact("github.com/gorilla/mux", Some("v1"), None));
        assert!(blocked.contains(REVISION_LOOKUP), "{blocked}");
        assert!(!blocked.contains("not found"), "{blocked}");
    }

    #[test]
    fn a_resolved_candidate_keeps_its_expected_errors() {
        let expected = envelope(
            &json!({"r0": glibc(&json!({
                "c0": {"oid": SHA, "meta": null, "x0": null},
                "c1": null,
                "c2": null,
                "c3": null,
            }))}),
            &[failing(&json!(["r0", "c0", "meta"]))],
        );
        let none = refused(&expected, glibc_news());
        assert!(
            none.contains(
                "no revision and path in release/2.34/master/NEWS resolve in \
             github.com/bminor/glibc"
            ),
            "{none}"
        );
    }

    #[test]
    fn one_host_failing_is_not_one_host_existing() {
        let one_host = envelope(
            &json!({"r0": mux(&json!({"c0": {"oid": SHA}})), "r1": null}),
            &[failing(&json!(["r1"]))],
        );
        let blocked = refused(&one_host, owner_name("gorilla/mux", None, None));
        assert!(blocked.contains(LOOKUP), "{blocked}");
        assert!(!blocked.contains("internal git failure"), "{blocked}");
    }

    #[test]
    fn failed_default_branch_is_not_an_empty_repository() {
        let single = envelope(
            &json!({"r0": repository(
                "github.com/ascoders/blog",
                None,
                &json!({"c0": null}),
            )}),
            &[failing(&json!(["r0", "defaultBranch"]))],
        );
        let single = refused(&single, exact("github.com/ascoders/blog", None, None));
        assert!(single.contains(LOOKUP), "{single}");

        let split = envelope(
            &json!({"r0": repository(
                "github.com/bminor/glibc",
                None,
                &json!({"c0": null, "c1": null, "c2": null, "c3": null}),
            )}),
            &[failing(&json!(["r0", "defaultBranch"]))],
        );
        let split = refused(&split, glibc_news());
        assert!(split.contains(LOOKUP), "{split}");
    }

    fn repo_refused(envelope: &Envelope, (repo, _): (Repo, Target)) -> String {
        match locate_repository(&repo, envelope, "r", REPO_PARTS) {
            Ok(found) => panic!("located {}", found.repo),
            Err(error) => report(&error),
        }
    }

    #[test]
    fn repository_lookups_name_what_is_missing_or_failed() {
        let none = envelope(&json!({"r0": null, "r1": null}), &[]);
        let blocked = repo_refused(&none, owner_name("gorilla/mux", None, None));
        assert!(blocked.contains("gorilla/mux is not on sourcegraph.com"), "{blocked}");
        assert!(
            blocked.contains(
                "sourcegraph.com serves indexed repositories from github.com, \
             gitlab.com and a few other hosts; a renamed repository appears \
             only under its new name"
            ),
            "{blocked}"
        );

        let both_hosts = envelope(
            &json!({
                "r0": mux(&json!({})),
                "r1": repository("gitlab.com/gorilla/mux", Some("master"), &json!({})),
            }),
            &[],
        );
        let both = repo_refused(&both_hosts, owner_name("gorilla/mux", None, None));
        assert!(
            both.contains(
                "gorilla/mux exists as github.com/gorilla/mux and \
             gitlab.com/gorilla/mux"
            ),
            "{both}"
        );

        let one_host =
            envelope(&json!({"r0": mux(&json!({})), "r1": null}), &[failing(&json!(["r1"]))]);
        let one = repo_refused(&one_host, owner_name("gorilla/mux", None, None));
        assert!(one.contains(LOOKUP), "{one}");
    }

    #[test]
    fn a_null_root_with_errors_beneath_failed() {
        let null_root = |path: &Value| envelope(&json!({"r0": null}), &[failing(path)]);

        let tags = repo_refused(
            &null_root(&json!(["r0", "tags", "nodes"])),
            exact("github.com/gorilla/mux", None, None),
        );
        assert!(tags.contains("Sourcegraph failed to answer the tag list"), "{tags}");
        assert!(!tags.contains("not on sourcegraph.com"), "{tags}");

        let branches = repo_refused(
            &null_root(&json!(["r0", "all_branches", "nodes", 3, "target"])),
            exact("github.com/gorilla/mux", None, None),
        );
        assert!(branches.contains("Sourcegraph failed to answer the branch list"), "{branches}");

        for path in [json!(["r0"]), json!(["r0", "contributors"])] {
            let lookup =
                repo_refused(&null_root(&path), exact("github.com/gorilla/mux", None, None));
            assert!(lookup.contains(LOOKUP), "{path}: {lookup}");
        }

        let one_host_tags = envelope(
            &json!({"r0": mux(&json!({})), "r1": null}),
            &[failing(&json!(["r1", "tags", "nodes"]))],
        );
        let one = repo_refused(&one_host_tags, owner_name("gorilla/mux", None, None));
        assert!(one.contains("Sourcegraph failed to answer the tag list"), "{one}");

        let both_null = envelope(
            &json!({"r0": null, "r1": null}),
            &[failing(&json!(["r1", "all_tags", "nodes"]))],
        );
        let both = repo_refused(&both_null, owner_name("gorilla/mux", None, None));
        assert!(both.contains(LOOKUP), "all_tags is not in this table: {both}");
    }

    const REPO_PARTS: &[(&str, &str)] =
        &[("tags", "the tag list"), ("all_branches", "the branch list")];

    #[test]
    fn only_the_establishing_root_decides() {
        let second_root_error = envelope(
            &json!({
                "m0": mux(&json!({"commit": {"oid": SHA}})),
                "d0": null,
            }),
            &[failing(&json!(["d0", "comparison"]))],
        );
        let found = locate_repository(
            &exact("github.com/gorilla/mux", None, None).0,
            &second_root_error,
            "m",
            &[],
        )
        .unwrap_or_else(|e| panic!("{}", report(&e)));
        assert_eq!(found, RepoAt { repo: "github.com/gorilla/mux".to_owned(), index: 0 });
        assert_eq!(found.at("m"), "m0");
        assert_eq!(found.at("d"), "d0");

        let first_root_error = envelope(
            &json!({
                "m0": null,
                "d0": mux(&json!({"commit": {"oid": SHA}})),
            }),
            &[failing(&json!(["m0", "commit"]))],
        );
        let blocked = match locate_repository(
            &exact("github.com/gorilla/mux", None, None).0,
            &first_root_error,
            "m",
            &[],
        ) {
            Ok(found) => panic!("located {}", found.repo),
            Err(error) => report(&error),
        };
        assert!(blocked.contains(LOOKUP), "{blocked}");
    }
}
