use std::slice;

use serde::Deserialize;
use serde_json::json;

use super::graphql::{Envelope, Request, View};
use super::repository::{locate_repository, repository_lookup_failed, repository_variables};
use crate::address::Repo;
use crate::error::Error;

const PARTS: &[(&str, &str)] = &[
    ("branches", "the branch list"),
    ("all_branches", "the branch list"),
    ("tags", "the tag list"),
    ("all_tags", "the tag list"),
];

const DOCUMENT: &str = "\
query Refs($repo0: String!, $repo1: String!, $gitlab: Boolean!, $first: Int, $query: String, \
$unfiltered: Boolean!) {
  r0: repository(name: $repo0) { ...Refs }
  r1: repository(name: $repo1) @include(if: $gitlab) { ...Refs }
}

fragment Refs on Repository {
  name
  defaultBranch { displayName }
  branches(first: $first, query: $query) { ...Listing }
  tags(first: $first, query: $query) { ...Listing }
  all_branches: branches(first: 1) @include(if: $unfiltered) { totalCount }
  all_tags: tags(first: 1) @include(if: $unfiltered) { totalCount }
}

fragment Listing on GitRefConnection {
  totalCount
  nodes { displayName target { oid commit { committer { date } } } }
}
";

/// A repository's branches and tags.
#[derive(Debug, PartialEq, Eq)]
pub struct Refs {
    /// The repository's name as the server spells it.
    pub repo: String,
    pub default_branch: Option<String>,
    pub branches: RefList,
    pub tags: RefList,
}

/// The refs of one kind that match the text, newest first.
#[derive(Debug, PartialEq, Eq)]
pub struct RefList {
    /// How many match the text.
    pub total: u64,
    /// How many there are without the text, when a text was given.
    pub unfiltered: Option<u64>,
    pub refs: Vec<Ref>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Ref {
    pub name: String,
    /// `None` for a ref that doesn't point at a commit.
    pub commit: Option<String>,
    /// The commit's committer timestamp.
    pub committed: Option<String>,
}

#[derive(Clone, Copy)]
enum Kind {
    Branch,
    Tag,
}

impl Kind {
    fn key(self) -> &'static str {
        match self {
            Kind::Branch => "branches",
            Kind::Tag => "tags",
        }
    }

    fn unfiltered_key(self) -> &'static str {
        match self {
            Kind::Branch => "all_branches",
            Kind::Tag => "all_tags",
        }
    }

    fn failed(self) -> Error {
        match self {
            Kind::Branch => Error::failed("the branch list"),
            Kind::Tag => Error::failed("the tag list"),
        }
    }
}

