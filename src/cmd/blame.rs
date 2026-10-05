use serde::Serialize;

use super::read::{LineJson, file_line, json_lines, numbered_lines};
use super::{Command, Help, Present, UsageError};
use crate::address::{Address, AddressError, LineRange, Pinned, RepoPath};
use crate::args::Cli;
use crate::diagnostic::{Diagnostic, Outcome};
use crate::domain::{self, Hunk};
use crate::render::json::{Partial, document};
use crate::render::text::{address_line, notice, path_word};
use crate::sourcegraph::Fetch;
use crate::sourcegraph::blame::{self, BlameRequest};
use crate::sourcegraph::envelope::decode;

pub const HELP: Help = Help {
    about: "\
sego blame prints the commit that last changed each of a file's lines at
a revision, with the lines.
",
    synopsis: "[--json] <repo>[@<rev>] <path> <lines>",
    details: "\
Operands:
  <repo>    a repository, such as github.com/gorilla/mux; a URL, a trailing
            .git and a trailing slash are accepted. Matching is
            case-insensitive.
  <rev>     a branch, tag or commit-ish, such as main, v1.7.0 or
            a7962380~1; it cannot contain \"..\". Without it, the default
            branch.
  <path>    the file's path from the repository root
  <lines>   N, N-M, N- (to the last line) or N,M (the same as N-M); 1-based
            and inclusive. 1- is the whole file.

Output:
  An address line: the repository at the commit the revision resolved to,
  the path and the lines blamed, then the file's line count. Then each
  hunk, after a blank line: an address line with the commit that last
  changed its lines, the path as it was in that commit and the hunk's
  lines, then the day of the author date (UTC), the author's name and the
  commit's subject; then the hunk's numbered lines:

    github.com/gorilla/mux@a7962380ca08 mux.go 214-221 • 601 lines

    github.com/gorilla/mux@eac83ba2c004 mux.go 214-216 • 2012-10-03 • …
     214:
     215: // Get returns a route registered with the given name.
     216: func (r *Router) Get(name string) *Route {

  The blame follows the repository's .git-blame-ignore-revs, as
  git blame --ignore-revs-file .git-blame-ignore-revs does. Its cost
  follows the history of the lines, not the width of the range.

  A text file over the upstream's 1.5 MiB read cap has its hunks without
  their lines, followed by a notice: line. A binary file has its size in
  the address line, then a notice: line, as a binary file has no lines to
  blame. A line wider than 500 columns is cut, and says so; --json has it
  whole.

  With --json, one document of kind \"blame\": repo, commit (40 hex digits),
  path, start, end (null without hunks), total_lines, byte_size, binary,
  and hunks [{start, end, commit, path, date, author {name, email},
  subject, lines [{n, text}]}]. Over the read cap, the hunks have no lines
  and partial {reason, hints} says why. Anything else is reported on
  stderr, and its document, of kind \"unavailable\" or \"error\", also goes
  to stdout.

Exit codes:
  0  answered      the hunks; a binary file's size
  1  answered no   no such repository, revision or path; a directory or a
                   submodule; a range that starts past the end
  2  invalid       bad operands, such as no <lines>, line 0, a range that
                   ends before it starts or a revision containing \"..\";
                   an invalid SEGO_ENDPOINT
  3  unanswered    no usable answer from the upstream; retry. A blame with
                   no answer within 95 s will take as long again.

Examples:
  sego blame github.com/gorilla/mux@v1.7.0 mux.go 214-221
  sego blame github.com/gorilla/mux@v1.7.0 mux.go 30
  sego blame github.com/gorilla/mux@v1.7.0 mux.go 1-
",
};

const OPERANDS: &str = "expected <repo>[@<rev>] <path> <lines>";

/// The blame `cli` asks for: `<repo>[@<rev>] <path> <lines>`.
pub fn request(cli: &Cli) -> Result<BlameRequest, UsageError> {
    let invalid =
        |message: String| UsageError::new(message, Some(Command::Blame));
    let (repo, path, lines) = match cli.operands.as_slice() {
        [repo, path, lines] => (repo, path, lines),
        [_, _] => return Err(invalid(format!("missing <lines>; {OPERANDS}"))),
        [] | [_] => {
            return Err(invalid(format!("missing operands; {OPERANDS}")));
        }
        [..] => {
            return Err(invalid(format!("too many operands; {OPERANDS}")));
        }
    };
    let address_error = |error: AddressError| invalid(error.to_string());
    let address = Address::parse(repo).map_err(address_error)?;
    let path = RepoPath::parse(path);
    if path.is_root() {
        return Err(invalid(
            "empty path; blame needs a file's path".to_owned(),
        ));
    }
    let LineRange { start, end } =
        LineRange::parse(lines).map_err(address_error)?;
    Ok(BlameRequest { address, path, start, end })
}

pub fn run(
    cli: &Cli,
    fetch: Fetch,
) -> Result<Box<dyn Present>, Box<dyn Diagnostic>> {
    let request = request(cli)?;
    let reply = fetch(&blame::plan(&request)).map_err(blame::timed_out)?;
    let blame = blame::answer(&request, decode(reply)?)?;
    Ok(Box::new(Blame { blame }))
}

/// Why a text file over the read cap has hunks without lines.
const OVER_CAP: &str = "the file is over the upstream's 1.5 MiB read cap, \
                        so its lines aren't shown";

/// A file's hunks, each with its lines unless the file is over the read
/// cap.
pub struct Blame {
    pub blame: domain::Blame,
}

impl Blame {
    /// The lines the hunks cover, if there are any hunks.
    fn range(&self) -> Option<LineRange> {
        let domain::Blame { start, end, .. } = self.blame;
        (!self.blame.hunks.is_empty())
            .then_some(LineRange { start, end: Some(end) })
    }

    /// Why the lines aren't shown, when a text file is over the read cap.
    /// A binary file has no lines to show, whatever its size.
    fn partial(&self) -> Option<Partial> {
        let over_cap = self.blame.lines.is_none() && !self.blame.binary;
        over_cap.then(|| Partial {
            reason: OVER_CAP.to_owned(),
            hints: Vec::new(),
        })
    }

    /// `hunk`'s lines, if the lines are known.
    fn lines(&self, hunk: &Hunk) -> Option<&[String]> {
        let lines = self.blame.lines.as_deref()?;
        let from = hunk.start.checked_sub(self.blame.start)?;
        let to = hunk.end.checked_sub(self.blame.start)?;
        let from = usize::try_from(from).ok()?;
        let to = usize::try_from(to).ok()?;
        lines.get(from..=to)
    }
}

/// `hunk`'s address line: the blamed repository at the hunk's commit, its
/// path then, and its lines; then the day of its date, its author and its
/// subject.
fn hunk_line(blame: &domain::Blame, hunk: &Hunk) -> String {
    let at =
        Pinned { repo: blame.at.repo.clone(), commit: hunk.commit.clone() };
    let address = at.to_string();
    let path = path_word(&hunk.path);
    let range = LineRange { start: hunk.start, end: Some(hunk.end) };
    let range = range.to_string();
    let day = hunk.date.get(..10).unwrap_or(&hunk.date);
    let meta =
        [day.to_owned(), hunk.author.name.clone(), hunk.subject.clone()];
    address_line(&[&address, &path, &range], &meta)
}

impl Present for Blame {
    fn outcome(&self) -> Outcome {
        Outcome::Answered
    }

    fn text(&self) -> String {
        let blame = &self.blame;
        let mut text = file_line(
            &blame.at,
            &blame.path,
            self.range(),
            blame.binary,
            blame.byte_size,
            blame.total_lines,
        );
        for hunk in &blame.hunks {
            text.push('\n');
            text.push_str(&hunk_line(blame, hunk));
            if let Some(lines) = self.lines(hunk) {
                text.push_str(&numbered_lines(hunk.start, lines));
            }
        }
        if let Some(Partial { reason, .. }) = self.partial() {
            text.push_str(&notice(&reason));
        }
        if blame.binary {
            text.push_str(&notice("a binary file has no lines to blame"));
        }
        text
    }

    fn json(&self) -> String {
        let blame = &self.blame;
        let hunks = blame
            .hunks
            .iter()
            .map(|hunk| HunkJson {
                start: hunk.start,
                end: hunk.end,
                commit: &hunk.commit.0,
                path: &hunk.path,
                date: &hunk.date,
                author: AuthorJson {
                    name: &hunk.author.name,
                    email: &hunk.author.email,
                },
                subject: &hunk.subject,
                lines: self
                    .lines(hunk)
                    .map(|lines| json_lines(hunk.start, lines)),
            })
            .collect();
        let body = BlameJson {
            repo: blame.at.repo.as_str(),
            commit: &blame.at.commit.0,
            path: &blame.path,
            start: blame.start,
            end: self.range().and_then(|range| range.end),
            total_lines: blame.total_lines,
            byte_size: blame.byte_size,
            binary: blame.binary,
            hunks,
            partial: self.partial(),
        };
        document("blame", &body)
    }
}

#[derive(Serialize)]
struct BlameJson<'a> {
    repo: &'a str,
    commit: &'a str,
    path: &'a str,
    start: u32,
    end: Option<u32>,
    total_lines: u32,
    byte_size: u64,
    binary: bool,
    hunks: Vec<HunkJson<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    partial: Option<Partial>,
}

