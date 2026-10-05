use serde::Serialize;

use super::list::path_line;
use super::read::count;
use super::{Command, Help, Present, UsageError};
use crate::address::{CommitId, Pinned, Range, RepoPath};
use crate::args::Cli;
use crate::diagnostic::{Diagnostic, Outcome};
use crate::domain::{
    CommitCount, CommitDetail, CommitSummary, Comparison, DiffHunk,
    FileChange, FileStatus,
};
use crate::escape::{command_line, content_line, sanitise, shell_word};
use crate::render::json::{Partial, document};
use crate::render::text::{address_line, day, hint, notice, path_word};
use crate::sourcegraph::Fetch;
use crate::sourcegraph::comparison::{self, CompareRequest, listed};
use crate::sourcegraph::envelope::decode;

pub const HELP: Help = Help {
    about: "\
sego compare prints what changed between two revisions, or in one commit:
the commits, the files changed and, for a file, its hunks.
",
    synopsis: "[--json] <repo>@[<base>...]<head> [<path>]",
    details: "\
Operands:
  <repo>    a repository, such as github.com/gorilla/mux; a URL, a trailing
            .git and a trailing slash are accepted. Matching is
            case-insensitive.
  <base>    a branch, tag or commit-ish, such as v1.7.0 or a7962380~1; it
            cannot contain \"..\". The files are compared from the merge
            base of <base> and <head>, as git diff <base>...<head> does,
            and the commits are those on <head> and not on <base>.
  <head>    a branch, tag or commit-ish. Without <base>, the commit it
            names is compared with its first parent.
  <path>    a file or a directory, from the repository root. A directory
            narrows the commits and the files to those below it; a file
            also shows its hunks. Without it, the whole repository.

Output:
  A first line: the range, pinned to the commits the revisions resolved
  to, the path, the number of commits, and the files changed with the
  lines added and deleted. Then, after a blank line, the commits, newest
  first: each an address line with the day of its author date (UTC), its
  author and its subject. Then, after a blank line, the head's address
  line, and a line for each file changed, in path order: its path, then
  added, deleted, renamed from its old path, or modified, then the lines
  added and deleted, or binary:

    github.com/gorilla/mux@a7962380ca08...98cb6bf42e08 • 30 commits • …

    github.com/gorilla/mux@98cb6bf42e08 • 2020-07-11 • Harshavardhana • …
    …

    github.com/gorilla/mux@98cb6bf42e08
    .circleci/config.yml • added • +87
    context.go • deleted at a7962380ca08 • -18
    mux.go • modified • +21 -16

  A deleted file names the commit that holds it, the base, when the base
  is an ancestor of the head; otherwise it has no address. A file renamed
  in the range, compared by its new path, shows as added; the comparison
  of the whole repository, or of a directory holding both paths, shows
  the rename.

  The list of commits stops at the newest 50, and the list of files at the
  first 200, each then followed by a notice: line with the total. With a
  <path>, a list of commits that stopped says more than 50, as there is no
  exact count.

  A file's <path> shows its hunks after its line, as a unified diff. A line
  wider than 500 columns is cut, and says so; --json has it whole.

  A single commit's first line is its address line, with the day, the
  author and the files changed. Then its parents, the first being the one
  it is diffed against; then, after a blank line, its whole message,
  indented; then its files.

  A comparison with no commits says so: <head> has none that <base> lacks.
  When <base> has commits that <head> lacks, a hint: line follows with the
  command for the two swapped. A <path> that didn't change says no changes.

  With --json, one document of kind \"comparison\": repo, base (null for a
  single commit) and head (40 hex digits); for a single commit, date,
  author, message and parents; path; commits [{commit, date, author,
  subject}], commit_count (null when there is no exact count) and
  commits_truncated; files_changed, added, deleted; files [{path,
  old_path, status, binary, added, deleted, readable_at}], with hunks
  [{old_start, old_lines, new_start, new_lines, section, lines,
  no_newline_old, no_newline_new}] for a file's <path>; hints; and
  partial {reason, hints} when a list stopped. Anything else is reported
  on stderr, and its document, of kind \"unavailable\" or \"error\", also
  goes to stdout.

Exit codes:
  0  answered      the comparison, including one with no commits
  1  answered no   no such repository, revision or path; a commit without
                   <base> that has no parent
  2  invalid       bad operands, such as no @<head>, a two-dot range or a
                   revision containing \"..\"; an invalid SEGO_ENDPOINT
  3  unanswered    no usable answer from the upstream; retry

