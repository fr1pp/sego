use serde::Serialize;

use super::{Command, Help, Present, UsageError};
use crate::address::{Address, AddressError, LineRange, RepoPath};
use crate::args::Cli;
use crate::diagnostic::{Diagnostic, Outcome};
use crate::domain::File;
use crate::escape::{command_line, content_line};
use crate::render::json::{Partial, document};
use crate::render::text::{
    Marker, address_line, hint, notice, numbered, path_word,
};
use crate::sourcegraph::Fetch;
use crate::sourcegraph::blob::{self, ReadRequest};
use crate::sourcegraph::envelope::decode;

/// The most lines a read without a range shows (spec §10: tune after
/// dogfooding).
pub const READ_LINE_CAP: u32 = 500;

pub const HELP: Help = Help {
    about: "\
sego read prints the numbered lines of a file at a revision.
",
    synopsis: "[--json] <repo>[@<rev>] <path> [<lines>]",
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
            and inclusive. Without it, the file up to line 500.

Output:
  An address line: the repository at the commit the revision resolved to,
  the path and the lines shown, then the file's line count. Then the
  numbered lines:

    github.com/gorilla/mux@a7962380ca08 mux.go 23-24 • 601 lines
      23: // NewRouter returns a new router instance.
      24: func NewRouter() *Router {

  A file longer than 500 lines, read without <lines>, stops at line 500,
  followed by a notice: line and a hint: line with the command for the
  rest. A binary file, whatever its size and the range, has its size in
  the address line, and no lines. A line wider than 500 columns is cut,
  and says so; --json has it whole.

  With --json, one document of kind \"file\": repo, commit (40 hex digits),
  path, byte_size, total_lines, content_type, binary, start, end (null
  without lines), lines [{n, text}], and partial {reason, hints} when the
  file stopped at line 500. Anything else is reported on stderr, and its
  document, of kind \"unavailable\" or \"error\", also goes to stdout.

Exit codes:
  0  answered      the lines
  1  answered no   no such repository, revision or path; a directory or a
                   submodule; a text file over the upstream's 1.5 MiB
                   read cap, whatever the range; a range that starts past
                   the end
  2  invalid       bad operands, such as line 0, a range that ends before
                   it starts or a revision containing \"..\"; an invalid
                   SEGO_ENDPOINT
  3  unanswered    no usable answer from the upstream; retry

Examples:
  sego read github.com/gorilla/mux mux.go
  sego read github.com/gorilla/mux@v1.7.0 mux.go 30
  sego read github.com/gorilla/mux@v1.7.0 mux.go 30-60
  sego read github.com/gorilla/mux@v1.7.0 mux.go 30-
  sego read github.com/gorilla/mux@v1.7.0 mux.go 30,60
",
};

const OPERANDS: &str = "expected <repo>[@<rev>] <path> [<lines>]";

/// The read `cli` asks for: `<repo>[@<rev>] <path> [<lines>]`. Without
/// `<lines>`, lines 1 to `READ_LINE_CAP`, and the request is capped.
pub fn request(cli: &Cli) -> Result<ReadRequest, UsageError> {
    let invalid =
        |message: String| UsageError::new(message, Some(Command::Read));
    let (repo, path, lines) = match cli.operands.as_slice() {
        [repo, path] => (repo, path, None),
        [repo, path, lines] => (repo, path, Some(lines)),
        [] | [_] => {
            let message = format!("missing operands; {OPERANDS}");
            return Err(invalid(message));
        }
        [..] => {
            let message = format!("too many operands; {OPERANDS}");
            return Err(invalid(message));
        }
    };
    let address_error = |error: AddressError| invalid(error.to_string());
    let address = Address::parse(repo).map_err(address_error)?;
    let path = RepoPath::parse(path);
    if path.is_root() {
        return Err(invalid(
            "empty path; read needs a file's path".to_owned(),
        ));
    }
    let (range, capped) = match lines {
        Some(lines) => {
            (LineRange::parse(lines).map_err(address_error)?, false)
        }
        None => (LineRange { start: 1, end: Some(READ_LINE_CAP) }, true),
    };
    let LineRange { start, end } = range;
    Ok(ReadRequest { address, path, start, end, capped })
}

pub fn run(
    cli: &Cli,
    fetch: Fetch,
) -> Result<Box<dyn Present>, Box<dyn Diagnostic>> {
    let request = request(cli)?;
    let reply = fetch(&blob::plan(&request))?;
    let file = blob::answer(&request, decode(reply)?)?;
    Ok(Box::new(Read { file, capped: request.capped }))
}

/// A file's lines; `capped` when no range was given, so the line cap
/// applies.
pub struct Read {
    pub file: File,
    pub capped: bool,
}

impl Read {
    /// The number of the last line shown, if any is.
    fn end(&self) -> Option<u32> {
        let shown = u32::try_from(self.file.lines.len()).ok()?;
        shown.checked_sub(1).map(|last| self.file.start + last)
    }

    /// The notice and the hint for the lines the cap left out.
    fn partial(&self) -> Option<(String, String)> {
        let File { at, path, total_lines, .. } = &self.file;
        let next = self.end()? + 1;
        if !self.capped || next > *total_lines {
            return None;
        }
        let rest = LineRange { start: next, end: Some(*total_lines) };
        let lines = if next == *total_lines { "line" } else { "lines" };
        let reason = format!("{lines} {rest} not shown");
        let from = LineRange { start: next, end: None }.to_string();
        let hint =
            command_line(&["sego", "read"], &[&at.to_string(), path, &from]);
        Some((reason, hint))
    }
}

/// `n` and `unit`, plural unless `n` is 1.
fn count(n: u64, unit: &str) -> String {
    match n {
        1 => format!("1 {unit}"),
        n => format!("{n} {unit}s"),
    }
}

impl Present for Read {
    fn outcome(&self) -> Outcome {
        Outcome::Answered
    }

    fn text(&self) -> String {
        let file = &self.file;
        let address = file.at.to_string();
        let range = self.end().map(|end| {
            LineRange { start: file.start, end: Some(end) }.to_string()
        });
        let path = path_word(&file.path);
        let mut words = vec![address.as_str(), &path];
        words.extend(range.as_deref());
        let meta = if file.binary {
            vec!["binary".to_owned(), count(file.byte_size, "byte")]
        } else {
            vec![count(u64::from(file.total_lines), "line")]
        };
        let mut text = address_line(&words, &meta);
        let lines: Vec<_> = (file.start..)
            .zip(&file.lines)
            .map(|(n, line)| (n, Marker::Match, content_line(line, None)))
            .collect();
        text.push_str(&numbered(&lines));
        if let Some((reason, command)) = self.partial() {
            text.push_str(&notice(&reason));
            text.push_str(&hint(&command));
        }
        text
    }

    fn json(&self) -> String {
        let file = &self.file;
        let body = FileJson {
            repo: file.at.repo.as_str(),
            commit: &file.at.commit.0,
            path: &file.path,
            byte_size: file.byte_size,
            total_lines: file.total_lines,
            content_type: &file.content_type,
            binary: file.binary,
            start: file.start,
            end: self.end(),
            lines: (file.start..)
                .zip(&file.lines)
                .map(|(n, text)| LineJson { n, text })
                .collect(),
            partial: self
                .partial()
                .map(|(reason, hint)| Partial { reason, hints: vec![hint] }),
        };
        document("file", &body)
    }
}

#[derive(Serialize)]
struct FileJson<'a> {
    repo: &'a str,
    commit: &'a str,
    path: &'a str,
    byte_size: u64,
    total_lines: u32,
    content_type: &'a str,
    binary: bool,
    start: u32,
    end: Option<u32>,
    lines: Vec<LineJson<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    partial: Option<Partial>,
}

