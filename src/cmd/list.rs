use serde::Serialize;

use super::{Help, Present};
use crate::address::{Address, Pinned, RepoPath};
use crate::args::{Cli, UsageError};
use crate::diagnostic::{Diagnostic, Outcome};
use crate::domain::{Entry, EntryKind, Listing, Submodule, Tree};
use crate::escape::shell_word;
use crate::render::json::{Partial, document};
use crate::render::text::{address_line, hint, notice};
use crate::sourcegraph::Fetch;
use crate::sourcegraph::envelope::decode;
use crate::sourcegraph::tree::{self, LIST_ENTRY_CAP, ListRequest};

pub const HELP: Help = Help {
    about: "\
sego list prints the entries of a directory at a revision.
",
    synopsis: "[--json] [--depth N] <repo>[@<rev>] [<path>]",
    details: "\
Operands:
  <repo>    a repository, such as github.com/gorilla/mux; a URL, a trailing
            .git and a trailing slash are accepted. Matching is
            case-insensitive.
  <rev>     a branch, tag or commit-ish, such as main, v1.7.0 or
            a7962380~1; it cannot contain \"..\". Without it, the default
            branch.
  <path>    the directory's path from the repository root, with or without
            a trailing /. Without it, the root.

Options:
  --depth N   entries down to N levels below the directory, at least 1
              (default 1)

Output:
  An address line: the repository at the commit the revision resolved to,
  the directory unless it is the root, then the number of entries. Then
  one entry per line, in path order, each a path from the repository root.
  A directory ends in /; a submodule is followed by the repository and
  commit it pins, or by its URL when that names no repository:

    github.com/rust-lang/rust@051478957371 src/tools/ • 47 entries
    src/tools/build_helper/
    src/tools/bump-stage0/
    src/tools/cargo • submodule github.com/rust-lang/cargo@37629051518c

  The upstream stops a listing at 1000 entries, which may leave out
  entries from anywhere in it. Such a listing ends with a notice: line,
  and with --depth above 1 a hint: line for one level less.

  A submodule's path answers with its address line, then, on a line of its
  own, the repository and commit it pins, or its URL.

  With --json, one document of kind \"listing\": repo, commit (40 hex
  digits), path, entries [{path, type}] with type directory, file or
  submodule, and partial {reason, hints} when the listing stopped at 1000
  entries. A submodule entry has submodule {url, repo, commit}, without
  repo and commit when its URL names no repository. A submodule's path
  gives a document of kind \"submodule\": repo, commit, path and
  submodule. Anything else is reported on stderr, and its document, of
  kind \"unavailable\" or \"error\", also goes to stdout.

Exit codes:
  0  answered      the entries, or the submodule's address
  1  answered no   no such repository, revision or path; a file
  2  invalid       bad operands, such as --depth 0 or a revision
                   containing \"..\"; an invalid SEGO_ENDPOINT
  3  unanswered    no usable answer from the upstream; retry

Examples:
  sego list github.com/gorilla/mux
  sego list github.com/gorilla/mux@v1.7.0
  sego list github.com/gorilla/mux@v1.7.0 .github/
  sego list --depth 2 github.com/gorilla/mux@v1.7.0
",
};

const OPERANDS: &str = "expected <repo>[@<rev>] [<path>]";

/// The listing `cli` asks for: `<repo>[@<rev>] [<path>]`, to `--depth`,
/// which defaults to 1.
pub fn request(cli: &Cli) -> Result<ListRequest, UsageError> {
    let invalid =
        |message: String| UsageError::new(message, cli.command.as_deref());
    let (repo, path) = match cli.operands.as_slice() {
        [repo] => (repo, ""),
        [repo, path] => (repo, path.as_str()),
        [] => {
            let message = format!("missing operands; {OPERANDS}");
            return Err(invalid(message));
        }
        [..] => {
            let message = format!("too many operands; {OPERANDS}");
            return Err(invalid(message));
        }
    };
    let address =
        Address::parse(repo).map_err(|error| invalid(error.to_string()))?;
    let path = RepoPath::parse(path);
    Ok(ListRequest { address, path, depth: cli.depth.unwrap_or(1) })
}

pub fn run(
    cli: &Cli,
    fetch: Fetch,
) -> Result<Box<dyn Present>, Box<dyn Diagnostic>> {
    let request = request(cli)?;
    let reply = fetch(&tree::plan(&request))?;
    let tree = tree::answer(&request, decode(reply)?)?;
    Ok(Box::new(List { tree, depth: request.depth }))
}

/// A listing or a submodule, and the depth it was asked for.
pub struct List {
    pub tree: Tree,
    pub depth: u32,
}

/// `path` as a directory operand: with a trailing `/`, or none for the
/// root.
fn directory(path: &str) -> Option<String> {
    (!path.is_empty()).then(|| format!("{path}/"))
}