/// The refs of `repo` that match `text`, at most `first` of each kind, or
/// all of them for `None`.
pub fn request(repo: &Repo, text: Option<&str>, first: Option<u32>) -> Request {
    let mut variables = repository_variables(repo);
    variables.insert("first".to_owned(), json!(first));
    variables.insert("query".to_owned(), json!(text));
    variables.insert("unfiltered".to_owned(), json!(text.is_some()));
    Request::new("Refs", DOCUMENT.to_owned(), variables)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Listing {
    total_count: u64,
    nodes: Vec<Node>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Node {
    display_name: String,
    target: Option<Target>,
}

#[derive(Deserialize)]
struct Target {
    oid: Option<String>,
    commit: Option<Commit>,
}

#[derive(Deserialize)]
struct Commit {
    committer: Option<Committer>,
}

#[derive(Deserialize)]
struct Committer {
    date: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Count {
    total_count: u64,
}

pub fn read(envelope: &Envelope, repo: &Repo, text: Option<&str>) -> Result<Refs, Error> {
    let found = locate_repository(repo, envelope, "r", PARTS)?;
    let at = found.at("r");
    let view = View::new(envelope, slice::from_ref(&at));
    for kind in [Kind::Branch, Kind::Tag] {
        if view.errored_beneath(&[kind.key()]) || view.errored_beneath(&[kind.unfiltered_key()]) {
            return Err(kind.failed());
        }
    }
    if view.errored_beneath(&["defaultBranch"]) {
        return Err(repository_lookup_failed());
    }
    let default_branch = view.read(&["defaultBranch", "displayName"]);
    let mut branches = list(&view, Kind::Branch)?;
    let mut tags = list(&view, Kind::Tag)?;
    if text.is_some() {
        for (list, kind) in [(&mut branches, Kind::Branch), (&mut tags, Kind::Tag)] {
            let count: Count = view.read(&[kind.unfiltered_key()]).ok_or_else(|| kind.failed())?;
            list.unfiltered = Some(count.total_count);
        }
    }
    Ok(Refs { repo: found.repo, default_branch, branches, tags })
}

fn list(view: &View<'_>, kind: Kind) -> Result<RefList, Error> {
    let listing: Listing = view.read(&[kind.key()]).ok_or_else(|| kind.failed())?;
    let mut refs = Vec::with_capacity(listing.nodes.len());
    for node in listing.nodes {
        let (commit, committed) = match node.target {
            Some(Target { oid, commit: Some(commit) }) => {
                (Some(oid.ok_or_else(|| kind.failed())?), commit.committer.and_then(|c| c.date))
            }
            _ => (None, None),
        };
        refs.push(Ref { name: node.display_name, commit, committed });
    }
    Ok(RefList { total: listing.total_count, unfiltered: None, refs })
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;
    use crate::error::report;
    use crate::sourcegraph::parsed;

    const REPO: &str = "example.com/acme/widget";

    fn read_refs(text: Option<&str>, body: &Value) -> Result<Refs, String> {
        read(&parsed(body), &Repo::Exact(REPO.to_owned()), text).map_err(|e| report(&e))
    }

    fn failure(text: Option<&str>, body: &Value) -> String {
        read_refs(text, body).expect_err("the refs fail")
    }

    fn failed(part: &str) -> String {
        format!("sego: error: Sourcegraph failed to answer {part}\n")
    }

    fn node(name: &str, n: usize) -> Value {
        json!({"displayName": name, "target": {"oid": format!("{n:040x}"),
            "commit": {"committer": {"date": "2026-01-02T03:04:05Z"}}}})
    }

    fn list(total: usize, nodes: &[Value]) -> Value {
        json!({"totalCount": total, "nodes": nodes})
    }

    fn root(branches: &Value, tags: &Value) -> Value {
        json!({
            "name": REPO,
            "defaultBranch": {"displayName": "main"},
            "branches": branches,
            "tags": tags,
        })
    }

    fn body(root: &Value) -> Value {
        json!({"data": {"r0": root}})
    }

    #[test]
    fn the_lists_are_read_with_their_totals() {
        let tag = json!({"displayName": "v1", "target": {"oid": "t", "commit": null}});
        let refs = read_refs(None, &body(&root(&list(3, &[node("main", 1)]), &list(1, &[tag]))))
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(refs.repo, REPO);
        assert_eq!(refs.default_branch.as_deref(), Some("main"));
        assert_eq!(refs.branches.total, 3);
        assert_eq!(refs.branches.unfiltered, None);
        assert_eq!(
            refs.branches.refs,
            [Ref {
                name: "main".to_owned(),
                commit: Some(format!("{:040x}", 1)),
                committed: Some("2026-01-02T03:04:05Z".to_owned()),
            }]
        );
        assert_eq!(refs.tags.refs, [Ref { name: "v1".to_owned(), commit: None, committed: None }]);
    }

    #[test]
    fn a_text_reads_the_unfiltered_totals() {
        let mut filtered = root(&list(1, &[node("main", 1)]), &list(0, &[]));
        filtered["all_branches"] = json!({"totalCount": 7});
        filtered["all_tags"] = json!({"totalCount": 9});
        let refs = read_refs(Some("ma"), &body(&filtered)).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!((refs.branches.unfiltered, refs.tags.unfiltered), (Some(7), Some(9)));
    }

    #[test]
    fn a_located_error_in_any_list_names_that_list() {
        for (part, text, message) in [
            ("tags", None, "the tag list"),
            ("branches", None, "the branch list"),
            ("all_tags", Some("x"), "the tag list"),
            ("all_branches", Some("x"), "the branch list"),
        ] {
            let located = json!({"errors": [{
                "message": "panic occurred: runtime error: slice bounds out of range",
                "path": ["r0", part, "nodes"]
            }], "data": {"r0": null}});
            assert_eq!(failure(text, &located), failed(message), "{part}");
        }
    }

    #[test]
    fn a_list_that_errors_beside_data_fails_that_list() {
        let mut tags_gone = body(&root(&list(1, &[node("main", 1)]), &Value::Null));
        tags_gone["errors"] = json!([{"message": "boom", "path": ["r0", "tags"]}]);
        assert_eq!(failure(None, &tags_gone), failed("the tag list"));

        let mut branches_gone = body(&root(&Value::Null, &list(0, &[])));
        branches_gone["errors"] = json!([{"message": "boom", "path": ["r0", "branches", "nodes"]}]);
        assert_eq!(failure(None, &branches_gone), failed("the branch list"));
    }

    #[test]
    fn a_failed_default_branch_fails_the_repository_lookup() {
        let mut gone = body(&root(&list(0, &[]), &list(0, &[])));
        gone["data"]["r0"]["defaultBranch"] = Value::Null;
        gone["errors"] = json!([{"message": "boom", "path": ["r0", "defaultBranch"]}]);
        assert_eq!(failure(None, &gone), failed("the repository lookup"));
    }

    #[test]
    fn a_repository_with_null_lists_and_no_error_fails_the_branch_list() {
        assert_eq!(
            failure(None, &body(&root(&Value::Null, &Value::Null))),
            failed("the branch list")
        );
    }

    #[test]
    fn a_text_without_the_unfiltered_totals_fails_the_branch_list() {
        let unfiltered = body(&root(&list(0, &[]), &list(0, &[])));
        assert_eq!(failure(Some("x"), &unfiltered), failed("the branch list"));
    }

    #[test]
    fn a_commit_without_its_id_fails_its_list() {
        let bad = json!({"displayName": "v1", "target": {"oid": null, "commit": {}}});
        assert_eq!(
            failure(None, &body(&root(&list(0, &[]), &list(1, &[bad])))),
            failed("the tag list")
        );
    }
}