Examples:
  sego compare github.com/gorilla/mux@v1.7.0...v1.8.0
  sego compare github.com/gorilla/mux@v1.7.0...v1.8.0 mux.go
  sego compare github.com/gorilla/mux@98cb6bf42e08
",
};

const OPERANDS: &str = "expected <repo>@[<base>...]<head> [<path>]";

/// The comparison `cli` asks for: `<repo>@[<base>...]<head> [<path>]`.
pub fn request(cli: &Cli) -> Result<CompareRequest, UsageError> {
    let invalid =
        |message: String| UsageError::new(message, Some(Command::Compare));
    let (range, path) = match cli.operands.as_slice() {
        [range] => (range, None),
        [range, path] => (range, Some(path)),
        [] => {
            let message = format!("missing operands; {OPERANDS}");
            return Err(invalid(message));
        }
        [..] => {
            let message = format!("too many operands; {OPERANDS}");
            return Err(invalid(message));
        }
    };
    let range =
        Range::parse(range).map_err(|error| invalid(error.to_string()))?;
    let path = path.map(|path| RepoPath::parse(path));
    Ok(CompareRequest { range, path })
}

pub fn run(
    cli: &Cli,
    fetch: Fetch,
) -> Result<Box<dyn Present>, Box<dyn Diagnostic>> {
    let request = request(cli)?;
    let reply = fetch(&comparison::plan(&request))?;
    let comparison = comparison::answer(&request, decode(reply)?)?;
    Ok(Box::new(Compare { comparison, range: request.range }))
}

/// A comparison, and the range as it was asked for, whose revisions a
/// swapped command repeats.
pub struct Compare {
    pub comparison: Comparison,
    pub range: Range,
}

/// The number of commits, as `commits` gives it.
fn commit_count(commits: CommitCount) -> String {
    match commits {
        CommitCount::Exact(n) => count(u64::from(n), "commit"),
        CommitCount::MoreThan(n) => format!("more than {n} commits"),
    }
}

impl Compare {
    /// The command for the range swapped, when the range has no commits
    /// and its reverse has some.
    fn swapped(&self) -> Option<String> {
        let Comparison { repo, ahead, behind, path, .. } = &self.comparison;
        let base = self.range.base.as_ref()?;
        if *ahead > 0 || *behind == 0 {
            return None;
        }
        let head = self.range.head.as_str();
        let range = format!("{}@{head}...{}", repo.as_str(), base.as_str());
        let mut operands = vec![range.as_str()];
        operands.extend(path.as_deref());
        Some(command_line(&["sego", "compare"], &operands))
    }

    /// Whether the list of commits stopped short of them all. A single
    /// commit lists none.
    fn commits_truncated(&self) -> bool {
        let Comparison { commit, commits, commit_count, .. } =
            &self.comparison;
        match *commit_count {
            _ if commit.is_some() => false,
            CommitCount::Exact(n) => n > listed(commits.len()),
            CommitCount::MoreThan(_) => true,
        }
    }

    /// The notice for a list of commits that stopped.
    fn commits_notice(&self) -> Option<String> {
        let shown = listed(self.comparison.commits.len());
        let total = commit_count(self.comparison.commit_count);
        self.commits_truncated()
            .then(|| format!("{total}; the newest {shown} shown"))
    }

    /// The notice for a list of files that stopped.
    fn files_notice(&self) -> Option<String> {
        let Comparison { files, files_changed, files_truncated, .. } =
            &self.comparison;
        let total = count(u64::from(*files_changed), "file");
        let shown = listed(files.len());
        files_truncated.then(|| format!("{total} changed; {shown} shown"))
    }

    fn partial(&self) -> Option<Partial> {
        let notices: Vec<String> =
            [self.commits_notice(), self.files_notice()]
                .into_iter()
                .flatten()
                .collect();
        (!notices.is_empty())
            .then(|| Partial { reason: notices.join(". "), hints: Vec::new() })
    }