/// The notice and the hints for a listing the upstream stopped.
fn partial(listing: &Listing, depth: u32) -> Option<(String, Vec<String>)> {
    if !listing.truncated {
        return None;
    }
    let reason = format!(
        "listing stopped at {LIST_ENTRY_CAP} entries; some entries are not \
         shown"
    );
    if depth <= 1 {
        return Some((reason, Vec::new()));
    }
    let mut operands = vec![listing.at.to_string()];
    operands.extend(directory(&listing.path));
    let command = Cli {
        command: Some("list".to_owned()),
        depth: Some(depth - 1),
        operands,
        ..Cli::default()
    };
    Some((reason, vec![command.command_line()]))
}

/// `n` entries, singular when it is 1.
fn entries(n: usize) -> String {
    match n {
        1 => "1 entry".to_owned(),
        n => format!("{n} entries"),
    }
}

/// Where a submodule points: its repository and commit as a shell word, or
/// its URL when that names no repository.
fn target(url: &str, target: Option<&Pinned>) -> String {
    match target {
        Some(target) => target.word(),
        None => shell_word(url).into_owned(),
    }
}

fn entry_line(Entry { path, kind }: &Entry) -> String {
    match kind {
        EntryKind::Directory => address_line(&[&format!("{path}/")], &[]),
        EntryKind::File => address_line(&[path], &[]),
        EntryKind::Submodule { url, target: pinned } => {
            let meta = format!("submodule {}", target(url, pinned.as_ref()));
            address_line(&[path], &[meta])
        }
    }
}

fn listing_text(listing: &Listing, depth: u32) -> String {
    let address = listing.at.to_string();
    let directory = directory(&listing.path);
    let mut words = vec![address.as_str()];
    words.extend(directory.as_deref());
    let mut text = address_line(&words, &[entries(listing.entries.len())]);
    for entry in &listing.entries {
        text.push_str(&entry_line(entry));
    }
    if let Some((reason, hints)) = partial(listing, depth) {
        text.push_str(&notice(&reason));
        for command in hints {
            text.push_str(&hint(&command));
        }
    }
    text
}

fn submodule_text(submodule: &Submodule) -> String {
    let Submodule { at, path, url, target: pinned } = submodule;
    let mut text =
        address_line(&[&at.to_string(), path], &["submodule".to_owned()]);
    text.push_str(&target(url, pinned.as_ref()));
    text.push('\n');
    text
}

impl Present for List {
    fn outcome(&self) -> Outcome {
        Outcome::Answered
    }

    fn text(&self) -> String {
        match &self.tree {
            Tree::Listing(listing) => listing_text(listing, self.depth),
            Tree::Submodule(submodule) => submodule_text(submodule),
        }
    }

    fn json(&self) -> String {
        match &self.tree {
            Tree::Listing(listing) => {
                let body = ListingJson {
                    repo: listing.at.repo.as_str(),
                    commit: &listing.at.commit.0,
                    path: &listing.path,
                    entries: listing
                        .entries
                        .iter()
                        .map(EntryJson::new)
                        .collect(),
                    partial: partial(listing, self.depth)
                        .map(|(reason, hints)| Partial { reason, hints }),
                };
                document("listing", &body)
            }
            Tree::Submodule(Submodule { at, path, url, target }) => {
                let body = SubmoduleAnswerJson {
                    repo: at.repo.as_str(),
                    commit: &at.commit.0,
                    path,
                    submodule: SubmoduleJson::new(url, target.as_ref()),
                };
                document("submodule", &body)
            }
        }
    }
}

#[derive(Serialize)]
struct ListingJson<'a> {
    repo: &'a str,
    commit: &'a str,
    path: &'a str,
    entries: Vec<EntryJson<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    partial: Option<Partial>,
}

#[derive(Serialize)]
struct EntryJson<'a> {
    path: &'a str,
    #[serde(rename = "type")]
    kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    submodule: Option<SubmoduleJson<'a>>,
}

impl<'a> EntryJson<'a> {
    fn new(Entry { path, kind }: &'a Entry) -> EntryJson<'a> {
        let (kind, submodule) = match kind {
            EntryKind::Directory => ("directory", None),
            EntryKind::File => ("file", None),
            EntryKind::Submodule { url, target } => {
                ("submodule", Some(SubmoduleJson::new(url, target.as_ref())))
            }
        };
        EntryJson { path, kind, submodule }
    }
}

/// A submodule's URL, and the repository and commit it pins when the URL
/// names a repository.
#[derive(Serialize)]
struct SubmoduleJson<'a> {
    url: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    repo: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    commit: Option<&'a str>,
}

impl<'a> SubmoduleJson<'a> {
    fn new(url: &'a str, target: Option<&'a Pinned>) -> SubmoduleJson<'a> {
        SubmoduleJson {
            url,
            repo: target.map(|target| target.repo.as_str()),
            commit: target.map(|target| target.commit.0.as_str()),
        }
    }
}

