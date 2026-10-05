//! Blaming a file's lines: the commit that last changed each, with the
//! file's metadata, line count and the lines themselves.

use std::cmp::Ordering;
use std::fmt;

use serde::Deserialize;

use crate::address::{Address, CommitId, Pinned, RepoName, RepoPath};
use crate::diagnostic::{Diagnostic, Outcome};
use crate::domain::{Author, Blame, Hunk, LinesCommand, Unavailable};

use super::blob::{Body, Lines, split_lines, take_read_cap};
use super::envelope::Envelope;
use super::node::{self, Blob, Node};
use super::{FetchError, GLOBAL_TIMEOUT, Plan};

/// The largest GraphQL `Int`, sent as the end of an open range, which the
/// upstream clamps to the file's last line.
pub const LAST_LINE: u32 = 2_147_483_647;

/// `node`, `lines` and `body` are read's; `blame` gives the hunks. Each is
/// its own alias, so the read cap's error at `body` or an error at `blame`
/// leaves the others intact.
const QUERY: &str = r"query Blame(
  $repo: String!, $rev: String!, $path: String!, $start: Int!, $end: Int!
) {
  repository(name: $repo) {
    name
    commit(rev: $rev) {
      oid
      node: path(path: $path) {
        __typename
        ... on GitBlob {
          path byteSize binary contentType submodule { url commit }
        }
        ... on GitTree { path }
      }
      lines: path(path: $path) { ... on GitBlob { totalLines } }
      blame: path(path: $path) {
        ... on GitBlob {
          blame(startLine: $start, endLine: $end) {
            startLine endLine rev filename message
            author { person { name email } date }
          }
        }
      }
      body: path(path: $path) {
        ... on GitBlob {
          content(startLine: $start, endLine: $end, textOnly: true)
        }
      }
    }
  }
}
";

/// Lines `start` to `end` of `path`, 1-based and inclusive; no `end` runs
/// to the last line.
#[derive(Debug, Clone)]
pub struct BlameRequest {
    pub address: Address,
    pub path: RepoPath,
    pub start: u32,
    pub end: Option<u32>,
}

/// Both ends are always sent: the upstream takes both or neither, and
/// clamps an end past the last line.
pub fn plan(request: &BlameRequest) -> Plan {
    Plan {
        query: QUERY,
        variables: serde_json::json!({
            "repo": request.address.repo.as_str(),
            "rev": request.address.rev_or_head(),
            "path": request.path.upstream(),
            "start": request.start,
            "end": request.end.unwrap_or(LAST_LINE),
        }),
    }
}

/// The path of the blame's own error.
const BLAME: &[&str] = &["repository", "commit", "blame", "blame"];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Data {
    repository: Option<Repository>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Repository {
    name: String,
    commit: Option<Commit>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Commit {
    oid: String,
    node: Option<Node>,
    lines: Option<Lines>,
    blame: Option<BlameField>,
    body: Option<Body>,
}

/// `blame` is absent unless the path is a blob, and never null: when the
/// blame fails, its error nulls the whole alias instead.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BlameField {
    blame: Option<Vec<WireHunk>>,
}

/// Lines `startLine` up to `endLine`, exclusive. `filename` is the path at
/// `rev`; `message` is the summary line, its leading spaces kept.
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct WireHunk {
    start_line: u32,
    end_line: u32,
    rev: String,
    filename: String,
    message: String,
    author: Signature,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Signature {
    person: Person,
    date: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Person {
    name: String,
    email: String,
}

/// The blame `envelope` answers `request` with, or why there is none. An
/// error at the blame is classified from the other aliases, never from its
/// text, which leaks git command lines and server paths.
pub fn answer(
    request: &BlameRequest,
    mut envelope: Envelope,
) -> Result<Blame, Box<dyn Diagnostic>> {
    let too_large = take_read_cap(&mut envelope);
    let failed = envelope.take_error(BLAME).is_some();
    let trace = envelope.trace.clone();
    let drift =
        |cause: String| FetchError::Field { cause, trace: trace.clone() };
    // A blob's alias is null only with an error at its path: the read
    // cap's or the blame's, both taken above, or another, which `into_data`
    // reports. So only drift reaches this.
    let missing = |field: &str| drift(format!("a blob has no {field}"));
    let Data { repository } = envelope.into_data()?;
    let Some(Repository { name, commit }) = repository else {
        let repo = request.address.repo.as_str().to_owned();
        return Err(Unavailable::RepoNotFound { repo }.into());
    };
    let Some(Commit { oid, node, lines, blame, body }) = commit else {
        let rev = request.address.rev_or_head().to_owned();
        return Err(Unavailable::RevisionNotFound {
            repo: name,
            rev,
            side: None,
        }
        .into());
    };
    let repo =
        RepoName::parse(&name).map_err(|error| drift(error.to_string()))?;
    let at = Pinned { repo, commit: CommitId(oid) };
    let Blob { path, byte_size, binary, .. } =
        node::blob(&at, &request.path, node)?;
    let total_lines = lines
        .and_then(|lines| lines.total_lines)
        .ok_or_else(|| missing("totalLines"))?;
    // The upstream counts an empty file as one line.
    let total_lines = if byte_size == 0 { 0 } else { total_lines };
    let start = request.start;
    let end = request.end.unwrap_or(LAST_LINE).min(total_lines);
    // A binary file's hunks split its raw bytes at newlines, so it answers
    // with its metadata alone, whatever the range.
    let hunks = if binary {
        Vec::new()
    } else if start > total_lines.max(1) {
        let command = LinesCommand::Blame;
        return Err(Unavailable::OutOfRange {
            at,
            path,
            start,
            total_lines,
            command,
        }
        .into());
    } else if total_lines == 0 {
        // Any range of an empty file is an error at the blame.
        Vec::new()
    } else if failed {
        let errors = vec!["the upstream couldn't blame this file".to_owned()];
        return Err(FetchError::Server { errors, trace }.into());
    } else {
        let wire = blame
            .and_then(|blame| blame.blame)
            .ok_or_else(|| missing("blame"))?;
        hunks(wire, start, end).map_err(drift)?
    };
    let lines = if too_large {
        None
    } else if binary {
        Some(Vec::new())
    } else {
        let content = body
            .and_then(|body| body.content)
            .ok_or_else(|| missing("content"))?;
        let lines = split_lines(&content);
        // Each hunk's lines are sliced from these by its range.
        let range = end.saturating_add(1).saturating_sub(start);
        if u32::try_from(lines.len()) != Ok(range) {
            return Err(drift(format!(
                "the content has {} lines, not the {range} of lines \
                 {start}-{end}",
                lines.len()
            ))
            .into());
        }
        Some(lines)
    };
    Ok(Blame {
        at,
        path,
        byte_size,
        total_lines,
        binary,
        start,
        end,
        hunks,
        lines,
    })
}

/// `wire` as inclusive hunks in line order, with adjacent hunks from one
/// commit and path merged; or why they don't cover `start..=end` exactly.
fn hunks(
    wire: Vec<WireHunk>,
    start: u32,
    end: u32,
) -> Result<Vec<Hunk>, String> {
    let mut hunks = Vec::with_capacity(wire.len());
    for WireHunk { start_line, end_line, rev, filename, message, author } in
        wire
    {
        if end_line <= start_line {
            return Err(format!(
                "a blame hunk has endLine {end_line}, not after its \
                 startLine {start_line}"
            ));
        }
        let Signature { person: Person { name, email }, date } = author;
        hunks.push(Hunk {
            start: start_line,
            end: end_line - 1,
            commit: CommitId(rev),
            path: filename,
            date,
            author: Author { name, email },
            subject: message.trim().to_owned(),
        });
    }
    hunks.sort_by_key(|hunk| hunk.start);
    let mut next = start;
    for hunk in &hunks {
        let problem = match hunk.start.cmp(&next) {
            Ordering::Less => "repeats",
            Ordering::Greater => "leaves out",
            Ordering::Equal => {
                next = hunk.end + 1;
                continue;
            }
        };
        let line = hunk.start.min(next);
        return Err(format!("the blame {problem} line {line}"));
    }
    if next <= end {
        return Err(format!("the blame leaves out line {next}"));
    }
    if next - 1 > end {
        return Err(format!("the blame runs past line {end}"));
    }
    let mut merged: Vec<Hunk> = Vec::with_capacity(hunks.len());
    for hunk in hunks {
        match merged.last_mut() {
            Some(last)
                if last.commit == hunk.commit && last.path == hunk.path =>
            {
                last.end = hunk.end;
            }
            _ => merged.push(hunk),
        }
    }
    Ok(merged)
}

/// A blame the upstream didn't answer within `GLOBAL_TIMEOUT`.
#[derive(Debug)]
pub struct SlowBlame {
    trace: Option<String>,
}

/// `error`, or a `SlowBlame` if it is a transport error that timed out.
pub fn timed_out(error: FetchError) -> Box<dyn Diagnostic> {
    match error {
        FetchError::Transport { timed_out: true, trace, .. } => {
            Box::new(SlowBlame { trace })
        }
        error @ (FetchError::Transport { .. }
        | FetchError::Status { .. }
        | FetchError::NotJson { .. }
        | FetchError::Oversized { .. }
        | FetchError::Empty { .. }
        | FetchError::Field { .. }
        | FetchError::Server { .. }
        | FetchError::Schema { .. }) => Box::new(error),
    }
}

impl fmt::Display for SlowBlame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "the upstream gave no blame within {} s",
            GLOBAL_TIMEOUT.as_secs()
        )
    }
}

