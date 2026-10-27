//! Resolution: which repository an address names, and the commit its
//! revision names there, each looked up once per invocation (ADR 0002).

use serde_json::{Map, Value, json};

use super::graphql::{Envelope, GqlErrorKind, Request};
use crate::address::{FULL_OID, Repo, Target, is_name, is_oid_like, valid_revision};
use crate::error::{Error, Kind};

pub const NOT_INDEXED: &str = "sourcegraph.com serves indexed repositories from \
    github.com, gitlab.com and a few other hosts; a renamed repository \
    appears only under its new name";

/// An address resolved to a commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    /// The repository's name as the server spells it.
    pub repo: String,
    pub commit: String,
    /// The revision as written; `None` for the default branch.
    pub rev: Option<String>,
    /// The path as read, from the repository's root.
    pub path: Option<String>,
    /// When Sourcegraph last updated its mirror, as read with the answer;
    /// `None` when the revision was a commit, or the server didn't say.
    pub mirrored_at: Option<String>,
    /// `Err` when the other host's lookup for `owner/name` failed.
    pub other_host: Result<(), Error>,
}

impl Resolved {
    /// When the mirror was last updated, as this end or a range's other end
    /// read it: both ends share one mirror.
    pub fn mirrored_with(&self, other: Option<&Resolved>) -> Option<String> {
        self.mirrored_at.clone().or_else(|| other.and_then(|other| other.mirrored_at.clone()))
    }

    pub fn new(
        found: &Repository,
        commit: String,
        rev: Option<String>,
        path: Option<String>,
    ) -> Resolved {
        let named = rev.as_deref().is_none_or(|rev| is_name(rev, &commit));
        Resolved {
            repo: found.name.clone(),
            mirrored_at: found.updated_at.clone().filter(|_| named),
            other_host: found.other_host.clone(),
            commit,
            rev,
            path,
        }
    }
}

/// The repository a response found for an address.
#[derive(Debug)]
pub struct Repository {
    /// The name as the server spells it.
    pub name: String,
    /// The response key it was read at.
    pub root: String,
    /// Whether it has a default branch, which only a repository with no
    /// commits lacks.
    has_commits: bool,
    /// Whether the default branch's lookup failed, which leaves that unknown.
    branch_failed: bool,
    /// When Sourcegraph last updated its mirror, to the minute.
    pub updated_at: Option<String>,
    pub other_host: Result<(), Error>,
}

impl Repository {
    /// The error for a repository with no commits, or whose default branch's
    /// lookup failed; `None` when it has commits.
    pub fn no_commits(&self) -> Option<Error> {
        if self.branch_failed {
            Some(Error::failed("the repository lookup"))
        } else if self.has_commits {
            None
        } else {
            Some(Error::not_found(format!("{} has no commits", self.name)))
        }
    }
}