#[derive(Serialize)]
struct LineJson<'a> {
    n: u32,
    text: &'a str,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostic::Diagnostic;
    use crate::escape::CLIP_COLUMNS;

    fn cli(operands: &[&str]) -> Cli {
        Cli {
            command: Some("read".to_owned()),
            operands: operands.iter().map(|&s| s.to_owned()).collect(),
            ..Cli::default()
        }
    }

    #[test]
    fn operands_become_a_request() {
        let range = |operands: &[&str]| {
            let request = request(&cli(operands)).unwrap();
            (request.start, request.end, request.capped)
        };
        let cap = Some(READ_LINE_CAP);
        assert_eq!(range(&["x/y", "a.go"]), (1, cap, true));
        assert_eq!(range(&["x/y", "a.go", "1-500"]), (1, cap, false));
        assert_eq!(range(&["x/y", "a.go", "30,60"]), (30, Some(60), false));
        assert_eq!(range(&["x/y", "a.go", "30-"]), (30, None, false));
        for operands in [&["x/y"][..], &["x/y", "a.go", "1", "2"]] {
            let error = request(&cli(operands)).unwrap_err();
            assert_eq!(error.code(), "usage");
            assert_eq!(error.hints(), Vec::<String>::new());
            assert_eq!(
                error.usage(),
                ["sego read [--json] <repo>[@<rev>] <path> [<lines>]"]
            );
        }
    }

    #[test]
    fn help_states_the_caps() {
        assert!(
            HELP.details.contains(&format!("up to line {READ_LINE_CAP}."))
        );
        assert!(
            HELP.details
                .contains(&format!("wider than {CLIP_COLUMNS} columns"))
        );
    }
}
