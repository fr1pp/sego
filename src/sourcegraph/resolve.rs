//! Resolution: which repository an address names, and the commit its
//! revision names there, each looked up once per invocation (ADR 0002).

use serde_json::{Map, Value, json};

use super::graphql::{Envelope, GqlErrorKind, Request};
use crate::address::{FULL_OID, Repo, is_name, is_oid_like};
use crate::error::{Error, Kind};
use crate::pointer;

pub const SOURCEGRAPH_COM_MIRRORS: &str = "sourcegraph.com serves indexed repositories from \
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
    pub lookup: Lookup,
}

/// What looking up an address's repository adds beside the answer, which
/// every answer that resolved an address takes in through
/// `noting_other_host`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lookup {
    /// When Sourcegraph last updated its mirror, as read with the response;
    /// `None` when the revision was a commit, or the server didn't say.
    mirrored_at: Option<String>,
    /// `Err` when the other host's lookup for `owner/name` failed.
    other_host: Result<(), Error>,
}

impl Lookup {
    /// Hands a failed lookup of the other host to `note`, and returns when
    /// Sourcegraph last updated its mirror: neither is taken without the
    /// other.
    pub fn noting_other_host(self, note: impl FnOnce(String)) -> Option<String> {
        if let Err(error) = self.other_host {
            note(error.to_string());
        }
        self.mirrored_at
    }
}

impl Resolved {
    /// The commit as a pointer names it.
    pub fn at(&self) -> pointer::At<'_> {
        pointer::At { repo: &self.repo, commit: &self.commit }
    }

    /// The lookup as this end or a range's other end read it: both ends
    /// share one repository and one mirror.
    pub fn lookup_with(&self, other: Option<&Resolved>) -> Lookup {
        let mirrored_at = self.lookup.mirrored_at.clone();
        Lookup {
            mirrored_at: mirrored_at.or_else(|| other?.lookup.mirrored_at.clone()),
            other_host: self.lookup.other_host.clone(),
        }
    }

    pub fn new(
        found: &Repository,
        commit: String,
        rev: Option<String>,
        path: Option<String>,
    ) -> Resolved {
        let named = rev.as_deref().is_none_or(|rev| is_name(rev, &commit));
        let mut lookup = found.lookup();
        lookup.mirrored_at = lookup.mirrored_at.filter(|_| named);
        Resolved { repo: found.name.clone(), lookup, commit, rev, path }
    }
}

/// The two ends of a range, each resolved.
#[derive(Debug, PartialEq, Eq)]
pub struct Ends {
    pub base: Resolved,
    pub head: Resolved,
}

/// The repository a response found for an address.
#[derive(Debug)]
pub struct Repository {
    /// The name as the server spells it.
    pub name: String,
    /// The response key it was read at.
    pub root: String,
    /// Which of the address's candidates it is, in order.
    candidate: usize,
    /// The default branch's name, which only a repository with no commits
    /// lacks.
    default_branch: Option<String>,
    /// Whether the default branch's lookup failed, which leaves that unknown.
    branch_failed: bool,
    /// When Sourcegraph last updated its mirror, to the minute.
    pub updated_at: Option<String>,
    pub other_host: Result<(), Error>,
}

