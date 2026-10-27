use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::graphql::{Envelope, Request, View, repeated};
use super::repository::{Located, ambiguous, candidates, locate, repository_variables};
use crate::address::{Repo, Target, pinned};
use crate::error::Error;

const LIMIT_MESSAGE: &str = "file exceeded size limit";
/// How many lines an open-ended range asks for, at most.
const OPEN_END_SPAN: i64 = 500_000;

/// A file at each revision and path the address may name, `#` numbering
/// them; `$split` asks whether each path exists and `$ambiguous#` whether its
/// revision names a ref, to tell the parts of a web URL apart.
const FILE: &str = "\
  c#: commit(rev: $rev#) {
    oid
    meta: blob(path: $path#) { byteSize totalLines binary rawURL lfs { byteSize } }
    body: blob(path: $path#) { content(startLine: $start, endLine: $end, textOnly: true) }
    dir: tree(path: $path#) { entries { path } }
    parent: tree(path: $parent#) { entries { path submodule { url commit } } }
    x#: path(path: $path#) @include(if: $split) { __typename }
  }
  g#: gitRefs(query: $rev#, first: 5) @include(if: $ambiguous#) { nodes { displayName } }";

/// A directory at each revision and path the address may name, as `FILE`.
const TREE: &str = "\
  c#: commit(rev: $rev#) {
    oid
    tree: tree(path: $path#) { entries(recursive: $recursive) { path isDirectory submodule { url commit } } }
    file: blob(path: $path#) { totalLines byteSize binary }
    parent: tree(path: $parent#) { entries { path submodule { url commit } } }
    x#: path(path: $path#) @include(if: $split) { __typename }
  }
  g#: gitRefs(query: $rev#, first: 5) @include(if: $ambiguous#) { nodes { displayName } }";

const GUESSED: &str = "$rev#: String!, $path#: String!, $parent#: String!, $ambiguous#: Boolean!";

/// An address resolved to a commit.
#[derive(Debug, PartialEq, Eq)]
pub struct Resolved {
    /// The repository's name as the server spells it.
    pub repo: String,
    pub commit: String,
    /// The revision as asked; `None` for the default branch.
    pub rev: Option<String>,
    pub path: Option<String>,
}

/// What a path names where a file was asked for.
#[derive(Debug, PartialEq, Eq)]
pub enum File {
    Blob(Blob),
    Submodule(Submodule),
    /// A directory, with how many entries it holds.
    Directory {
        entries: usize,
    },
}

#[derive(Debug, PartialEq, Eq)]
pub struct Blob {
    pub details: Details,
    /// The size of the object an LFS pointer names.
    pub lfs_size: Option<u64>,
    /// `None` when there are no lines to read: the file is binary, or has
    /// fewer lines than the first asked for.
    pub content: Option<Content>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Details {
    pub byte_size: u64,
    pub binary: bool,
    /// The file's lines; 0 for an empty file.
    pub total_lines: u32,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Content {
    /// The text of the lines asked for.
    Lines(String),
    /// The file is over the content limit, so its lines are read whole
    /// through the raw route at this site path.
    OverLimit { raw: String },
}

/// What a path names where a directory was asked for.
#[derive(Debug, PartialEq, Eq)]
pub enum Tree {
    /// Its entries, in the server's order.
    Entries(Vec<Entry>),
    Submodule(Submodule),
    /// A file.
    File(Details),
}

#[derive(Debug, PartialEq, Eq)]
pub struct Entry {
    pub path: String,
    pub is_directory: bool,
    pub submodule: Option<Submodule>,
}

/// A submodule as its parent stores it. Either field is empty when the
/// server left it out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Submodule {
    pub url: String,
    pub commit: String,
}

fn parent_of(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(parent, _)| parent)
}

/// The file `target` names in `repo`, with lines `start` to `end`, or to the
/// last for `None`.
pub fn file_request(repo: &Repo, target: &Target, start: u32, end: Option<u32>) -> Request {
    let start = i64::from(start);
    let end = end.map_or_else(|| (start + OPEN_END_SPAN).min(i64::from(i32::MAX)), i64::from);
    let shared = [("start", json!(start)), ("end", json!(end))];
    let (variables, guesses) = guessed(repo, target, &shared);
    let document = document("ShowFile", "$start: Int!, $end: Int!", FILE, guesses);
    Request::new("ShowFile", document, variables)
}

/// The directory `target` names in `repo`, with every entry beneath it when
/// `recursive`.
pub fn tree_request(repo: &Repo, target: &Target, recursive: bool) -> Request {
    let (variables, guesses) = guessed(repo, target, &[("recursive", json!(recursive))]);
    let document = document("ListTree", "$recursive: Boolean!", TREE, guesses);
    Request::new("ListTree", document, variables)
}

/// The variables of a file or directory question: the repository's, each
/// revision and path `target` may name, and the `shared` ones the guesses
/// use; and how many guesses there are. With none, nothing uses the shared
/// variables, and GraphQL refuses a variable nothing uses.
fn guessed(repo: &Repo, target: &Target, shared: &[(&str, Value)]) -> (Map<String, Value>, usize) {
    let split = matches!(target, Target::Split(_));
    let mut variables = repository_variables(repo);
    let guesses = candidates(target);
    if !guesses.is_empty() {
        variables.insert("split".to_owned(), json!(split));
        variables.extend(shared.iter().map(|(name, value)| ((*name).to_owned(), value.clone())));
    }
    for (n, guess) in guesses.iter().enumerate() {
        let path = guess.path.clone().unwrap_or_default();
        variables.insert(format!("rev{n}"), json!(guess.rev));
        variables.insert(format!("parent{n}"), json!(parent_of(&path)));
        variables.insert(format!("path{n}"), json!(path));
        variables.insert(format!("ambiguous{n}"), json!(split && ambiguous(&guess.rev)));
    }
    (variables, guesses.len())
}

/// The document of `operation`, declaring `declared` besides the
/// repository's and the guesses' variables, asking `each` for each guess.
fn document(operation: &str, declared: &str, each: &str, guesses: usize) -> String {
    let declared = if guesses == 0 {
        String::new()
    } else {
        format!(", $split: Boolean!, {declared}\n{}", repeated(GUESSED, guesses))
    };
    format!(
        "query {operation}($repo0: String!, $repo1: String!, $gitlab: Boolean!{declared}) {{
  r0: repository(name: $repo0) {{ ...Guesses }}
  r1: repository(name: $repo1) @include(if: $gitlab) {{ ...Guesses }}
}}

fragment Guesses on Repository {{
  name
  defaultBranch {{ displayName }}
{}
}}
",
        repeated(each, guesses)
    )
}

#[derive(Deserialize)]
struct Listing {
    entries: Vec<Listed>,
}

#[derive(Deserialize)]
struct Listed {
    path: String,
    submodule: Option<SubmoduleNode>,
}

/// A submodule's fields as the parent's tree gives them.
#[derive(Deserialize)]
struct SubmoduleNode {
    url: Option<String>,
    commit: Option<String>,
}

impl SubmoduleNode {
    fn submodule(self) -> Submodule {
        Submodule { url: self.url.unwrap_or_default(), commit: self.commit.unwrap_or_default() }
    }
}

/// The submodule the parent's tree lists at `path`.
fn submodule_at(view: &View<'_>, path: &str) -> Option<Submodule> {
    let parent: Listing = view.read(&["parent"])?;
    parent
        .entries
        .into_iter()
        .find(|entry| entry.path == path)?
        .submodule
        .map(SubmoduleNode::submodule)
}

fn resolved(located: Located) -> Resolved {
    Resolved { repo: located.repo, commit: located.commit, rev: located.rev, path: located.path }
}

fn not_found(located: &Located, path: &str) -> Error {
    Error::missing(format!("{path} not found in {}", pinned(&located.repo, &located.commit, None)))
}

fn limit_exceeded(view: &View<'_>, tail: &[&str]) -> bool {
    view.error(tail).is_some_and(|error| error.message.contains(LIMIT_MESSAGE))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Meta {
    byte_size: u64,
    total_lines: u64,
    binary: bool,
    #[serde(rename = "rawURL")]
    raw_url: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Lfs {
    byte_size: String,
}

#[derive(Deserialize)]
struct Entries {
    entries: Vec<serde_json::Value>,
}

/// The file at `target`, reading its lines only from `start` on.
pub fn read_file(
    envelope: &Envelope,
    repo: &Repo,
    target: &Target,
    start: u32,
) -> Result<(Resolved, File), Error> {
    let located = locate(repo, target, envelope)?;
    let view = View::new(envelope, &located.at);
    let path = located.path.clone().unwrap_or_default();
    if let Some(submodule) = submodule_at(&view, &path) {
        return Ok((resolved(located), File::Submodule(submodule)));
    }
    let details_failed = || Error::failed("the file's details");
    let (meta, body, dir) = (view.get(&["meta"]), view.get(&["body"]), view.get(&["dir"]));
    let (meta_failed, body_failed) = (view.errored(&["meta"]), view.errored(&["body"]));
    if meta_failed && body_failed && dir.is_some() {
        let entries = view.read::<Entries>(&["dir"]).map_or(0, |dir| dir.entries.len());
        return Ok((resolved(located), File::Directory { entries }));
    }
    let silent = !(meta_failed || body_failed || view.errored(&["dir"]));
    if silent && meta.is_none() && body.is_none() && dir.is_none() {
        return Err(not_found(&located, &path));
    }
    let submodule_open = meta_failed || body_failed || meta.is_none() || body.is_none();
    if submodule_open && view.errored(&["parent"]) {
        return Err(Error::failed("the directory listing"));
    }
    let meta: Meta = view.read(&["meta"]).ok_or_else(details_failed)?;
    if view.errored(&["meta", "lfs"]) && !limit_exceeded(&view, &["meta", "lfs"]) {
        return Err(details_failed());
    }
    let lfs_size = match view.get(&["meta", "lfs"]) {
        Some(_) => {
            let lfs: Lfs = view.read(&["meta", "lfs"]).ok_or_else(details_failed)?;
            Some(lfs.byte_size.parse::<u64>().map_err(|_| details_failed())?)
        }
        None => None,
    };
    let total_lines = if meta.byte_size == 0 {
        0
    } else {
        u32::try_from(meta.total_lines).map_err(|_| details_failed())?
    };
    let details = Details { byte_size: meta.byte_size, binary: meta.binary, total_lines };
    let content = if details.binary || start > total_lines {
        None
    } else if limit_exceeded(&view, &["body", "content"]) {
        let raw = meta
            .raw_url
            .filter(|raw| raw.starts_with('/') && !raw.starts_with("//"))
            .ok_or_else(details_failed)?;
        Some(Content::OverLimit { raw })
    } else {
        let text: String =
            view.read(&["body", "content"]).ok_or_else(|| Error::failed("the file content"))?;
        Some(Content::Lines(text))
    };
    Ok((resolved(located), File::Blob(Blob { details, lfs_size, content })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TreeListing {
    entries: Vec<TreeNode>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TreeNode {
    path: String,
    is_directory: bool,
    submodule: Option<SubmoduleNode>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FileNode {
    byte_size: u64,
    binary: bool,
    total_lines: u64,
}

/// The directory at `target`.
pub fn read_tree(
    envelope: &Envelope,
    repo: &Repo,
    target: &Target,
) -> Result<(Resolved, Tree), Error> {
    let located = locate(repo, target, envelope)?;
    let view = View::new(envelope, &located.at);
    let listing_failed = || Error::failed("the directory listing");
    let path = located.path.clone().filter(|path| !path.is_empty());
    if let Some(path) = &path
        && let Some(submodule) = submodule_at(&view, path)
    {
        return Ok((resolved(located), Tree::Submodule(submodule)));
    }
    let tree_failed = view.errored_beneath(&["tree"]);
    if view.get(&["tree"]).is_some() {
        if tree_failed {
            return Err(listing_failed());
        }
        let tree: TreeListing = view.read(&["tree"]).ok_or_else(listing_failed)?;
        let entries = tree
            .entries
            .into_iter()
            .map(|node| Entry {
                path: node.path,
                is_directory: node.is_directory,
                submodule: node.submodule.map(SubmoduleNode::submodule),
            })
            .collect();
        return Ok((resolved(located), Tree::Entries(entries)));
    }
    let Some(path) = path else {
        return Err(listing_failed());
    };
    let file_failed = view.errored_beneath(&["file"]);
    match view.get(&["file"]) {
        Some(_) if tree_failed && !file_failed => {
            let file: FileNode = view.read(&["file"]).ok_or_else(listing_failed)?;
            let total_lines = if file.byte_size == 0 {
                0
            } else {
                u32::try_from(file.total_lines).map_err(|_| listing_failed())?
            };
            let details = Details { byte_size: file.byte_size, binary: file.binary, total_lines };
            Ok((resolved(located), Tree::File(details)))
        }
        None if !tree_failed && !file_failed => Err(not_found(&located, &path)),
        _ => Err(listing_failed()),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;
    use crate::error::report;
    use crate::sourcegraph::parsed;

    const SHA: &str = "d81fd54f1a2b3c4d5e6f708192a3b4c5d6e7f809";
    const REPO: &str = "example.com/acme/widget";
    const SECRET: &str = "rpc error: code = Internal desc = git command failed";

    fn errors(list: &[(&[&str], &str)]) -> Value {
        let list: Vec<_> =
            list.iter().map(|(path, message)| json!({"message": message, "path": path})).collect();
        json!(list)
    }

    fn file() -> Value {
        json!({"data": {"r0": {
            "name": "example.com/x/y",
            "defaultBranch": {"displayName": "main"},
            "c0": {
                "oid": SHA,
                "meta": {
                    "byteSize": 6, "totalLines": 3, "binary": false,
                    "rawURL": format!("/r/example.com/x/y@{SHA}/-/raw/a.go"),
                    "lfs": null
                },
                "body": {"content": "a\nb\nc\n"},
                "dir": null,
                "parent": {"entries": [{"path": "a.go", "submodule": null}]}
            }
        }}, "errors": [{"message": "not a directory", "path": ["r0", "c0", "dir"]}]})
    }

    fn over_the_content_limit() -> Value {
        let mut body = file();
        let c0 = &mut body["data"]["r0"]["c0"];
        c0["meta"]["byteSize"] = json!(2_000_000);
        c0["body"] = json!(null);
        body["errors"] = errors(&[
            (&["r0", "c0", "dir"], "not a directory"),
            (&["r0", "c0", "body", "content"], "file exceeded size limit"),
            (&["r0", "c0", "meta", "lfs"], "file exceeded size limit"),
        ]);
        body
    }

    fn at(path: &str) -> Target {
        Target::At { rev: Some("main".to_owned()), path: Some(path.to_owned()) }
    }

    fn read(body: &Value, start: u32) -> Result<File, String> {
        let repo = Repo::Exact("example.com/x/y".to_owned());
        read_file(&parsed(body), &repo, &at("a.go"), start)
            .map(|(_, file)| file)
            .map_err(|e| report(&e))
    }

    fn failed(part: &str) -> String {
        format!("sego: error: Sourcegraph failed to answer {part}\n")
    }

    #[test]
    fn a_file_is_read_with_its_lines() {
        let File::Blob(blob) = read(&file(), 1).unwrap_or_else(|e| panic!("{e}")) else {
            panic!("not a blob")
        };
        assert_eq!(blob.details, Details { byte_size: 6, binary: false, total_lines: 3 });
        assert_eq!(blob.content, Some(Content::Lines("a\nb\nc\n".to_owned())));
        let File::Blob(past) = read(&file(), 4).unwrap_or_else(|e| panic!("{e}")) else {
            panic!("not a blob")
        };
        assert_eq!(past.content, None);
    }

    #[test]
    fn a_file_over_the_content_limit_is_read_through_its_raw_url() {
        let File::Blob(blob) = read(&over_the_content_limit(), 1).unwrap_or_else(|e| panic!("{e}"))
        else {
            panic!("not a blob")
        };
        let raw = format!("/r/example.com/x/y@{SHA}/-/raw/a.go");
        assert_eq!(blob.content, Some(Content::OverLimit { raw }));
        assert_eq!(blob.lfs_size, None);
    }

    #[test]
    fn errors_name_the_part_and_hide_the_server_text() {
        let secret = "git exec: /srv/secret-host failed";
        let mut body = file();
        body["data"]["r0"]["c0"]["body"] = json!(null);
        body["errors"] = errors(&[(&["r0", "c0", "body"], secret)]);

        let mut meta = file();
        meta["data"]["r0"]["c0"]["meta"] = json!(null);
        meta["errors"] = errors(&[(&["r0", "c0", "meta"], secret)]);

        let mut parent = file();
        let c0 = &mut parent["data"]["r0"]["c0"];
        c0["meta"] = json!(null);
        c0["parent"] = json!(null);
        parent["errors"] =
            errors(&[(&["r0", "c0", "meta"], secret), (&["r0", "c0", "parent"], secret)]);

        for (body, part) in [
            (body, "the file content"),
            (meta, "the file's details"),
            (parent, "the directory listing"),
        ] {
            assert_eq!(read(&body, 1).err(), Some(failed(part)));
        }
    }

    #[test]
    fn only_the_content_limit_error_at_meta_lfs_means_not_lfs() {
        let mut body = file();
        body["data"]["r0"]["c0"]["meta"]["lfs"] = json!(null);
        body["errors"] = errors(&[(&["r0", "c0", "meta", "lfs"], "lfs backend exploded")]);
        assert_eq!(read(&body, 1).err(), Some(failed("the file's details")));
    }

    #[test]
    fn a_raw_url_that_is_not_a_site_path_is_not_read() {
        for raw_url in ["@evil.example/x", "//evil.example/x", "https://evil.example/x", ""] {
            let mut body = over_the_content_limit();
            body["data"]["r0"]["c0"]["meta"]["rawURL"] = json!(raw_url);
            assert_eq!(read(&body, 1).err(), Some(failed("the file's details")), "{raw_url}");
        }
    }

    #[test]
    fn a_directory_where_a_file_was_asked_for_is_its_entry_count() {
        let mut body = file();
        let c0 = &mut body["data"]["r0"]["c0"];
        c0["meta"] = json!(null);
        c0["body"] = json!(null);
        c0["dir"] = json!({"entries": [{"path": "a"}, {"path": "b"}]});
        body["errors"] =
            errors(&[(&["r0", "c0", "meta"], "not a blob"), (&["r0", "c0", "body"], "not a blob")]);
        assert_eq!(read(&body, 1), Ok(File::Directory { entries: 2 }));
    }

    fn entry(path: &str, is_dir: bool) -> Value {
        json!({"path": path, "isDirectory": is_dir, "submodule": null})
    }

    fn tree_body(mut commit: Value) -> Value {
        commit["oid"] = json!(SHA);
        json!({"data": {"r0": {
            "name": REPO,
            "defaultBranch": {"displayName": "main"},
            "c0": commit,
        }}})
    }

    fn tree(body: &Value, path: Option<&str>) -> Result<Tree, String> {
        let target = Target::At { rev: Some("v1".to_owned()), path: path.map(str::to_owned) };
        read_tree(&parsed(body), &Repo::Exact(REPO.to_owned()), &target)
            .map(|(_, tree)| tree)
            .map_err(|e| report(&e))
    }

    #[test]
    fn a_tree_is_read_in_the_servers_order_with_its_submodules() {
        let module = json!({"path": "lib", "isDirectory": false,
            "submodule": {"url": "../other", "commit": null}});
        let body = tree_body(json!({
            "tree": {"entries": [entry("b", false), entry("a", true), module]},
            "file": null,
            "parent": null,
        }));
        let Tree::Entries(entries) = tree(&body, None).unwrap_or_else(|e| panic!("{e}")) else {
            panic!("not entries")
        };
        let paths: Vec<&str> = entries.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(paths, ["b", "a", "lib"]);
        assert_eq!(
            entries[2].submodule,
            Some(Submodule { url: "../other".to_owned(), commit: String::new() })
        );
    }

    #[test]
    fn an_error_beneath_the_tree_fails_the_listing() {
        let error_at = |part: &str| json!({"message": SECRET, "path": ["r0", "c0", part]});
        let parent = json!({"entries": [entry("broken", false)]});

        let mut tree_only = tree_body(json!({"tree": null, "file": null, "parent": parent}));
        tree_only["errors"] = json!([error_at("tree")]);
        let mut file_and_tree = tree_only.clone();
        file_and_tree["errors"] = json!([error_at("file"), error_at("tree")]);
        let mut all_three = tree_only.clone();
        all_three["errors"] = json!([error_at("file"), error_at("tree"), error_at("parent")]);
        for body in [tree_only, file_and_tree, all_three] {
            assert_eq!(tree(&body, Some("broken")).err(), Some(failed("the directory listing")));
        }

        let mut at_the_root = tree_body(json!({"tree": null, "file": null, "parent": null}));
        at_the_root["errors"] = json!([error_at("tree")]);
        assert_eq!(tree(&at_the_root, None).err(), Some(failed("the directory listing")));
    }

    #[test]
    fn a_file_where_a_tree_was_asked_for_is_its_details() {
        let mut body = tree_body(json!({
            "tree": null,
            "file": {"totalLines": 1, "byteSize": 0, "binary": false},
            "parent": null,
        }));
        body["errors"] = json!([{"message": "not a directory", "path": ["r0", "c0", "tree"]}]);
        assert_eq!(
            tree(&body, Some("empty.txt")),
            Ok(Tree::File(Details { byte_size: 0, binary: false, total_lines: 0 }))
        );
    }

    #[test]
    fn a_split_address_asks_each_guess_and_looks_up_the_ambiguous_ones() {
        let segments = ["abcd1234", "y", "z"].map(str::to_owned).to_vec();
        let request =
            file_request(&Repo::Exact(REPO.to_owned()), &Target::Split(segments), 1, None);
        let variables = &request.variables;
        assert_eq!(variables["split"], true);
        assert_eq!((&variables["rev1"], &variables["path1"]), (&json!("abcd1234/y"), &json!("z")));
        assert_eq!(
            (&variables["ambiguous0"], &variables["ambiguous1"]),
            (&json!(true), &json!(false))
        );
        assert!(request.document.contains("c2: commit(rev: $rev2)"), "{}", request.document);
        assert!(!request.document.contains("c3"), "{}", request.document);
    }

    #[test]
    fn an_address_with_no_valid_guess_declares_nothing_it_doesnt_use() {
        let target = Target::Split(vec!["-x".to_owned()]);
        let request = tree_request(&Repo::Exact(REPO.to_owned()), &target, false);
        assert!(!request.document.contains("$split"), "{}", request.document);
        assert!(!request.variables.contains_key("recursive"), "{:?}", request.variables);
    }

    #[test]
    fn owner_name_asks_gitlab_com_too() {
        let both =
            file_request(&Repo::OwnerName("acme/widget".to_owned()), &at("a.go"), 1, Some(2));
        assert_eq!(both.variables["repo1"], "gitlab.com/acme/widget");
        assert_eq!(both.variables["gitlab"], true);
        let one = file_request(&Repo::Exact(REPO.to_owned()), &at("a.go"), 1, Some(2));
        assert_eq!(
            (&one.variables["repo1"], &one.variables["gitlab"]),
            (&json!(""), &json!(false))
        );
    }
}