impl Diagnostic for SlowBlame {
    fn code(&self) -> &'static str {
        "transport"
    }

    fn outcome(&self) -> Outcome {
        Outcome::Unanswered
    }

    fn causes(&self) -> Vec<String> {
        vec![
            "a blame's cost follows its lines' history, not the range's width"
                .to_owned(),
            "the same request will take as long again".to_owned(),
        ]
    }

    fn detail(&self) -> Option<serde_json::Value> {
        let trace = self.trace.as_deref()?;
        Some(serde_json::json!({"trace": trace}))
    }
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use expect_test::expect;

    use super::*;
    use crate::diagnostic::report;
    use crate::domain::Blame;
    use crate::sourcegraph::testing::{self, decoded, failure};

    /// The capture cases, by name: each is `tests/blame/<name>.json`.
    fn cases() -> Vec<(&'static str, BlameRequest)> {
        let blame = |address: &str, path: &str, start, end| BlameRequest {
            address: Address::parse(address).unwrap(),
            path: RepoPath::parse(path),
            start,
            end,
        };
        let mux = "github.com/gorilla/mux@v1.7.0";
        let go = "github.com/golang/go@go1.23.0";
        vec![
            ("range", blame(mux, "mux.go", 214, Some(221))),
            ("whole", blame(mux, "mux.go", 1, None)),
            ("past-end", blame(mux, "mux.go", 700, Some(710))),
            ("end-clamped", blame(mux, "mux.go", 595, None)),
            ("end-past", blame(mux, "mux.go", 595, Some(700))),
            ("renamed", blame(go, "src/fmt/print.go", 1, Some(20))),
            (
                "ignore-revs",
                blame(
                    "github.com/pypa/pip@24.0",
                    "src/pip/_internal/cache.py",
                    33,
                    Some(44),
                ),
            ),
            ("too-large", blame(go, "api/go1.txt", 1, Some(5))),
            (
                "binary",
                blame(
                    go,
                    "src/archive/zip/testdata/gophercolor16x16.png",
                    1,
                    Some(8),
                ),
            ),
            (
                "binary-over-cap",
                blame(
                    go,
                    "src/crypto/internal/boring/syso/\
                     goboringcrypto_linux_amd64.syso",
                    1,
                    Some(10),
                ),
            ),
            ("empty", blame(go, "test/typeparam/issue47740.out", 1, None)),
            (
                "symlink",
                blame(
                    "github.com/serde-rs/serde@v1.0.200",
                    "serde/README.md",
                    1,
                    Some(1),
                ),
            ),
            (
                "submodule",
                blame(
                    "github.com/rust-lang/rust@1.80.0",
                    "src/tools/cargo",
                    1,
                    Some(1),
                ),
            ),
            ("directory", blame(mux, ".github", 1, Some(1))),
            ("missing-path", blame(mux, "nope.go", 1, Some(1))),
            (
                "missing-revision",
                blame("github.com/gorilla/mux@v9.9.9", "mux.go", 1, Some(1)),
            ),
            (
                "missing-repo",
                blame("github.com/gorilla/doesnotexist", "x", 1, Some(1)),
            ),
        ]
    }

    #[test]
    #[ignore = "network: run by just capture"]
    fn capture() {
        for (case, request) in cases() {
            testing::capture("blame", case, &plan(&request));
        }
    }

    fn case(name: &str) -> BlameRequest {
        cases()
            .into_iter()
            .find_map(|(case, request)| (case == name).then_some(request))
            .unwrap_or_else(|| panic!("no case {name}"))
    }

    /// The envelope of `tests/blame/<name>.json`.
    macro_rules! envelope {
        ($name:literal) => {
            testing::envelope(include_str!(concat!(
                "../../tests/blame/",
                $name,
                ".json"
            )))
        };
    }

    /// The answer to case `name`, from its capture.
    macro_rules! blame {
        ($name:literal) => {
            answer(&case($name), envelope!($name))
        };
    }

    /// The blame a result answers with.
    fn answered(result: Result<Blame, Box<dyn Diagnostic>>) -> Blame {
        result.unwrap_or_else(|diagnostic| panic!("{}", report(&*diagnostic)))
    }

    /// Each hunk on a line: its range, short commit, path, date, author and
    /// subject, with any character outside ASCII escaped, as the upstream
    /// can send text that isn't in NFC.
    fn hunks(blame: &Blame) -> String {
        let mut text = String::new();
        for hunk in &blame.hunks {
            let line = format!(
                "{}-{} {} {} {} {} <{}> {}",
                hunk.start,
                hunk.end,
                hunk.commit.short(),
                hunk.path,
                hunk.date,
                hunk.author.name,
                hunk.author.email,
                hunk.subject
            );
            for c in line.chars() {
                if c.is_ascii() {
                    text.push(c);
                } else {
                    text.extend(c.escape_unicode());
                }
            }
            text.push('\n');
        }
        text
    }

    /// The hunks are inclusive, in line order, and cover `start..=end`.
    fn assert_covers(blame: &Blame) {
        let mut next = blame.start;
        for hunk in &blame.hunks {
            assert!(hunk.start <= hunk.end, "{}", hunks(blame));
            assert_eq!(hunk.start, next, "{}", hunks(blame));
            next = hunk.end + 1;
        }
        assert_eq!(next, blame.end + 1, "{}", hunks(blame));
    }

    #[test]
    fn ranges_are_sent_whole() {
        let sent = |request: &BlameRequest| {
            let variables = plan(request).variables;
            (variables["start"].as_u64(), variables["end"].as_u64())
        };
        assert_eq!(sent(&case("range")), (Some(214), Some(221)));
        assert_eq!(
            sent(&case("end-clamped")),
            (Some(595), Some(2_147_483_647))
        );
        let line = BlameRequest { start: 30, end: Some(30), ..case("range") };
        assert_eq!(sent(&line), (Some(30), Some(30)));
    }

    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "a whole file's hunks are one snapshot"
    )]
    fn hunks_are_inclusive_sorted_and_merged() {
        let range = answered(blame!("range"));
        assert_covers(&range);
        assert_eq!(
            (range.at.to_string(), range.path.as_str(), range.total_lines),
            ("github.com/gorilla/mux@a7962380ca08".to_owned(), "mux.go", 601)
        );
        assert_eq!((range.start, range.end), (214, 221));
        assert_eq!(range.lines.as_ref().map(Vec::len), Some(8));
        expect![[r#"
            214-216 eac83ba2c004 mux.go 2012-10-03T04:48:17Z moraes <moraes@noreply.sourcegraph.com> Initial files.
            217-217 758eb64354aa mux.go 2018-12-07T15:48:26Z Joe Wilner <joewilner@noreply.sourcegraph.com> Improve subroute configuration propagation #422
            218-221 eac83ba2c004 mux.go 2012-10-03T04:48:17Z moraes <moraes@noreply.sourcegraph.com> Initial files.
        "#]].assert_eq(&hunks(&range));
        let whole = answered(blame!("whole"));
        assert_covers(&whole);
        assert_eq!((whole.start, whole.end), (1, 601));
        assert_eq!(whole.lines.as_ref().map(Vec::len), Some(601));
        expect![[r#"
            1-7 eac83ba2c004 mux.go 2012-10-03T04:48:17Z moraes <moraes@noreply.sourcegraph.com> Initial files.
            8-8 92ae1d67265b mux.go 2015-07-09T18:46:53Z Dj Gilcrease <djgilcrease@noreply.sourcegraph.com> Update the walk method to walk matchers so it walks the full list of routers and child routers
            9-11 eac83ba2c004 mux.go 2012-10-03T04:48:17Z moraes <moraes@noreply.sourcegraph.com> Initial files.
            12-12 c0a5cbce5acc mux.go 2015-05-29T21:16:30Z Craig Jellick <craigjellick@noreply.sourcegraph.com> Fix typo
            13-14 eac83ba2c004 mux.go 2012-10-03T04:48:17Z moraes <moraes@noreply.sourcegraph.com> Initial files.
            15-15 a659b61323b0 mux.go 2017-08-30T05:39:17Z Mayank Patel <mayankpatel@noreply.sourcegraph.com> Fix #271:  Return 405 instead of 404 when request method doesn't match the route
            16-16 e3702bed27f0 mux.go 2018-05-13T03:22:33Z Matt Silverlock <mattsilverlock@noreply.sourcegraph.com> [docs] Improve docstrings for middleware, skipclean (#375)
            17-17 ded0c29b24f9 mux.go 2018-05-01T03:11:36Z Matt Silverlock <mattsilverlock@noreply.sourcegraph.com> Fix linter issues (docs) (#370)
            18-18 a659b61323b0 mux.go 2017-08-30T05:39:17Z Mayank Patel <mayankpatel@noreply.sourcegraph.com> Fix #271:  Return 405 instead of 404 when request method doesn't match the route
            19-20 ded0c29b24f9 mux.go 2018-05-01T03:11:36Z Matt Silverlock <mattsilverlock@noreply.sourcegraph.com> Fix linter issues (docs) (#370)
            21-22 a659b61323b0 mux.go 2017-08-30T05:39:17Z Mayank Patel <mayankpatel@noreply.sourcegraph.com> Fix #271:  Return 405 instead of 404 when request method doesn't match the route
            23-24 eac83ba2c004 mux.go 2012-10-03T04:48:17Z moraes <moraes@noreply.sourcegraph.com> Initial files.
            25-25 6137e193cdcb mux.go 2018-12-17T14:42:43Z alimy <alimy@noreply.sourcegraph.com> remove redundant code that remove support gorilla/context (#427)
            26-48 eac83ba2c004 mux.go 2012-10-03T04:48:17Z moraes <moraes@noreply.sourcegraph.com> Initial files.
            49-52 a659b61323b0 mux.go 2017-08-30T05:39:17Z Mayank Patel <mayankpatel@noreply.sourcegraph.com> Fix #271:  Return 405 instead of 404 when request method doesn't match the route
            53-54 eac83ba2c004 mux.go 2012-10-03T04:48:17Z moraes <moraes@noreply.sourcegraph.com> Initial files.
            55-55 758eb64354aa mux.go 2018-12-07T15:48:26Z Joe Wilner <joewilner@noreply.sourcegraph.com> Improve subroute configuration propagation #422
            56-57 eac83ba2c004 mux.go 2012-10-03T04:48:17Z moraes <moraes@noreply.sourcegraph.com> Initial files.
            58-58 758eb64354aa mux.go 2018-12-07T15:48:26Z Joe Wilner <joewilner@noreply.sourcegraph.com> Improve subroute configuration propagation #422
            59-59 fdfca9f91739 mux.go 2016-06-04T08:04:23Z Eric J. Holmes <ericj.holmes@noreply.sourcegraph.com> Support native context.Context when go1.7 is used.
            60-61 6137e193cdcb mux.go 2018-12-17T14:42:43Z alimy <alimy@noreply.sourcegraph.com> remove redundant code that remove support gorilla/context (#427)
            62-62 fdfca9f91739 mux.go 2016-06-04T08:04:23Z Eric J. Holmes <ericj.holmes@noreply.sourcegraph.com> Support native context.Context when go1.7 is used.
            63-63 e718e932ee60 mux.go 2013-06-05T23:07:27Z John Nadratowski <johnnadratowski@noreply.sourcegraph.com> Add KeepContext flag to Router to disable clearing of context data.
            64-64 758eb64354aa mux.go 2018-12-07T15:48:26Z Joe Wilner <joewilner@noreply.sourcegraph.com> Improve subroute configuration propagation #422
            65-66 53c1911da2b5 mux.go 2018-01-16T17:23:47Z Roberto Santalla <robertosantalla@noreply.sourcegraph.com> [feat] Add middleware support as discussed in #293 (#294)
            67-124 758eb64354aa mux.go 2018-12-07T15:48:26Z Joe Wilner <joewilner@noreply.sourcegraph.com> Improve subroute configuration propagation #422
            125-126 eac83ba2c004 mux.go 2012-10-03T04:48:17Z moraes <moraes@noreply.sourcegraph.com> Initial files.
            127-137 9f48112f18a1 mux.go 2017-11-05T04:08:26Z Kamil Kisiel <kamilkisiel@noreply.sourcegraph.com> [docs] Document router.Match (#313)
            138-139 eac83ba2c004 mux.go 2012-10-03T04:48:17Z moraes <moraes@noreply.sourcegraph.com> Initial files.
            140-140 38545dbae82f mux.go 2013-02-03T02:48:19Z Sergey Shepelev <sergeyshepelev@noreply.sourcegraph.com> Removed excess bool variable assignment in `if`s
            141-146 53c1911da2b5 mux.go 2018-01-16T17:23:47Z Roberto Santalla <robertosantalla@noreply.sourcegraph.com> [feat] Add middleware support as discussed in #293 (#294)
            147-149 eac83ba2c004 mux.go 2012-10-03T04:48:17Z moraes <moraes@noreply.sourcegraph.com> Initial files.
            150-150 c329c7d19328 mux.go 2015-12-25T20:16:04Z bign8 <bign8@noreply.sourcegraph.com> Potential fix for #20
            151-155 7f0880185913 mux.go 2017-11-05T17:23:20Z Roberto Santalla <robertosantalla@noreply.sourcegraph.com> MatchErr is set to ErrNotFound if NotFoundHandler is used (#311)
            156-157 ded0c29b24f9 mux.go 2018-05-01T03:11:36Z Matt Silverlock <mattsilverlock@noreply.sourcegraph.com> Fix linter issues (docs) (#370)
            158-159 a659b61323b0 mux.go 2017-08-30T05:39:17Z Mayank Patel <mayankpatel@noreply.sourcegraph.com> Fix #271:  Return 405 instead of 404 when request method doesn't match the route
            160-162 c329c7d19328 mux.go 2015-12-25T20:16:04Z bign8 <bign8@noreply.sourcegraph.com> Potential fix for #20
            163-163 7f0880185913 mux.go 2017-11-05T17:23:20Z Roberto Santalla <robertosantalla@noreply.sourcegraph.com> MatchErr is set to ErrNotFound if NotFoundHandler is used (#311)
            164-165 c329c7d19328 mux.go 2015-12-25T20:16:04Z bign8 <bign8@noreply.sourcegraph.com> Potential fix for #20
            166-167 7f0880185913 mux.go 2017-11-05T17:23:20Z Roberto Santalla <robertosantalla@noreply.sourcegraph.com> MatchErr is set to ErrNotFound if NotFoundHandler is used (#311)
            168-175 eac83ba2c004 mux.go 2012-10-03T04:48:17Z moraes <moraes@noreply.sourcegraph.com> Initial files.
            176-176 8ac5cf967fba mux.go 2014-05-20T21:59:54Z Dave Newman <davenewman@noreply.sourcegraph.com> Add SkipClean option
            177-178 0a192a193177 mux.go 2016-09-02T15:33:43Z Kush Mansingh <kushmansingh@noreply.sourcegraph.com> Add useEncodedPath option to router and routes (#190)
            179-179 c9183aaddd26 mux.go 2017-10-20T03:46:20Z Mike Busch <mikebusch@noreply.sourcegraph.com> use req.URL.EscapedPath() instead of getPath(req) (#306)
            180-180 0a192a193177 mux.go 2016-09-02T15:33:43Z Kush Mansingh <kushmansingh@noreply.sourcegraph.com> Add useEncodedPath option to router and routes (#190)
            181-181 8ac5cf967fba mux.go 2014-05-20T21:59:54Z Dave Newman <davenewman@noreply.sourcegraph.com> Add SkipClean option
            182-182 674ef1c28022 mux.go 2016-08-24T13:45:17Z Kush Mansingh <kushmansingh@noreply.sourcegraph.com> Add mechanism to route based on the escaped path (#184)
            183-183 d10d5466f2db mux.go 2013-11-15T02:27:38Z Philip Schlump <philipschlump@noreply.sourcegraph.com> Fixed problem with droping query string.
            184-189 786d36e5ab04 mux.go 2016-03-07T19:42:43Z jingweno <jingweno@noreply.sourcegraph.com> `go fmt`
            190-190 d10d5466f2db mux.go 2013-11-15T02:27:38Z Philip Schlump <philipschlump@noreply.sourcegraph.com> Fixed problem with droping query string.
            191-194 8ac5cf967fba mux.go 2014-05-20T21:59:54Z Dave Newman <davenewman@noreply.sourcegraph.com> Add SkipClean option
            195-197 eac83ba2c004 mux.go 2012-10-03T04:48:17Z moraes <moraes@noreply.sourcegraph.com> Initial files.
            198-198 38545dbae82f mux.go 2013-02-03T02:48:19Z Sergey Shepelev <sergeyshepelev@noreply.sourcegraph.com> Removed excess bool variable assignment in `if`s
            199-199 eac83ba2c004 mux.go 2012-10-03T04:48:17Z moraes <moraes@noreply.sourcegraph.com> Initial files.
            200-201 fdfca9f91739 mux.go 2016-06-04T08:04:23Z Eric J. Holmes <ericj.holmes@noreply.sourcegraph.com> Support native context.Context when go1.7 is used.
            202-202 eac83ba2c004 mux.go 2012-10-03T04:48:17Z moraes <moraes@noreply.sourcegraph.com> Initial files.
            203-207 a659b61323b0 mux.go 2017-08-30T05:39:17Z Mayank Patel <mayankpatel@noreply.sourcegraph.com> Fix #271:  Return 405 instead of 404 when request method doesn't match the route
            208-208 eac83ba2c004 mux.go 2012-10-03T04:48:17Z moraes <moraes@noreply.sourcegraph.com> Initial files.
            209-209 c329c7d19328 mux.go 2015-12-25T20:16:04Z bign8 <bign8@noreply.sourcegraph.com> Potential fix for #20
            210-210 eac83ba2c004 mux.go 2012-10-03T04:48:17Z moraes <moraes@noreply.sourcegraph.com> Initial files.
            211-211 a659b61323b0 mux.go 2017-08-30T05:39:17Z Mayank Patel <mayankpatel@noreply.sourcegraph.com> Fix #271:  Return 405 instead of 404 when request method doesn't match the route
            212-216 eac83ba2c004 mux.go 2012-10-03T04:48:17Z moraes <moraes@noreply.sourcegraph.com> Initial files.
            217-217 758eb64354aa mux.go 2018-12-07T15:48:26Z Joe Wilner <joewilner@noreply.sourcegraph.com> Improve subroute configuration propagation #422
            218-222 eac83ba2c004 mux.go 2012-10-03T04:48:17Z moraes <moraes@noreply.sourcegraph.com> Initial files.
            223-223 758eb64354aa mux.go 2018-12-07T15:48:26Z Joe Wilner <joewilner@noreply.sourcegraph.com> Improve subroute configuration propagation #422
            224-225 eac83ba2c004 mux.go 2012-10-03T04:48:17Z moraes <moraes@noreply.sourcegraph.com> Initial files.
            226-227 033224c12ed4 mux.go 2014-04-23T16:19:14Z Thomas ten Cate <thomastencate@noreply.sourcegraph.com> Document behaviour of StrictSlash and PathPrefix better, and add tests to nail this down
            228-228 eac83ba2c004 mux.go 2012-10-03T04:48:17Z moraes <moraes@noreply.sourcegraph.com> Initial files.
            229-229 c572efe4294d mux.go 2017-12-02T20:38:52Z Matt Silverlock <mattsilverlock@noreply.sourcegraph.com> [docs] Note StrictSlash re-direct behaviour #308 (#321)
            230-234 033224c12ed4 mux.go 2014-04-23T16:19:14Z Thomas ten Cate <thomastencate@noreply.sourcegraph.com> Document behaviour of StrictSlash and PathPrefix better, and add tests to nail this down
            235-235 eac83ba2c004 mux.go 2012-10-03T04:48:17Z moraes <moraes@noreply.sourcegraph.com> Initial files.
            236-240 c572efe4294d mux.go 2017-12-02T20:38:52Z Matt Silverlock <mattsilverlock@noreply.sourcegraph.com> [docs] Note StrictSlash re-direct behaviour #308 (#321)
            241-244 b864f07c539c mux.go 2014-04-23T17:53:35Z Thomas ten Cate <thomastencate@noreply.sourcegraph.com> Propagate StrictSlash to subrouters instead of rudely turning it off
            245-249 eac83ba2c004 mux.go 2012-10-03T04:48:17Z moraes <moraes@noreply.sourcegraph.com> Initial files.
            250-250 8ac5cf967fba mux.go 2014-05-20T21:59:54Z Dave Newman <davenewman@noreply.sourcegraph.com> Add SkipClean option
            251-251 a41434aac388 mux.go 2016-05-02T17:24:09Z jingweno <jingweno@noreply.sourcegraph.com> Clarify more of `SkipClean`
            252-262 8ac5cf967fba mux.go 2014-05-20T21:59:54Z Dave Newman <davenewman@noreply.sourcegraph.com> Add SkipClean option
            263-273 0a192a193177 mux.go 2016-09-02T15:33:43Z Kush Mansingh <kushmansingh@noreply.sourcegraph.com> Add useEncodedPath option to router and routes (#190)
            274-279 eac83ba2c004 mux.go 2012-10-03T04:48:17Z moraes <moraes@noreply.sourcegraph.com> Initial files.
            280-281 758eb64354aa mux.go 2018-12-07T15:48:26Z Joe Wilner <joewilner@noreply.sourcegraph.com> Improve subroute configuration propagation #422
            282-294 eac83ba2c004 mux.go 2012-10-03T04:48:17Z moraes <moraes@noreply.sourcegraph.com> Initial files.
            295-295 78f60492585e mux.go 2013-01-17T08:52:41Z Jongmin Kim <jongminkim@noreply.sourcegraph.com> Update mux.go
            296-346 eac83ba2c004 mux.go 2012-10-03T04:48:17Z moraes <moraes@noreply.sourcegraph.com> Initial files.
            347-347 02c98b3f736c mux.go 2016-02-29T03:31:51Z Matt Silverlock <mattsilverlock@noreply.sourcegraph.com> [docs] Satisfied golint.
            348-352 cef3b0cd6d46 mux.go 2014-04-17T08:17:15Z sqs <sqs@noreply.sourcegraph.com> Add BuildVarsFunc to allow modifying route variables before generating a route's URL
            353-374 92ae1d67265b mux.go 2015-07-09T18:46:53Z Dj Gilcrease <djgilcrease@noreply.sourcegraph.com> Update the walk method to walk matchers so it walks the full list of routers and child routers
            375-377 cf57124f1d4a mux.go 2016-07-25T16:05:24Z Martin Hamrle <martinhamrle@noreply.sourcegraph.com> Fix error handling in  Router.Walk
            378-379 92ae1d67265b mux.go 2015-07-09T18:46:53Z Dj Gilcrease <djgilcrease@noreply.sourcegraph.com> Update the walk method to walk matchers so it walks the full list of routers and child routers
            380-380 4d814f7650d4 mux.go 2017-05-19T18:22:54Z Nick Miyake <nickmiyake@noreply.sourcegraph.com> Update ancestors parameter for WalkFunc for matcher subrouters
            381-384 92ae1d67265b mux.go 2015-07-09T18:46:53Z Dj Gilcrease <djgilcrease@noreply.sourcegraph.com> Update the walk method to walk matchers so it walks the full list of routers and child routers
            385-385 4d814f7650d4 mux.go 2017-05-19T18:22:54Z Nick Miyake <nickmiyake@noreply.sourcegraph.com> Update ancestors parameter for WalkFunc for matcher subrouters
            386-399 92ae1d67265b mux.go 2015-07-09T18:46:53Z Dj Gilcrease <djgilcrease@noreply.sourcegraph.com> Update the walk method to walk matchers so it walks the full list of routers and child routers
            400-408 eac83ba2c004 mux.go 2012-10-03T04:48:17Z moraes <moraes@noreply.sourcegraph.com> Initial files.
            409-413 a659b61323b0 mux.go 2017-08-30T05:39:17Z Mayank Patel <mayankpatel@noreply.sourcegraph.com> Fix #271:  Return 405 instead of 404 when request method doesn't match the route
            414-424 eac83ba2c004 mux.go 2012-10-03T04:48:17Z moraes <moraes@noreply.sourcegraph.com> Initial files.
            425-425 fdfca9f91739 mux.go 2016-06-04T08:04:23Z Eric J. Holmes <ericj.holmes@noreply.sourcegraph.com> Support native context.Context when go1.7 is used.
            426-431 eac83ba2c004 mux.go 2012-10-03T04:48:17Z moraes <moraes@noreply.sourcegraph.com> Initial files.
            432-435 13c822608100 mux.go 2015-08-05T08:24:37Z Orne Brocaar <ornebrocaar@noreply.sourcegraph.com> Update comment and specify KeepContext option.
            436-436 eac83ba2c004 mux.go 2012-10-03T04:48:17Z moraes <moraes@noreply.sourcegraph.com> Initial files.
            437-437 fdfca9f91739 mux.go 2016-06-04T08:04:23Z Eric J. Holmes <ericj.holmes@noreply.sourcegraph.com> Support native context.Context when go1.7 is used.
            438-442 eac83ba2c004 mux.go 2012-10-03T04:48:17Z moraes <moraes@noreply.sourcegraph.com> Initial files.
            443-444 fdfca9f91739 mux.go 2016-06-04T08:04:23Z Eric J. Holmes <ericj.holmes@noreply.sourcegraph.com> Support native context.Context when go1.7 is used.
            445-446 eac83ba2c004 mux.go 2012-10-03T04:48:17Z moraes <moraes@noreply.sourcegraph.com> Initial files.
            447-448 fdfca9f91739 mux.go 2016-06-04T08:04:23Z Eric J. Holmes <ericj.holmes@noreply.sourcegraph.com> Support native context.Context when go1.7 is used.
            449-469 eac83ba2c004 mux.go 2012-10-03T04:48:17Z moraes <moraes@noreply.sourcegraph.com> Initial files.
            470-470 8ac5cf967fba mux.go 2014-05-20T21:59:54Z Dave Newman <davenewman@noreply.sourcegraph.com> Add SkipClean option
            471-485 eac83ba2c004 mux.go 2012-10-03T04:48:17Z moraes <moraes@noreply.sourcegraph.com> Initial files.
            486-487 ac3897eae376 mux.go 2015-10-03T07:21:00Z Matt Casper <mattcasper@noreply.sourcegraph.com> Fix a typo in the comments, add a few comment docs
            488-488 c0a5cbce5acc mux.go 2015-05-29T21:16:30Z Craig Jellick <craigjellick@noreply.sourcegraph.com> Fix typo
            489-490 eac83ba2c004 mux.go 2012-10-03T04:48:17Z moraes <moraes@noreply.sourcegraph.com> Initial files.
            491-491 c0a5cbce5acc mux.go 2015-05-29T21:16:30Z Craig Jellick <craigjellick@noreply.sourcegraph.com> Fix typo
            492-493 eac83ba2c004 mux.go 2012-10-03T04:48:17Z moraes <moraes@noreply.sourcegraph.com> Initial files.
            494-496 c0a5cbce5acc mux.go 2015-05-29T21:16:30Z Craig Jellick <craigjellick@noreply.sourcegraph.com> Fix typo
            497-498 ac3897eae376 mux.go 2015-10-03T07:21:00Z Matt Casper <mattcasper@noreply.sourcegraph.com> Fix a typo in the comments, add a few comment docs
            499-503 c0a5cbce5acc mux.go 2015-05-29T21:16:30Z Craig Jellick <craigjellick@noreply.sourcegraph.com> Fix typo
            504-510 eac83ba2c004 mux.go 2012-10-03T04:48:17Z moraes <moraes@noreply.sourcegraph.com> Initial files.
            511-511 8c683ee571b6 mux.go 2017-06-21T21:27:26Z Cody Oss <codyoss@noreply.sourcegraph.com> Fix typo
            512-512 ac3897eae376 mux.go 2015-10-03T07:21:00Z Matt Casper <mattcasper@noreply.sourcegraph.com> Fix a typo in the comments, add a few comment docs
            513-519 c0a5cbce5acc mux.go 2015-05-29T21:16:30Z Craig Jellick <craigjellick@noreply.sourcegraph.com> Fix typo
            520-523 98fb535d771e mux.go 2015-07-19T08:57:47Z Clint Ryan <clintryan@noreply.sourcegraph.com> Issue 16: Return the regexp compile error
            524-528 c0a5cbce5acc mux.go 2015-05-29T21:16:30Z Craig Jellick <craigjellick@noreply.sourcegraph.com> Fix typo
            529-538 eac83ba2c004 mux.go 2012-10-03T04:48:17Z moraes <moraes@noreply.sourcegraph.com> Initial files.
            539-539 c21431a6cd00 mux.go 2015-07-05T10:18:38Z Clint Ryan <clintryan@noreply.sourcegraph.com> Fixed up commenting and removed unused code
            540-540 c0a5cbce5acc mux.go 2015-05-29T21:16:30Z Craig Jellick <craigjellick@noreply.sourcegraph.com> Fix typo
            541-564 eac83ba2c004 mux.go 2012-10-03T04:48:17Z moraes <moraes@noreply.sourcegraph.com> Initial files.
            565-565 c0a5cbce5acc mux.go 2015-05-29T21:16:30Z Craig Jellick <craigjellick@noreply.sourcegraph.com> Fix typo
            566-567 c21431a6cd00 mux.go 2015-07-05T10:18:38Z Clint Ryan <clintryan@noreply.sourcegraph.com> Fixed up commenting and removed unused code
            568-592 c0a5cbce5acc mux.go 2015-05-29T21:16:30Z Craig Jellick <craigjellick@noreply.sourcegraph.com> Fix typo
            593-601 a659b61323b0 mux.go 2017-08-30T05:39:17Z Mayank Patel <mayankpatel@noreply.sourcegraph.com> Fix #271:  Return 405 instead of 404 when request method doesn't match the route
        "#]].assert_eq(&hunks(&whole));
    }

    /// A reply for a text blob of `total_lines` lines: `hunks`, each
    /// `(startLine, endLine, rev, filename)`, and the lines they span, each
    /// its own number; or with `error`, that error at the blame alias.
    fn synthetic(
        total_lines: u32,
        hunks: &[(u32, u32, &str, &str)],
        error: Option<&str>,
    ) -> Envelope {
        let first = hunks.iter().map(|hunk| hunk.0).min().unwrap_or(1);
        let last = hunks.iter().map(|hunk| hunk.1).max().unwrap_or(1);
        let content: String =
            (first..last).map(|n| n.to_string() + "\n").collect();
        let hunks: Vec<serde_json::Value> = hunks
            .iter()
            .map(|&(start, end, rev, filename)| {
                serde_json::json!({
                    "startLine": start, "endLine": end, "rev": rev,
                    "filename": filename, "message": " a subject ",
                    "author": {
                        "person": {"name": "A", "email": "a@example.com"},
                        "date": "2024-01-02T03:04:05Z"
                    }
                })
            })
            .collect();
        let blame = match error {
            Some(_) => serde_json::Value::Null,
            None => serde_json::json!({"blame": hunks}),
        };
        let mut body = serde_json::json!({"data": {"repository": {
            "name": "github.com/gorilla/mux",
            "commit": {
                "oid": "a7962380ca08b5a188038c69871b8d3fbdf31e89",
                "node": {"__typename": "GitBlob", "path": "mux.go",
                    "byteSize": 4 * total_lines, "binary": false,
                    "contentType": "text/plain", "submodule": null},
                "lines": {"totalLines": total_lines},
                "blame": blame,
                "body": {"content": content}
            }
        }}});
        if let Some(message) = error {
            body["errors"] = serde_json::json!([{
                "message": message,
                "path": ["repository", "commit", "blame", "blame"]
            }]);
        }
        decoded(body.to_string())
    }

    fn lines(start: u32, end: u32) -> BlameRequest {
        BlameRequest { start, end: Some(end), ..case("range") }
    }

    const ONE: &str = "1111111111111111111111111111111111111111";
    const TWO: &str = "2222222222222222222222222222222222222222";

    #[test]
    fn adjacent_hunks_merge_only_with_the_same_commit_and_path() {
        let same = [(1, 5, ONE, "mux.go"), (5, 7, ONE, "mux.go")];
        let blame = answered(answer(&lines(1, 6), synthetic(6, &same, None)));
        assert_covers(&blame);
        expect![[r#"
            1-6 111111111111 mux.go 2024-01-02T03:04:05Z A <a@example.com> a subject
        "#]].assert_eq(&hunks(&blame));
        let moved = [(5, 7, ONE, "mux.go"), (1, 5, ONE, "old.go")];
        let blame = answered(answer(&lines(1, 6), synthetic(6, &moved, None)));
        assert_covers(&blame);
        expect![[r#"
            1-4 111111111111 old.go 2024-01-02T03:04:05Z A <a@example.com> a subject
            5-6 111111111111 mux.go 2024-01-02T03:04:05Z A <a@example.com> a subject
        "#]].assert_eq(&hunks(&blame));
        let other = [(1, 5, ONE, "mux.go"), (5, 7, TWO, "mux.go")];
        let blame = answered(answer(&lines(1, 6), synthetic(6, &other, None)));
        assert_eq!(blame.hunks.len(), 2);
    }

    #[test]
    fn a_gap_or_overlap_is_bad_response() {
        let gap = [(1, 3, ONE, "mux.go"), (4, 6, TWO, "mux.go")];
        let result = answer(&lines(1, 5), synthetic(5, &gap, None));
        expect![[r#"
            [bad_response Unanswered]
            error: the upstream's answer has missing or unknown fields
            caused by: the blame leaves out line 3
        "#]]
        .assert_eq(&failure(result));
        let overlap = [(1, 4, ONE, "mux.go"), (3, 6, TWO, "mux.go")];
        let result = answer(&lines(1, 5), synthetic(5, &overlap, None));
        expect![[r#"
            [bad_response Unanswered]
            error: the upstream's answer has missing or unknown fields
            caused by: the blame repeats line 3
        "#]]
        .assert_eq(&failure(result));
        let past = [(1, 3, ONE, "mux.go"), (3, 7, TWO, "mux.go")];
        let result = answer(&lines(1, 5), synthetic(6, &past, None));
        expect![[r#"
            [bad_response Unanswered]
            error: the upstream's answer has missing or unknown fields
            caused by: the blame runs past line 5
        "#]]
        .assert_eq(&failure(result));
        let empty = [(1, 3, ONE, "mux.go"), (3, 3, TWO, "mux.go")];
        let result = answer(&lines(1, 2), synthetic(5, &empty, None));
        expect![[r#"
            [bad_response Unanswered]
            error: the upstream's answer has missing or unknown fields
            caused by: a blame hunk has endLine 3, not after its startLine 3
        "#]]
        .assert_eq(&failure(result));
    }

    #[test]
    fn content_short_of_the_range_is_bad_response() {
        let hunk = [(1, 6, ONE, "mux.go")];
        let mut envelope = synthetic(5, &hunk, None);
        let data = envelope.data.as_mut().unwrap();
        data["repository"]["commit"]["body"]["content"] = "1\n2\n".into();
        let result = answer(&lines(1, 5), envelope);
        expect![[r#"
            [bad_response Unanswered]
            error: the upstream's answer has missing or unknown fields
            caused by: the content has 2 lines, not the 5 of lines 1-5
        "#]]
        .assert_eq(&failure(result));
    }

    #[test]
    fn the_end_is_clamped() {
        for blame in [blame!("end-clamped"), blame!("end-past")] {
            let blame = answered(blame);
            assert_covers(&blame);
            assert_eq!((blame.start, blame.end), (595, 601));
            assert_eq!(blame.lines.as_ref().map(Vec::len), Some(7));
        }
        expect![[r#"
            [out_of_range AnsweredNo]
            mux.go in github.com/gorilla/mux@a7962380ca08 has 601 lines, so line 700 is past its end
            hint: sego blame github.com/gorilla/mux@a7962380ca08 mux.go 1-601
        "#]].assert_eq(&failure(blame!("past-end")));
        let error = Some("git blame failed");
        let result = answer(&lines(602, 602), synthetic(601, &[], error));
        expect![[r#"
            [out_of_range AnsweredNo]
            mux.go in github.com/gorilla/mux@a7962380ca08 has 601 lines, so line 602 is past its end
            hint: sego blame github.com/gorilla/mux@a7962380ca08 mux.go 1-601
        "#]].assert_eq(&failure(result));
        let last = BlameRequest { start: 601, end: None, ..case("range") };
        let hunk = [(601, 602, ONE, "mux.go")];
        let blame = answered(answer(&last, synthetic(601, &hunk, None)));
        assert_eq!((blame.start, blame.end), (601, 601));
        assert_eq!(blame.lines, Some(vec!["601".to_owned()]));
        assert_covers(&blame);
    }

    #[test]
    fn renames_keep_the_old_path() {
        let blame = answered(blame!("renamed"));
        assert_covers(&blame);
        assert!(
            blame.hunks.iter().any(|hunk| hunk.path == "src/pkg/fmt/print.go"),
            "{}",
            hunks(&blame)
        );
        expect![[r#"
            1-7 418b97c67013 src/lib/fmt/print.go 2008-10-24T23:33:29Z Rob Pike <robpike@noreply.sourcegraph.com> add printf to fmt.
            8-8 a440cc0d702e src/fmt/print.go 2018-10-16T02:54:39Z Rob Pike <robpike@noreply.sourcegraph.com> fmt: print maps in key-sorted order
            9-11 1c729599996c src/pkg/fmt/print.go 2009-12-15T23:27:16Z Robert Griesemer <robertgriesemer@noreply.sourcegraph.com> 1) Change default gofmt default settings for
            12-12 d75e186e2c01 src/fmt/print.go 2022-04-19T04:06:41Z Rob Pike <robpike@noreply.sourcegraph.com> fmt: add a function to recover the original format string given a State
            13-13 12ad9b4315cc src/pkg/fmt/print.go 2011-09-29T18:23:06Z Rob Pike <robpike@noreply.sourcegraph.com> fmt: replace channel cache with slice.
            14-14 30aa701fec9f src/pkg/fmt/print.go 2011-11-08T23:40:58Z Rob Pike <robpike@noreply.sourcegraph.com> renaming_2: gofix -r go1pkgrename src/pkg/[a-l]*
            15-16 418b97c67013 src/lib/fmt/print.go 2008-10-24T23:33:29Z Rob Pike <robpike@noreply.sourcegraph.com> add printf to fmt.
            17-20 7da4ceddd020 src/fmt/print.go 2016-02-21T15:05:44Z Martin Mo\u{308}hrmann <martinmohrmann@noreply.sourcegraph.com> fmt: simplify buffer write methods and adjust calls to them
        "#]].assert_eq(&hunks(&blame));
    }

    #[test]
    fn ignored_revisions_and_symlinks() {
        let ignored = answered(blame!("ignore-revs"));
        assert_covers(&ignored);
        let line_35 = ignored
            .hunks
            .iter()
            .find(|hunk| (hunk.start..=hunk.end).contains(&35))
            .unwrap();
        assert!(line_35.commit.0.starts_with("fe5b7c9c"));
        expect![[r#"
            33-33 3dd4a0d1ac05 pip/cache.py 2017-07-20T18:59:44Z Pradyun Gedam <pradyungedam@noreply.sourcegraph.com> Introduce pip.cache.Cache base class for pip's caches (#4623)
            34-35 fe5b7c9c9af4 pip/cache.py 2017-06-15T13:04:05Z Pradyun Gedam <pradyungedam@noreply.sourcegraph.com> Begin moving WheelCache to pip.cache
            36-36 3dd4a0d1ac05 pip/cache.py 2017-07-20T18:59:44Z Pradyun Gedam <pradyungedam@noreply.sourcegraph.com> Introduce pip.cache.Cache base class for pip's caches (#4623)
            37-37 133ec67bedfd pip/cache.py 2017-07-18T13:54:46Z Pradyun Gedam <pradyungedam@noreply.sourcegraph.com> Move get_cache_path_for_link to WheelCache (#4603)
            38-38 93e6dd718483 src/pip/_internal/cache.py 2023-03-18T11:48:42Z St\u{e9}phane Bidoul <stephanebidoul@noreply.sourcegraph.com> Remove now unused FormatControl in WheelCache
            39-39 c148bcc1aa40 src/pip/_internal/cache.py 2020-12-24T23:00:05Z Jon Dufresne <jondufresne@noreply.sourcegraph.com> Use short Python3 super() syntax
            40-41 8ce08590129e src/pip/_internal/cache.py 2020-01-02T12:24:09Z St\u{e9}phane Bidoul (ACSONE) <stephanebidoul%28acsone%29@noreply.sourcegraph.com> Remove redundant expanduser in WheelCache
            42-42 133ec67bedfd pip/cache.py 2017-07-18T13:54:46Z Pradyun Gedam <pradyungedam@noreply.sourcegraph.com> Move get_cache_path_for_link to WheelCache (#4603)
            43-43 6d665fd065ab src/pip/_internal/cache.py 2021-07-24T04:13:10Z harupy <harupy@noreply.sourcegraph.com> convert type comments
            44-44 3dd4a0d1ac05 pip/cache.py 2017-07-20T18:59:44Z Pradyun Gedam <pradyungedam@noreply.sourcegraph.com> Introduce pip.cache.Cache base class for pip's caches (#4623)
        "#]].assert_eq(&hunks(&ignored));
        let symlink = answered(blame!("symlink"));
        assert_covers(&symlink);
        assert_eq!(symlink.lines, Some(vec!["../README.md".to_owned()]));
        expect![[r#"
            Blame {
                at: Pinned {
                    repo: RepoName(
                        "github.com/serde-rs/serde",
                    ),
                    commit: CommitId(
                        "cc865ac5236c094275b10bff4fa41e561b3e359f",
                    ),
                },
                path: "serde/README.md",
                byte_size: 12,
                total_lines: 1,
                binary: false,
                start: 1,
                end: 1,
                hunks: [
                    Hunk {
                        start: 1,
                        end: 1,
                        commit: CommitId(
                            "89cbb816735c8aa59fbbc442df04b6e3f9b6e7e8",
                        ),
                        path: "serde/README.md",
                        date: "2017-02-21T05:11:27Z",
                        author: Author {
                            name: "dtolnay",
                            email: "dtolnay@noreply.sourcegraph.com",
                        },
                        subject: "Include readme and licenses in crates.io archive",
                    },
                ],
                lines: Some(
                    [
                        "../README.md",
                    ],
                ),
            }
        "#]].assert_debug_eq(&symlink);
    }

    #[test]
    fn over_the_cap_there_are_hunks_without_lines() {
        let blame = answered(blame!("too-large"));
        assert_covers(&blame);
        assert!(blame.lines.is_none());
        expect![[r#"
            Blame {
                at: Pinned {
                    repo: RepoName(
                        "github.com/golang/go",
                    ),
                    commit: CommitId(
                        "6885bad7dd86880be6929c02085e5c7a67ff2887",
                    ),
                },
                path: "api/go1.txt",
                byte_size: 1759838,
                total_lines: 30871,
                binary: false,
                start: 1,
                end: 5,
                hunks: [
                    Hunk {
                        start: 1,
                        end: 5,
                        commit: CommitId(
                            "f69132d7ad89c6507af6079bd75d2fe50f7f7fe8",
                        ),
                        path: "api/go1.txt",
                        date: "2012-03-17T18:20:46Z",
                        author: Author {
                            name: "Brad Fitzpatrick",
                            email: "bradfitzpatrick@noreply.sourcegraph.com",
                        },
                        subject: "build: catch API changes during build",
                    },
                ],
                lines: None,
            }
        "#]]
        .assert_debug_eq(&blame);
    }

    #[test]
    fn binary_files_have_no_hunks() {
        let binary = answered(blame!("binary"));
        assert!(binary.binary && binary.hunks.is_empty());
        expect![[r#"
            Blame {
                at: Pinned {
                    repo: RepoName(
                        "github.com/golang/go",
                    ),
                    commit: CommitId(
                        "6885bad7dd86880be6929c02085e5c7a67ff2887",
                    ),
                },
                path: "src/archive/zip/testdata/gophercolor16x16.png",
                byte_size: 785,
                total_lines: 8,
                binary: true,
                start: 1,
                end: 8,
                hunks: [],
                lines: Some(
                    [],
                ),
            }
        "#]]
        .assert_debug_eq(&binary);
        let over = answered(blame!("binary-over-cap"));
        assert!(over.binary && over.hunks.is_empty() && over.lines.is_none());
        expect![[r#"
            Blame {
                at: Pinned {
                    repo: RepoName(
                        "github.com/golang/go",
                    ),
                    commit: CommitId(
                        "6885bad7dd86880be6929c02085e5c7a67ff2887",
                    ),
                },
                path: "src/crypto/internal/boring/syso/goboringcrypto_linux_amd64.syso",
                byte_size: 2555664,
                total_lines: 3790,
                binary: true,
                start: 1,
                end: 10,
                hunks: [],
                lines: None,
            }
        "#]].assert_debug_eq(&over);
    }

    #[test]
    fn empty_files_have_no_hunks() {
        let empty = answered(blame!("empty"));
        assert!(empty.hunks.is_empty());
        assert_eq!((empty.byte_size, empty.total_lines), (0, 0));
        expect![[r#"
            Blame {
                at: Pinned {
                    repo: RepoName(
                        "github.com/golang/go",
                    ),
                    commit: CommitId(
                        "6885bad7dd86880be6929c02085e5c7a67ff2887",
                    ),
                },
                path: "test/typeparam/issue47740.out",
                byte_size: 0,
                total_lines: 0,
                binary: false,
                start: 1,
                end: 0,
                hunks: [],
                lines: Some(
                    [],
                ),
            }
        "#]]
        .assert_debug_eq(&empty);
        let second = BlameRequest { start: 2, ..case("empty") };
        let result = answer(&second, envelope!("empty"));
        expect![[r#"
            [out_of_range AnsweredNo]
            test/typeparam/issue47740.out in github.com/golang/go@6885bad7dd86 has 0 lines, so line 2 is past its end
        "#]].assert_eq(&failure(result));
    }

    #[test]
    fn not_files() {
        let mut text = String::new();
        for result in [
            blame!("submodule"),
            blame!("directory"),
            blame!("missing-path"),
            blame!("missing-revision"),
            blame!("missing-repo"),
        ] {
            text.push_str(&failure(result));
        }
        expect![[r#"
            [not_a_file AnsweredNo]
            src/tools/cargo is a submodule in github.com/rust-lang/rust@051478957371
            hint: sego list github.com/rust-lang/rust@051478957371 src/tools/cargo
            [not_a_file AnsweredNo]
            .github is a directory in github.com/gorilla/mux@a7962380ca08
            hint: sego list github.com/gorilla/mux@a7962380ca08 .github/
            [path_not_found AnsweredNo]
            nope.go not found in github.com/gorilla/mux@a7962380ca08
            hint: sego list github.com/gorilla/mux@a7962380ca08
            [revision_not_found AnsweredNo]
            revision v9.9.9 not found in github.com/gorilla/mux
            hint: sego list github.com/gorilla/mux
            [repo_not_found AnsweredNo]
            repository github.com/gorilla/doesnotexist not found
            hint: sego search 'type:repo doesnotexist'
        "#]].assert_eq(&text);
    }

    #[test]
    fn other_blame_errors_are_server_errors() {
        let secret = "git command [git blame] failed: /data/repos/x";
        let result = answer(&lines(2, 3), synthetic(5, &[], Some(secret)));
        let text = failure(result);
        assert!(!text.contains("git"), "{text}");
        expect![[r#"
            [server_error Unanswered]
            error: the upstream answered with an error
            caused by: the upstream couldn't blame this file
        "#]]
        .assert_eq(&text);
    }

    #[test]
    fn slow_blames_say_so() {
        let transport = |timed_out, cause: &str| FetchError::Transport {
            url: "https://sourcegraph.com/.api/graphql".into(),
            cause: cause.into(),
            timed_out,
            trace: Some("abc123".into()),
        };
        let mut text = String::new();
        for diagnostic in [
            timed_out(transport(true, "timeout: global")),
            timed_out(transport(false, "timeout: connect")),
        ] {
            let _ = writeln!(
                text,
                "[{} {:?}] hints: {:?}",
                diagnostic.code(),
                diagnostic.outcome(),
                diagnostic.hints()
            );
            text.push_str(&report(&*diagnostic));
            if let Some(detail) = diagnostic.detail() {
                let _ = writeln!(text, "detail: {detail}");
            }
        }
        expect![[r#"
            [transport Unanswered] hints: []
            error: the upstream gave no blame within 95 s
            caused by: a blame's cost follows its lines' history, not the range's width
            caused by: the same request will take as long again
            detail: {"trace":"abc123"}
            [transport Unanswered] hints: []
            error: the request to https://sourcegraph.com/.api/graphql failed
            caused by: timeout: connect
            detail: {"trace":"abc123"}
        "#]].assert_eq(&text);
    }
}