impl Repository {
    /// The one of `keys`, a root per candidate of the address in order,
    /// that this candidate is read at.
    pub fn root_among<'k>(&self, keys: [&'k str; 2]) -> &'k str {
        keys[self.candidate]
    }

    /// What looking it up adds beside the answer.
    pub fn lookup(&self) -> Lookup {
        Lookup { mirrored_at: self.updated_at.clone(), other_host: self.other_host.clone() }
    }

    /// The error for a repository with no commits, or whose default branch's
    /// lookup failed; `None` when it has commits.
    pub fn no_commits(&self) -> Option<Error> {
        if self.branch_failed {
            Some(Error::failed("the repository lookup"))
        } else if self.default_branch.is_some() {
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

/// The variables naming `repo`: `$repo0`, and `$repo1`, the gitlab.com
/// candidate of `owner/name`, which `$gitlab` switches on. A `String!` is
/// sent even where its part is skipped, and `null` for one fails the request
/// with an error that names no path, so `$repo1` is empty for an exact name.
pub fn repository_variables(repo: &Repo) -> Map<String, Value> {
    let names = repo.candidates();
    let mut variables = Map::new();
    variables.insert("repo0".to_owned(), json!(names[0]));
    variables.insert("repo1".to_owned(), json!(names.get(1).map_or("", String::as_str)));
    variables.insert("gitlab".to_owned(), json!(names.len() > 1));
    variables
}

/// Each candidate has its own `repository` alias. `repositories(names:)`
/// would take both in one field, but it sorts its results, drops a missing
/// name, and nulls all of `data` when it fails.
const ROOTS: [&str; 2] = ["r0", "r1"];

/// The response keys of `repo`'s candidates, `r0` and `r1` in the
/// documents, one per candidate in order.
pub fn roots(repo: &Repo) -> &'static [&'static str] {
    &ROOTS[..repo.candidates().len()]
}

/// The revisions of an address that name no commit in `repo`: one
/// revision, a range's base or head, or both ends of a range.
enum Missing<'a> {
    Revision(&'a str),
    Base(&'a str),
    Head(&'a str),
    Both(&'a str, &'a str),
}

/// The error for revisions that name no commit in `found`. When `sego refs`
/// can match one, it states the default branch, then points to the refs
/// the first such one could have meant.
fn missing_revisions(found: &Repository, missing: &Missing<'_>) -> Error {
    let repo = &found.name;
    let one =
        |label: &str, rev: &str| format!("{label} {rev} not found in {repo}{}", hex_reading(rev));
    let (message, revs) = match *missing {
        Missing::Revision(rev) => (one("revision", rev), vec![rev]),
        Missing::Base(rev) => (one("base revision", rev), vec![rev]),
        Missing::Head(rev) => (one("head revision", rev), vec![rev]),
        Missing::Both(base, head) => {
            let reading = match (is_abbreviation(base), is_abbreviation(head)) {
                (false, false) => String::new(),
                (true, true) => {
                    ": no ref has either name and no single commit starts with either".to_owned()
                }
                (true, false) => end_hex_reading(base),
                (false, true) => end_hex_reading(head),
            };
            (format!("revisions {base} and {head} not found in {repo}{reading}"), vec![base, head])
        }
    };
    let error = Error::not_found(message);
    let Some(line) = revs.iter().find_map(|rev| pointer::refs(repo, rev)) else {
        return error;
    };
    let branch =
        found.default_branch.as_ref().map(|branch| format!("the default branch is {branch}"));
    error.facts(branch.into_iter().chain([line]))
}

/// Whether `rev` could abbreviate a commit ID: 4 to 39 hex digits.
fn is_abbreviation(rev: &str) -> bool {
    is_oid_like(rev) && rev.len() < FULL_OID
}

/// What a missing revision that could abbreviate a commit ID rules out,
/// which is true whether it names nothing or more than one commit.
fn hex_reading(rev: &str) -> &'static str {
    if is_abbreviation(rev) {
        ": no ref has that name and no single commit starts with it"
    } else {
        ""
    }
}

/// `hex_reading` for `rev`, the one of two missing ends that could
/// abbreviate a commit ID.
fn end_hex_reading(rev: &str) -> String {
    format!(": no ref is named {rev} and no single commit starts with it")
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
        [] => Err(Error::not_found(format!("{} is not on {}", repo.as_written(), envelope.host))
            .fact(SOURCEGRAPH_COM_MIRRORS)),
        [i] => {
            let other = (0..roots.len()).find(|&j| j != i && failed(j));
            let root = roots[i];
            Ok(Repository {
                name: name_at(i),
                root: root.to_owned(),
                candidate: i,
                default_branch: envelope
                    .get(&[root, "defaultBranch", "displayName"])
                    .and_then(Value::as_str)
                    .map(str::to_owned),
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
        [a, b, ..] => Err(Error::usage_from_response(format!(
            "{} exists as {} and {}",
            repo.as_written(),
            name_at(a),
            name_at(b)
        ))),
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
    Err(found.no_commits().unwrap_or_else(|| missing_revisions(found, &Missing::Revision(rev))))
}

/// Both ends of a range, each looked up once. Two missing names share one
/// fetch, so one may come back after waiting for it, which reads as missing
/// too.
const RANGE_ENDS: &str = "\
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
pub fn range_request(repo: &Repo, base: &str, head: &str) -> Request {
    let mut variables = repository_variables(repo);
    variables.insert("a".to_owned(), json!(base));
    variables.insert("b".to_owned(), json!(head));
    Request::new("Range", RANGE_ENDS.to_owned(), variables)
}

/// Both ends of the range `base` and `head`, from the response to
/// `range_request`.
pub fn read_range(envelope: &Envelope, repo: &Repo, base: &str, head: &str) -> Result<Ends, Error> {
    let found = repository(repo, envelope, roots(repo))?;
    let a = commit(envelope, &found, &[&found.root, "a"], base);
    let b = commit(envelope, &found, &[&found.root, "b"], head);
    let (a, b) = match (a, b) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(e), _) | (_, Err(e)) if e.kind() != Kind::NotFound => return Err(e),
        (Err(_), Err(_)) => {
            return Err(found
                .no_commits()
                .unwrap_or_else(|| missing_revisions(&found, &Missing::Both(base, head))));
        }
        (Err(_), Ok(_)) => return Err(missing_revisions(&found, &Missing::Base(base))),
        (Ok(_), Err(_)) => return Err(missing_revisions(&found, &Missing::Head(head))),
    };
    let end = |oid: String, rev: &str| Resolved::new(&found, oid, Some(rev.to_owned()), None);
    Ok(Ends { base: end(a, base), head: end(b, head) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_hex_revision_says_what_it_rules_out() {
        let found = Repository {
            name: "r".to_owned(),
            root: "r0".to_owned(),
            candidate: 0,
            default_branch: Some("main".to_owned()),
            branch_failed: false,
            updated_at: None,
            other_host: Ok(()),
        };
        let message = |rev: &str| missing_revisions(&found, &Missing::Revision(rev)).to_string();
        assert_eq!(
            message("49c0"),
            "revision 49c0 not found in r: no ref has that name and no single commit starts with it"
        );
        assert_eq!(message("main"), "revision main not found in r");
        assert_eq!(message(&"a".repeat(40)), format!("revision {} not found in r", "a".repeat(40)));
    }
}
