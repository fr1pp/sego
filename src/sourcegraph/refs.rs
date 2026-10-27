use serde::Deserialize;
use serde_json::{Value, json};

use super::Page;
use super::graphql::{Envelope, Request};
use super::resolve::{self, repository_variables};
use crate::address::Repo;
use crate::error::Error;

/// A repository's branches and tags with the object each names, and how many
/// there are; no commit is read, so nothing is fetched.
const DOCUMENT: &str = "\
query Refs($repo0: String!, $repo1: String!, $gitlab: Boolean!, $first: Int, $query: String, \
$unfiltered: Boolean!) {
  r0: repository(name: $repo0) { ...Refs }
  r1: repository(name: $repo1) @include(if: $gitlab) { ...Refs }
}

fragment Refs on Repository {
  name
  defaultBranch { displayName }
  mirrorInfo { updatedAt }
  branches(first: $first, query: $query) { ...Listing }
  tags(first: $first, query: $query) { ...Listing }
  all_branches: branches(first: 1) @include(if: $unfiltered) { totalCount }
  all_tags: tags(first: 1) @include(if: $unfiltered) { totalCount }
}

fragment Listing on GitRefConnection {
  totalCount
  nodes { displayName target { oid } }
}
";

/// A repository's branches and tags.
#[derive(Debug, PartialEq, Eq)]
pub struct Refs {
    /// The repository's name as the server spells it.
    pub repo: String,
    /// When Sourcegraph last updated its mirror, as read with the refs.
    pub mirrored_at: Option<String>,
    pub default_branch: Option<String>,
    pub branches: RefList,
    pub tags: RefList,
}

/// The refs of one kind that match the text, in the server's order.
#[derive(Debug, PartialEq, Eq)]
pub struct RefList {
    pub page: Page<Ref>,
    /// How many there are without the text, when a text was given.
    pub unfiltered: Option<u64>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Ref {
    pub name: String,
    /// The object the ref names: a commit, or for some tags a tree or a blob.
    pub target: Option<String>,
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
}

pub fn read(envelope: &Envelope, repo: &Repo, text: Option<&str>) -> Result<Refs, Error> {
    let roots: &[&str] = if repo.candidates().len() > 1 { &["r0", "r1"] } else { &["r0"] };
    for kind in [Kind::Branch, Kind::Tag] {
        let listed = |root: &&str| {
            envelope.error_beneath(&[root, kind.key()]).is_some()
                || envelope.error_beneath(&[root, kind.unfiltered_key()]).is_some()
        };
        if roots.iter().any(listed) {
            return Err(kind.failed());
        }
    }
    let found = resolve::repository(repo, envelope, roots)?;
    let root = found.root.as_str();
    if envelope.error_beneath(&[root, "defaultBranch"]).is_some() {
        return Err(Error::failed("the repository lookup"));
    }
    let default_branch = envelope
        .get(&[root, "defaultBranch", "displayName"])
        .and_then(Value::as_str)
        .map(str::to_owned);
    let list = |kind: Kind| -> Result<RefList, Error> {
        let listing = envelope
            .get(&[root, kind.key()])
            .and_then(|value| Listing::deserialize(value).ok())
            .ok_or_else(|| kind.failed())?;
        let unfiltered = match text {
            Some(_) => Some(
                envelope
                    .get(&[root, kind.unfiltered_key(), "totalCount"])
                    .and_then(Value::as_u64)
                    .ok_or_else(|| kind.failed())?,
            ),
            None => None,
        };
        let items: Vec<Ref> = listing
            .nodes
            .into_iter()
            .map(|node| Ref { name: node.display_name, target: node.target.and_then(|t| t.oid) })
            .collect();
        let more = (items.len() as u64) < listing.total_count;
        Ok(RefList { page: Page { items, total: Some(listing.total_count), more }, unfiltered })
    };
    Ok(Refs {
        mirrored_at: found.updated_at.clone(),
        branches: list(Kind::Branch)?,
        tags: list(Kind::Tag)?,
        repo: found.name,
        default_branch,
    })
}