#[derive(Serialize)]
struct HunkJson<'a> {
    start: u32,
    end: u32,
    commit: &'a str,
    path: &'a str,
    date: &'a str,
    author: AuthorJson<'a>,
    subject: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    lines: Option<Vec<LineJson<'a>>>,
}

#[derive(Serialize)]
struct AuthorJson<'a> {
    name: &'a str,
    email: &'a str,
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use super::*;
    use crate::address::{CommitId, RepoName};
    use crate::args;
    use crate::domain::Author;
    use crate::escape::CLIP_COLUMNS;
    use crate::sourcegraph::GLOBAL_TIMEOUT;

    fn at(commit: &str) -> Pinned {
        Pinned {
            repo: RepoName::parse("github.com/gorilla/mux").unwrap(),
            commit: CommitId(commit.repeat(40)),
        }
    }

    /// A blame of `path`, lines 1 to `lines.len()`, in one hunk whose path
    /// at its commit is `hunk_path`.
    fn blame(path: &str, hunk_path: &str, lines: Vec<String>) -> Blame {
        let end = u32::try_from(lines.len()).unwrap();
        let hunk = Hunk {
            start: 1,
            end,
            commit: at("b").commit,
            path: hunk_path.to_owned(),
            date: "2012-10-03T04:48:17Z".to_owned(),
            author: Author {
                name: "moraes".to_owned(),
                email: "moraes@noreply.sourcegraph.com".to_owned(),
            },
            subject: "Initial files.".to_owned(),
        };
        Blame {
            blame: domain::Blame {
                at: at("a"),
                path: path.to_owned(),
                byte_size: 601,
                total_lines: end,
                binary: false,
                start: 1,
                end,
                hunks: vec![hunk],
                lines: Some(lines),
            },
        }
    }

    #[test]
    fn dashed_paths_paste_back() {
        let lines = vec!["a".to_owned(), "b".to_owned()];
        let text = blame("--x.go", "--x.go", lines).text();
        let header = text.lines().nth(2).unwrap();
        let (address, meta) = header.split_once(" • ").unwrap();
        assert_eq!(address, "github.com/gorilla/mux@bbbbbbbbbbbb /--x.go 1-2");
        assert_eq!(meta, "2012-10-03 • moraes • Initial files.");
        let argv = ["blame"].into_iter().chain(address.split(' '));
        let cli = args::parse(argv.map(OsString::from)).unwrap();
        assert_eq!(cli.command.as_deref(), Some("blame"));
        let request = request(&cli).unwrap();
        assert_eq!(request.address.repo.as_str(), "github.com/gorilla/mux");
        assert_eq!(request.address.rev_or_head(), "bbbbbbbbbbbb");
        assert_eq!(request.path.upstream(), "--x.go");
        assert_eq!((request.start, request.end), (1, Some(2)));
    }

    #[test]
    fn json_lines_are_never_clipped() {
        let wide = "x".repeat(CLIP_COLUMNS + 100);
        let blame = blame("wide.go", "wide.go", vec![wide.clone()]);
        assert!(!blame.text().contains(&wide));
        let document: serde_json::Value =
            serde_json::from_str(&blame.json()).unwrap();
        assert_eq!(document["hunks"][0]["lines"][0]["text"], wide.as_str());
    }

    #[test]
    fn help_states_the_limits() {
        let timeout = format!("within {} s", GLOBAL_TIMEOUT.as_secs());
        let clip = format!("wider than {CLIP_COLUMNS} columns");
        for text in [&timeout, &clip] {
            assert!(HELP.details.contains(text.as_str()), "no {text:?}");
        }
    }

    #[test]
    fn help_shows_the_synopsis() {
        let synopsis = "sego blame [--json] <repo>[@<rev>] <path> <lines>";
        assert_eq!(Command::Blame.synopsis(0), synopsis);
        let block = format!("\nUsage:\n  {synopsis}\n\n");
        assert!(Command::Blame.help().contains(&block));
        let cli = Cli {
            command: Some("blame".to_owned()),
            operands: vec!["x/y".to_owned(), "a.go".to_owned()],
            ..Cli::default()
        };
        let error = request(&cli).unwrap_err();
        assert_eq!(error.usage(), [synopsis]);
    }
}