    /// The first line: the pinned range or commit and the path; a single
    /// commit's day and author; then the commits and the files changed.
    fn first_line(&self) -> String {
        let c = &self.comparison;
        let at = match &c.base {
            Some(base) => format!(
                "{}@{}...{}",
                c.repo.as_str(),
                base.short(),
                c.head.short()
            ),
            None => self.at(&c.head).to_string(),
        };
        let path = c.path.as_deref().map(path_word);
        let mut words = vec![at.as_str()];
        words.extend(path.as_deref());
        let mut meta = Vec::new();
        if let Some(CommitDetail { date, author, .. }) = &c.commit {
            meta.extend([day(date).to_owned(), author.clone()]);
        }
        let unchanged =
            c.path.is_some() && c.files_changed == 0 && c.commits.is_empty();
        if c.ahead == 0 {
            meta.push("no commits".to_owned());
        } else if unchanged {
            meta.push("no changes".to_owned());
        } else {
            if c.commit.is_none() {
                meta.push(commit_count(c.commit_count));
            }
            meta.push(match c.files_changed {
                0 => "no files changed".to_owned(),
                n => format!(
                    "{} changed, +{} -{}",
                    count(u64::from(n), "file"),
                    c.added,
                    c.deleted
                ),
            });
        }
        address_line(&words, &meta)
    }

    /// The repository at `commit`.
    fn at(&self, commit: &CommitId) -> Pinned {
        Pinned { repo: self.comparison.repo.clone(), commit: commit.clone() }
    }

    /// A single commit's parents, then its message after a blank line.
    fn commit_text(&self, detail: &CommitDetail) -> String {
        let parents: Vec<String> = detail
            .parents
            .iter()
            .map(|parent| self.at(parent).to_string())
            .collect();
        let mut words = vec!["parents:"];
        words.extend(parents.iter().map(String::as_str));
        let marked = match parents.len() {
            1 => "diffed against",
            _ => "diffed against the first",
        };
        let mut text = address_line(&words, &[marked.to_owned()]);
        text.push('\n');
        for line in detail.message.split('\n') {
            if !line.is_empty() {
                text.push_str("    ");
                text.push_str(&sanitise(line));
            }
            text.push('\n');
        }
        text
    }

    fn commit_line(&self, commit: &CommitSummary) -> String {
        let CommitSummary { commit, date, author, subject } = commit;
        let meta = [day(date).to_owned(), author.clone(), subject.clone()];
        address_line(&[&self.at(commit).to_string()], &meta)
    }
}

/// A changed file's entry line: its path, its status and its lines added
/// and deleted, or binary.
fn file_line(file: &FileChange) -> String {
    let FileChange { path, old_path, status, binary, added, deleted, .. } =
        file;
    let status_word = match status {
        FileStatus::Added => "added".to_owned(),
        FileStatus::Deleted => match &file.readable_at {
            Some(at) => format!("deleted at {}", at.short()),
            None => "deleted".to_owned(),
        },
        FileStatus::Renamed => {
            let old = old_path.as_deref().unwrap_or_default();
            format!("renamed from {}", shell_word(&path_word(old)))
        }
        FileStatus::Modified => "modified".to_owned(),
    };
    let lines = match status {
        _ if *binary => "binary".to_owned(),
        FileStatus::Added => format!("+{added}"),
        FileStatus::Deleted => format!("-{deleted}"),
        FileStatus::Renamed | FileStatus::Modified => {
            format!("+{added} -{deleted}")
        }
    };
    path_line(path, &[status_word, lines])
}

/// `hunk` as a unified diff, its lines cut as content lines, with git's
/// marker after each side's last line when it has no newline.
fn hunk_text(hunk: &DiffHunk) -> String {
    let DiffHunk { old_start, old_lines, new_start, new_lines, .. } = hunk;
    let mut text =
        format!("@@ -{old_start},{old_lines} +{new_start},{new_lines} @@");
    if let Some(section) = hunk.section.as_deref().filter(|s| !s.is_empty()) {
        text.push(' ');
        text.push_str(&sanitise(section));
    }
    text.push('\n');
    let old_end = hunk.lines.iter().rposition(|line| !line.starts_with('+'));
    let new_end = hunk.lines.iter().rposition(|line| !line.starts_with('-'));
    for (i, line) in hunk.lines.iter().enumerate() {
        text.push_str(&content_line(line, None));
        text.push('\n');
        let old = hunk.no_newline_old && old_end == Some(i);
        let new = hunk.no_newline_new && new_end == Some(i);
        if old || new {
            text.push_str("\\ No newline at end of file\n");
        }
    }
    text
}

impl Present for Compare {
    fn outcome(&self) -> Outcome {
        Outcome::Answered
    }