#[derive(Serialize)]
struct SubmoduleAnswerJson<'a> {
    repo: &'a str,
    commit: &'a str,
    path: &'a str,
    submodule: SubmoduleJson<'a>,
}

#[cfg(test)]
mod tests {
    use expect_test::expect;

    use super::*;
    use crate::address::{CommitId, RepoName};

    fn cli(operands: &[&str], depth: Option<u32>) -> Cli {
        Cli {
            command: Some("list".to_owned()),
            operands: operands.iter().map(|&s| s.to_owned()).collect(),
            depth,
            ..Cli::default()
        }
    }

    fn at() -> Pinned {
        Pinned {
            repo: RepoName::parse("github.com/x/a").unwrap(),
            commit: CommitId(
                "a7962380ca08b5a188038c69871b8d3fbdf31e89".to_owned(),
            ),
        }
    }

    #[test]
    fn operands_become_a_request() {
        let listed = |operands: &[&str], depth| {
            let request = request(&cli(operands, depth)).unwrap();
            (request.path.upstream().to_owned(), request.depth)
        };
        assert_eq!(listed(&["x/y@v1"], None), (String::new(), 1));
        assert_eq!(
            listed(&["x/y", "src/tools/"], Some(3)),
            ("src/tools".to_owned(), 3)
        );
        for operands in [&[][..], &["x/y", "a", "b"]] {
            let error = request(&cli(operands, None)).unwrap_err();
            assert_eq!(error.code(), "usage");
            assert_eq!(error.hints(), Vec::<String>::new());
            assert_eq!(
                error.usage(),
                ["sego list [--json] [--depth N] <repo>[@<rev>] [<path>]"]
            );
        }
    }

    #[test]
    fn unresolved_submodules_show_their_url() {
        let url = "/srv/git/x y";
        let listing = Listing {
            at: at(),
            path: String::new(),
            entries: vec![Entry {
                path: "x".to_owned(),
                kind: EntryKind::Submodule {
                    url: url.to_owned(),
                    target: None,
                },
            }],
            truncated: false,
        };
        let submodule = Submodule {
            at: at(),
            path: "x".to_owned(),
            url: url.to_owned(),
            target: None,
        };
        let answers = [
            List { tree: Tree::Listing(listing), depth: 1 },
            List { tree: Tree::Submodule(submodule), depth: 1 },
        ];
        let shown: String =
            answers.iter().map(|list| list.text() + &list.json()).collect();
        expect![[r#"
            github.com/x/a@a7962380ca08 • 1 entry
            x • submodule '/srv/git/x y'
            {"version":1,"kind":"listing","repo":"github.com/x/a","commit":"a7962380ca08b5a188038c69871b8d3fbdf31e89","path":"","entries":[{"path":"x","type":"submodule","submodule":{"url":"/srv/git/x y"}}]}
            github.com/x/a@a7962380ca08 x • submodule
            '/srv/git/x y'
            {"version":1,"kind":"submodule","repo":"github.com/x/a","commit":"a7962380ca08b5a188038c69871b8d3fbdf31e89","path":"x","submodule":{"url":"/srv/git/x y"}}
        "#]].assert_eq(&shown);
    }

    #[test]
    fn truncated_listings_hint_one_level_less() {
        let shown: String = [1, 3]
            .map(|depth| {
                let listing = Listing {
                    at: at(),
                    path: "a b".to_owned(),
                    entries: Vec::new(),
                    truncated: true,
                };
                let list = List { tree: Tree::Listing(listing), depth };
                list.text() + &list.json()
            })
            .concat();
        expect![[r#"
            github.com/x/a@a7962380ca08 'a b/' • 0 entries
            notice: listing stopped at 1000 entries; some entries are not shown
            {"version":1,"kind":"listing","repo":"github.com/x/a","commit":"a7962380ca08b5a188038c69871b8d3fbdf31e89","path":"a b","entries":[],"partial":{"reason":"listing stopped at 1000 entries; some entries are not shown","hints":[]}}
            github.com/x/a@a7962380ca08 'a b/' • 0 entries
            notice: listing stopped at 1000 entries; some entries are not shown
            hint: sego list github.com/x/a@a7962380ca08 'a b/' --depth 2
            {"version":1,"kind":"listing","repo":"github.com/x/a","commit":"a7962380ca08b5a188038c69871b8d3fbdf31e89","path":"a b","entries":[],"partial":{"reason":"listing stopped at 1000 entries; some entries are not shown","hints":["sego list github.com/x/a@a7962380ca08 'a b/' --depth 2"]}}
        "#]].assert_eq(&shown);
    }

    #[test]
    fn help_states_the_cap() {
        assert!(
            HELP.details
                .contains(&format!("listing at {LIST_ENTRY_CAP} entries"))
        );
        assert!(
            HELP.details.contains(&format!("stopped at {LIST_ENTRY_CAP}"))
        );
    }
}