/// An RFC 3339 time in UTC to the minute, `2026-10-27T04:17Z`, or as the
/// server wrote it when it is no such time.
fn to_the_minute(time: &str) -> String {
    match time.get(..16) {
        Some(minute) if time.ends_with('Z') => format!("{minute}Z"),
        _ => time.to_owned(),
    }
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

/// The revisions of an address that name no commit in `repo`: one
/// revision, a range's base or head, or both ends of a range.
enum Missing<'a> {
    Revision(&'a str),
    Base(&'a str),
    Head(&'a str),
    Both(&'a str, &'a str),
}

fn missing_revisions(repo: &str, missing: &Missing<'_>) -> Error {
    let message = match missing {
        Missing::Revision(rev) => format!("revision {rev} not found in {repo}{}", hex_reading(rev)),
        Missing::Base(rev) => {
            format!("base revision {rev} not found in {repo}{}", hex_reading(rev))
        }
        Missing::Head(rev) => {
            format!("head revision {rev} not found in {repo}{}", hex_reading(rev))
        }
        Missing::Both(base, head) => format!("revisions {base} and {head} not found in {repo}"),
    };
    Error::not_found(message)
}

/// What a missing revision of 4 to 39 hex digits rules out, which is true
/// whether it names nothing or more than one commit.
fn hex_reading(rev: &str) -> &'static str {
    if is_oid_like(rev) && rev.len() < FULL_OID {
        ": no ref has that name and no single commit starts with it"
    } else {
        ""
    }
}

/// The candidate of `repo` the response holds, read from the roots `roots`,
/// one per candidate in order. Both of `owner/name`'s candidates existing is
/// a usage error: the address must name its host.
pub fn repository(repo: &Repo, envelope: &Envelope, roots: &[&str]) -> Result<Repository, Error> {
    let candidates = repo.candidates();
    let name_at = |i: usize| {
        envelope
            .get(&[roots[i], "name"])
            .and_then(Value::as_str)
            .map_or_else(|| candidates[i].clone(), str::to_owned)
    };
    let found: Vec<usize> =
        (0..roots.len()).filter(|&i| envelope.get(&[roots[i]]).is_some()).collect();
    let failed = |i: usize| {
        envelope.get(&[roots[i]]).is_none() && envelope.error_beneath(&[roots[i]]).is_some()
    };
    match found[..] {
        [] if (0..roots.len()).any(failed) => Err(Error::failed("the repository lookup")),
        [] => {
            let written = match repo {
                Repo::Exact(name) | Repo::OwnerName(name) => name,
            };
            Err(Error::not_found(format!("{written} is not on {}", envelope.host))
                .fact(NOT_INDEXED))
        }
        [i] => {
            let other = (0..roots.len()).find(|&j| j != i && failed(j));
            let root = roots[i];
            Ok(Repository {
                name: name_at(i),
                root: root.to_owned(),
                has_commits: envelope.get(&[root, "defaultBranch"]).is_some(),
                branch_failed: envelope.error_beneath(&[root, "defaultBranch"]).is_some(),
                updated_at: envelope
                    .get(&[root, "mirrorInfo", "updatedAt"])
                    .and_then(Value::as_str)
                    .map(to_the_minute),
                other_host: match other {
                    Some(j) => Err(Error::failed(&format!("whether {} exists", candidates[j]))),
                    None => Ok(()),
                },
            })
        }
        [a, b, ..] => {
            let written = match repo {
                Repo::Exact(name) | Repo::OwnerName(name) => name,
            };
            Err(Error::usage_from_response(format!(
                "{written} exists as {} and {}",
                name_at(a),
                name_at(b)
            )))
        }
    }
}

/// The full commit ID that `rev` names at `at`, the path of a `commit`
/// lookup in `found`.
pub fn commit(
    envelope: &Envelope,
    found: &Repository,
    at: &[&str],
    rev: &str,
) -> Result<String, Error> {
    if let Some(oid) = envelope.get(&[at, &["oid"]].concat()).and_then(Value::as_str) {
        return Ok(oid.to_owned());
    }
    if envelope.errors_beneath(at).iter().any(|e| e.kind() != GqlErrorKind::RevisionNotFound) {
        return Err(Error::failed("the revision lookup"));
    }
    Err(found
        .no_commits()
        .unwrap_or_else(|| missing_revisions(&found.name, &Missing::Revision(rev))))
}

/// Both ends of a range, each looked up once. Two missing names share one
/// fetch, so one may come back after waiting for it, which reads as missing
/// too.
const RANGE: &str = "\
query Range($repo0: String!, $repo1: String!, $gitlab: Boolean!, $a: String!, $b: String!) {
  r0: repository(name: $repo0) { ...Ends }
  r1: repository(name: $repo1) @include(if: $gitlab) { ...Ends }
}

fragment Ends on Repository {
  name
  defaultBranch { displayName }
  mirrorInfo { updatedAt }
  a: commit(rev: $a) { oid }
  b: commit(rev: $b) { oid }
}
";

/// The request that resolves the range `base` and `head` in `repo`.
pub fn range_request(repo: &Repo, (base, head): (&str, &str)) -> Request {
    let mut variables = repository_variables(repo);
    variables.insert("a".to_owned(), json!(base));
    variables.insert("b".to_owned(), json!(head));
    Request::new("Range", RANGE.to_owned(), variables)
}

/// Both ends of the range `base` and `head`, from the response to
/// `range_request`.
pub fn read_range(
    envelope: &Envelope,
    repo: &Repo,
    (base, head): (&str, &str),
) -> Result<(Resolved, Resolved), Error> {
    let roots: &[&str] = if repo.candidates().len() > 1 { &["r0", "r1"] } else { &["r0"] };
    let found = repository(repo, envelope, roots)?;
    let a = commit(envelope, &found, &[&found.root, "a"], base);
    let b = commit(envelope, &found, &[&found.root, "b"], head);
    let (a, b) = match (a, b) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(e), _) | (_, Err(e)) if e.kind() != Kind::NotFound => return Err(e),
        (Err(_), Err(_)) => {
            return Err(found
                .no_commits()
                .unwrap_or_else(|| missing_revisions(&found.name, &Missing::Both(base, head))));
        }
        (Err(_), Ok(_)) => return Err(missing_revisions(&found.name, &Missing::Base(base))),
        (Ok(_), Err(_)) => return Err(missing_revisions(&found.name, &Missing::Head(head))),
    };
    let end = |oid: String, rev: &str| Resolved::new(&found, oid, Some(rev.to_owned()), None);
    Ok((end(a, base), end(b, head)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn split(segments: &[&str]) -> Target {
        Target::Split(segments.iter().map(ToString::to_string).collect())
    }

    #[test]
    fn a_slashed_url_revision_has_a_candidate_per_split() {
        let got: Vec<(String, Option<String>)> =
            candidates(&split(&["release", "2.34", "master", "NEWS"]))
                .into_iter()
                .map(|c| (c.rev, c.path))
                .collect();
        let want = [
            ("release", Some("2.34/master/NEWS")),
            ("release/2.34", Some("master/NEWS")),
            ("release/2.34/master", Some("NEWS")),
            ("release/2.34/master/NEWS", None),
        ]
        .map(|(r, p)| (r.to_owned(), p.map(str::to_owned)));
        assert_eq!(got, want);
        assert!(candidates(&split(&["-x", "a"])).is_empty());
    }

    #[test]
    fn a_missing_hex_revision_says_what_it_rules_out() {
        let message = |rev: &str| missing_revisions("r", &Missing::Revision(rev)).to_string();
        assert_eq!(
            message("49c0"),
            "revision 49c0 not found in r: no ref has that name and no single commit starts with it"
        );
        assert_eq!(message("main"), "revision main not found in r");
        assert_eq!(message(&"a".repeat(40)), format!("revision {} not found in r", "a".repeat(40)));
    }
}