    fn text(&self) -> String {
        let c = &self.comparison;
        let mut text = self.first_line();
        if let Some(detail) = &c.commit {
            text.push_str(&self.commit_text(detail));
        }
        if !c.commits.is_empty() {
            text.push('\n');
            for commit in &c.commits {
                text.push_str(&self.commit_line(commit));
            }
            if let Some(reason) = self.commits_notice() {
                text.push_str(&notice(&reason));
            }
        }
        if !c.files.is_empty() {
            text.push('\n');
            text.push_str(&address_line(
                &[&self.at(&c.head).to_string()],
                &[],
            ));
            for (i, file) in c.files.iter().enumerate() {
                text.push_str(&file_line(file));
                if c.path_is_file && i == 0 {
                    for hunk in &c.hunks {
                        text.push_str(&hunk_text(hunk));
                    }
                }
            }
            if let Some(reason) = self.files_notice() {
                text.push_str(&notice(&reason));
            }
        }
        if let Some(command) = self.swapped() {
            text.push_str(&hint(&command));
        }
        text
    }

    fn json(&self) -> String {
        let c = &self.comparison;
        let detail = c.commit.as_ref();
        let commit_count = match c.commit_count {
            CommitCount::Exact(n) => Some(n),
            CommitCount::MoreThan(_) => None,
        };
        let files = c
            .files
            .iter()
            .enumerate()
            .map(|(i, file)| FileJson {
                path: &file.path,
                old_path: file.old_path.as_deref(),
                status: match file.status {
                    FileStatus::Added => "added",
                    FileStatus::Deleted => "deleted",
                    FileStatus::Renamed => "renamed",
                    FileStatus::Modified => "modified",
                },
                binary: file.binary,
                added: file.added,
                deleted: file.deleted,
                readable_at: file.readable_at.as_ref().map(|at| at.0.as_str()),
                hunks: (c.path_is_file && i == 0)
                    .then(|| c.hunks.iter().map(HunkJson::new).collect()),
            })
            .collect();
        let body = ComparisonJson {
            repo: c.repo.as_str(),
            base: c.base.as_ref().map(|base| base.0.as_str()),
            head: &c.head.0,
            date: detail.map(|detail| detail.date.as_str()),
            author: detail.map(|detail| detail.author.as_str()),
            message: detail.map(|detail| detail.message.as_str()),
            parents: detail.map(|detail| {
                detail.parents.iter().map(|parent| parent.0.as_str()).collect()
            }),
            path: c.path.as_deref(),
            commits: c
                .commits
                .iter()
                .map(|commit| CommitJson {
                    commit: &commit.commit.0,
                    date: &commit.date,
                    author: &commit.author,
                    subject: &commit.subject,
                })
                .collect(),
            commit_count,
            commits_truncated: self.commits_truncated(),
            files_changed: c.files_changed,
            added: c.added,
            deleted: c.deleted,
            files,
            hints: self.swapped().into_iter().collect(),
            partial: self.partial(),
        };
        document("comparison", &body)
    }
}

#[derive(Serialize)]
struct ComparisonJson<'a> {
    repo: &'a str,
    base: Option<&'a str>,
    head: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    date: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    author: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    parents: Option<Vec<&'a str>>,
    path: Option<&'a str>,
    commits: Vec<CommitJson<'a>>,
    commit_count: Option<u32>,
    commits_truncated: bool,
    files_changed: u32,
    added: u32,
    deleted: u32,
    files: Vec<FileJson<'a>>,
    hints: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    partial: Option<Partial>,
}

#[derive(Serialize)]
struct CommitJson<'a> {
    commit: &'a str,
    date: &'a str,
    author: &'a str,
    subject: &'a str,
}

#[derive(Serialize)]
struct FileJson<'a> {
    path: &'a str,
    old_path: Option<&'a str>,
    status: &'static str,
    binary: bool,
    added: u32,
    deleted: u32,
    readable_at: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    hunks: Option<Vec<HunkJson<'a>>>,
}

#[derive(Serialize)]
struct HunkJson<'a> {
    old_start: u32,
    old_lines: u32,
    new_start: u32,
    new_lines: u32,
    section: Option<&'a str>,
    lines: Vec<&'a str>,
    no_newline_old: bool,
    no_newline_new: bool,
}

impl<'a> HunkJson<'a> {
    fn new(hunk: &'a DiffHunk) -> HunkJson<'a> {
        HunkJson {
            old_start: hunk.old_start,
            old_lines: hunk.old_lines,
            new_start: hunk.new_start,
            new_lines: hunk.new_lines,
            section: hunk.section.as_deref(),
            lines: hunk.lines.iter().map(String::as_str).collect(),
            no_newline_old: hunk.no_newline_old,
            no_newline_new: hunk.no_newline_new,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use expect_test::expect;

    use super::*;
    use crate::address::RepoName;
    use crate::args;
    use crate::cmd::read;
    use crate::escape::CLIP_COLUMNS;
    use crate::sourcegraph::comparison::{COMMIT_CAP, FILE_CAP};

    fn commit(digit: &str) -> CommitId {
        CommitId(digit.repeat(40))
    }

    /// `v1...v2` in mux, one commit ahead, with no files.
    fn comparison() -> Comparison {
        Comparison {
            repo: RepoName::parse("github.com/gorilla/mux").unwrap(),
            base: Some(commit("a")),
            head: commit("b"),
            commit: None,
            ahead: 1,
            behind: 0,
            commits: vec![CommitSummary {
                commit: commit("b"),
                date: "2020-07-11T20:05:21Z".to_owned(),
                author: "moraes".to_owned(),
                subject: "Merge branch 'x'".to_owned(),
            }],
            commit_count: CommitCount::Exact(1),
            files_changed: 0,
            added: 0,
            deleted: 0,
            files: Vec::new(),
            files_truncated: false,
            path: None,
            path_is_file: false,
            hunks: Vec::new(),
        }
    }

    fn compare(comparison: Comparison) -> Compare {
        let range = Range::parse("github.com/gorilla/mux@v1...v2").unwrap();
        Compare { comparison, range }
    }

    fn file(path: &str, status: FileStatus) -> FileChange {
        FileChange {
            path: path.to_owned(),
            old_path: None,
            status,
            binary: false,
            added: 1,
            deleted: 1,
            readable_at: None,
        }
    }

    #[test]
    fn dashed_paths_print_with_a_slash() {
        let renamed = FileChange {
            old_path: Some("-old.go".to_owned()),
            ..file("--x.go", FileStatus::Renamed)
        };
        let deleted =
            FileChange { added: 0, ..file("-gone.go", FileStatus::Deleted) };
        let compare = compare(Comparison {
            files_changed: 2,
            added: 1,
            deleted: 2,
            files: vec![renamed, deleted],
            path: Some("-x".to_owned()),
            ..comparison()
        });
        let text = compare.text();
        expect![[r#"
            github.com/gorilla/mux@aaaaaaaaaaaa...bbbbbbbbbbbb /-x • 1 commit • 2 files changed, +1 -2

            github.com/gorilla/mux@bbbbbbbbbbbb • 2020-07-11 • moraes • Merge branch 'x'

            github.com/gorilla/mux@bbbbbbbbbbbb
            /--x.go • renamed from /-old.go • +1 -1
            /-gone.go • deleted • -1
        "#]].assert_eq(&text);
        let entry = text.lines().last().unwrap();
        let (path, _) = entry.split_once(" • ").unwrap();
        let header = text.lines().nth(4).unwrap();
        let argv = ["read"].into_iter().chain(header.split(' ')).chain([path]);
        let cli = args::parse(argv.map(OsString::from)).unwrap();
        let request = read::request(&cli).unwrap();
        assert_eq!(request.address.rev_or_head(), "bbbbbbbbbbbb");
        assert_eq!(request.path.upstream(), "-gone.go");
        let document: serde_json::Value =
            serde_json::from_str(&compare.json()).unwrap();
        assert_eq!(document["path"], "-x");
        assert_eq!(document["files"][0]["path"], "--x.go");
        assert_eq!(document["files"][0]["old_path"], "-old.go");
    }

    #[test]
    fn reversed_ranges_hint_the_swapped_command() {
        let reversed = compare(Comparison {
            ahead: 0,
            behind: 3,
            commits: Vec::new(),
            commit_count: CommitCount::Exact(0),
            path: Some("-x".to_owned()),
            ..comparison()
        });
        expect![[r#"
            github.com/gorilla/mux@aaaaaaaaaaaa...bbbbbbbbbbbb /-x • no commits
            hint: sego compare github.com/gorilla/mux@v2...v1 '-x'
        "#]]
        .assert_eq(&reversed.text());
        let document: serde_json::Value =
            serde_json::from_str(&reversed.json()).unwrap();
        assert_eq!(
            document["hints"],
            serde_json::json!([reversed.swapped().unwrap()])
        );
    }

    #[test]
    fn json_lines_are_never_clipped() {
        let wide = format!("+{}", "x".repeat(CLIP_COLUMNS + 100));
        let compare = compare(Comparison {
            files_changed: 1,
            added: 1,
            files: vec![FileChange {
                deleted: 0,
                ..file("wide.go", FileStatus::Added)
            }],
            path: Some("wide.go".to_owned()),
            path_is_file: true,
            hunks: vec![DiffHunk {
                old_start: 0,
                old_lines: 0,
                new_start: 1,
                new_lines: 1,
                section: None,
                lines: vec![wide.clone()],
                no_newline_old: false,
                no_newline_new: false,
            }],
            ..comparison()
        });
        assert!(!compare.text().contains(&wide));
        let document: serde_json::Value =
            serde_json::from_str(&compare.json()).unwrap();
        let hunk = &document["files"][0]["hunks"][0];
        assert_eq!(hunk["lines"][0], wide.as_str());
    }

    #[test]
    fn commits_without_files_say_so() {
        let commits = compare(comparison());
        expect![[r#"
            github.com/gorilla/mux@aaaaaaaaaaaa...bbbbbbbbbbbb • 1 commit • no files changed

            github.com/gorilla/mux@bbbbbbbbbbbb • 2020-07-11 • moraes • Merge branch 'x'
        "#]].assert_eq(&commits.text());
        let untouched = compare(Comparison {
            base: None,
            commit: Some(CommitDetail {
                date: "2020-07-11T20:05:21Z".to_owned(),
                author: "moraes".to_owned(),
                message: "Subject\n\nBody".to_owned(),
                parents: vec![commit("c")],
            }),
            commits: Vec::new(),
            path: Some("LICENSE".to_owned()),
            ..comparison()
        });
        expect![[r#"
            github.com/gorilla/mux@bbbbbbbbbbbb LICENSE • 2020-07-11 • moraes • no changes
            parents: github.com/gorilla/mux@cccccccccccc • diffed against

                Subject

                Body
        "#]].assert_eq(&untouched.text());
    }

    #[test]
    fn operands_become_a_request() {
        let cli = |operands: &[&str]| Cli {
            command: Some("compare".to_owned()),
            operands: operands.iter().map(|&s| s.to_owned()).collect(),
            ..Cli::default()
        };
        let narrowed = request(&cli(&["x/y@v1...v2", "src/"])).unwrap();
        assert_eq!(narrowed.range.to_string(), "x/y@v1...v2");
        assert_eq!(narrowed.path.unwrap().upstream(), "src");
        let commit = request(&cli(&["x/y@abc"])).unwrap();
        assert_eq!((commit.range.base, commit.path), (None, None));
        let errors: Vec<String> =
            [&[][..], &["x/y"], &["x/y@a..b"], &["x/y@v1...v2", "a", "b"]]
                .iter()
                .map(|operands| {
                    let error = request(&cli(operands)).unwrap_err();
                    assert_eq!(error.hints(), Vec::<String>::new());
                    format!("{error}\n")
                })
                .collect();
        expect![[r#"
            missing operands; expected <repo>@[<base>...]<head> [<path>]
            missing revision; expected <repo>@[<base>...]<head>
            invalid revision a..b; a revision cannot contain ".."
            too many operands; expected <repo>@[<base>...]<head> [<path>]
        "#]]
        .assert_eq(&errors.concat());
    }

    #[test]
    fn help_states_the_caps() {
        for text in [
            format!("newest {COMMIT_CAP}"),
            format!("more than {COMMIT_CAP}"),
            format!("first {FILE_CAP}"),
            format!("wider than {CLIP_COLUMNS} columns"),
        ] {
            assert!(HELP.details.contains(text.as_str()), "no {text:?}");
        }
    }

    #[test]
    fn help_shows_the_synopsis() {
        let synopsis =
            "sego compare [--json] <repo>@[<base>...]<head> [<path>]";
        assert_eq!(Command::Compare.synopsis(0), synopsis);
        let block = format!("\nUsage:\n  {synopsis}\n\n");
        assert!(Command::Compare.help().contains(&block));
        let cli = Cli {
            command: Some("compare".to_owned()),
            operands: vec!["x/y@a..b".to_owned()],
            ..Cli::default()
        };
        let error = request(&cli).unwrap_err();
        assert_eq!(error.usage(), [synopsis]);
    }
}
