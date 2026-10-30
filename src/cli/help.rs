use super::{
    COMMANDS, DIFF_SYNOPSIS, LOG_SYNOPSIS, LS_SYNOPSIS, REFS_SYNOPSIS, SEARCH_SYNOPSIS,
    SHOW_SYNOPSIS, SYNOPSIS,
};
use crate::error::usage_block;

// A string continued with `\` drops the next line's leading spaces, so a
// block whose first line is indented is joined with `concat!`.
const REPO: &str = concat!(
    "  REPO     host/owner/name, owner/name, or a GitHub, GitLab or\n",
    "           Sourcegraph URL\n",
);

const REV: &str = "  @REV     a branch, tag or commit; the default branch without it\n";

const PATH_AND_LINES: &str = concat!(
    "  :PATH    a path from the repository root\n",
    "  LINES    N, N-M, N,M, or N- through the last line (lines are\n",
    "           numbered from 1)\n",
);

const OPTIONS: &str = "\
options:
  --json       one JSON document on stdout
  -h, --help   this help, or a command's help after its name
  --version    sego's version
";

const GENERAL_OUTPUT_AND_EXAMPLES: &str = r#"output:
  A header line comes first: what was read, such as REPO@COMMIT:PATH,
  with its counts. Lines of a file are numbered "N:". Notes go to stderr
  as "sego: note: ...". --json prints one JSON document instead, notes
  in "notes"; "sego COMMAND --help" lists its fields.

examples:
  sego search 'repo:^github\.com/gorilla/mux$@v1.8.0 NewRouter'
  sego show github.com/gorilla/mux@v1.8.0:mux.go 120-180
  sego show github.com/gorilla/mux@v1.8.0:mux.go 1-
  sego ls github.com/gorilla/mux@v1.8.0
  sego refs --json github.com/gorilla/mux 1.8 | jq -r '.refs[].name'
  sego log github.com/gorilla/mux:mux.go
  sego diff github.com/gorilla/mux@v1.7.4...v1.8.0 mux.go
"#;

const SEARCH_QUERY: &str = "\
QUERY... is joined with single spaces and sent as one Sourcegraph query.

query:
  -term          literal text, not an option (-h is still help)
  and, or, not   operators, in any case
  repo:, file:, lang:, type:, count:, timeout:
                 common filters
  repo:R@REV     search at a branch, tag or commit; rev:REV is the same
  rev:at.time(D) search at the last commit before date D; needs repo:
  repo:has.file(path:RE content:RE)
                 only repositories with a file that matches each part given
  select:repo    one result per repository; select:file, one per file
  -- TEXT        text that is one of sego's options, such as --json
";

const SEARCH_FACTS: &str = r#"defaults:
  A search covers each repository's default branch, and stops after 30
  results or at its 20 s deadline, though the answer can arrive later;
  count:N and timeout:60s raise them. By default,
  Sourcegraph excludes archived repositories and forks; add archived:yes or
  fork:yes to include them. No search reads a file over 1 MiB, and
  sego show reads files under 64 MiB; index:no searches files skipped for
  too many distinct trigrams. A query that names NOT-INDEXED, such as
  'repo:^REPO$ NOT-INDEXED count:all', lists the files the index skipped.

text:
  N UNIT  R repos  T s      UNIT is matches, commits, files,
                            directories, symbols or repos; N+ when
                            more may exist; "stopped at T s" when the
                            deadline cut the search, and a note names
                            what reaches further
  query  QUERY              the query as sent
  widen  FILTER (N repos)   repositories in scope left out by default
  REPO@COMMIT:PATH          a file with results, then "N:text" for a
                            matching line and "N-text" for context, or
                            "NOT-INDEXED: REASON" for a file the index
                            skipped
  A match is one matched range; a line can hold several.
  A diff result previews at most 5 files and 3 hunks a file.
  Text prints the first 100 matches; --json carries every result fetched.
  Sourcegraph doesn't report a search backend that fails to answer, so a
  0 can come from one, and a search over several repositories can miss a
  backend's share.

json:
  query pattern_type unit count at_least stopped elapsed_ms limit_ms
  widen[{filter repos}] alert
  results[]: kind (line, path, binary, unindexed, symbol, commit or repo)
    and repo; file kinds add commit rev path; unindexed adds reason; line
    adds line text match ranges chunk, with match false on a context
    line; symbol adds line name symbol_kind container; commit adds commit
    date author subject lines highlights preview_limited; date is the UTC
    day of the author date
  server notes

examples:
  sego search 'repo:^github\.com/gorilla/mux$ "func NewRouter"'
  sego search --json 'repo:^github\.com/gorilla/mux$ StrictSlash count:100'
"#;

const SHOW_FACTS: &str = r#"text:
  A file: REPO@COMMIT:PATH  lines A-B of N  rev REV  mirrored T, then a
  row per line, "N:text". Text shows lines 1-500 when LINES is not given,
  and 1- shows the whole file; --json carries the same lines.
  rev REV shows when REV was a name, and mirrored T when Sourcegraph last
  updated its mirror from the host, as read with this answer.
  CRLF: every line shown that has a line ending ends in CRLF, and those
  CRs are left out; any other CR prints as \u{d}. not UTF-8: bytes that
  aren't print as \x{hh}.
  binary N bytes: Sourcegraph's classification, not git's. A symlink is a
  file that shows its target; file modes aren't available.
  A directory or the root prints the ls answer; LINES for one is an error.
  A commit: REPO@COMMIT  DATE  AUTHOR  parents P  N files  +A -D  T tags,
  then the message, indented, a row per file, "STATUS  +A -D  PATH", and
  a row per tag that holds the commit, "tag  NAME", newest created first.
  A root commit has no file list: Sourcegraph has no diff for it. Text
  prints 100 tags; --json carries every tag.

json:
  A file: repo rev commit path kind (file or submodule) total_lines
  byte_size binary crlf utf8 (null when unknown) lfs_size
  submodule{url address commit} start
  end lines[{line text bytes}], bytes the line in base64 when not UTF-8
  A commit: repo rev commit kind author author_date committer
  committer_date parents subject message
  files[{status old_path new_path added deleted}] files_total files_more
  added deleted tags tags_total, both null when the tags couldn't be read
  Each adds mirrored_at notes.

examples:
  sego show github.com/gorilla/mux@v1.8.0:mux.go 120-180
  sego show github.com/gorilla/mux@758eb64354aa
"#;

const LS_OPTIONS: &str = "\
options:
  --json        one JSON document on stdout
  --recursive   the whole subtree, not one directory
  -h, --help    this help, or a command's help after its name
  --version     sego's version
";

const LS_FACTS: &str = r#"text:
  REPO@COMMIT[:PATH]  N entries  D dirs  F files  S submodules  rev REV
  mirrored T, then a row per entry, "KIND  PATH", with KIND dir, file or
  submodule. A file or a submodule is its own one row. Paths are from the
  repository root, sorted segment by segment. A submodule's URL comes
  from .gitmodules; "?" when it has none. Text prints 200 entries; --json
  carries every entry.

json:
  repo rev commit path kind (directory, file or submodule) recursive
  entries[{path kind submodule_url submodule_address submodule_commit}]
  entries_total entries_more mirrored_at notes

examples:
  sego ls github.com/gorilla/mux@v1.8.0
  sego ls --recursive github.com/gorilla/mux@v1.8.0:.github
"#;

const REFS_TEXT: &str = concat!(
    "  TEXT     only refs whose name contains TEXT, in any case: 1.8\n",
    "           matches v1.8.0, 1.8.0 and release-1.8 alike\n",
);

const REFS_FACTS: &str = r#"text:
  REPO  N branches  M tags  mirrored T, or with TEXT, "X of Y branches
  match" and "X of Y tags match", then a row per ref, "KIND  TARGET
  NAME", the default branch marked. TARGET is the object the ref names,
  a tree for some tags.
  Refs come in Sourcegraph's order: the default branch, then newest
  created first (an annotated tag's tagger date, else its commit's date),
  then by name. Text prints 100 branches and 100 tags; --json carries
  every ref.

json:
  repo query default_branch branches_total tags_total branches_all
  tags_all branches_more tags_more refs[{kind name target}] mirrored_at
  notes

examples:
  sego refs github.com/gorilla/mux 1.8
  sego refs --json github.com/gorilla/mux | jq -r '.refs[].name'
"#;

const LOG_OPERANDS: &str = "\
operands:
  REPO       host/owner/name, owner/name, or a GitHub, GitLab or
             Sourcegraph URL
  REPO@REV   commits reachable from REV, in git log order; the default
             branch without @REV
  REPO@A..B  commits reachable from B and not from A, as git log A..B
  PATH       one path, as git log REV -- PATH, which simplifies history
";

const LOG_OPTIONS: &str = "\
options:
  --json       one JSON document on stdout
  --follow     follow PATH back through renames, as git log --follow; one
               revision and one plain path; merges aren't listed
  -h, --help   this help, or a command's help after its name
  --version    sego's version
";

const LOG_FACTS: &str = r#"text:
  REPO@COMMIT[:PATH]  N commits  behind B  follow  rev REV  mirrored T,
  then a row per commit, "COMMIT  DATE  AUTHOR  SUBJECT", with
  "(tag: NAME)" before the subject of a tagged commit. behind B: commits
  in A and not in B, for a range with no PATH.
  Text and JSON list the first 150 commits of a REV, and without --follow
  a note names what lists the rest; a range lists them all in JSON.

json:
  repo rev commit rev_a rev_b commit_a commit_b path follow behind
  commits_total commits_more commits[{commit date author subject parents
  tags}]; date is the UTC day of the author date; tags is null when the
  tag list couldn't be read; mirrored_at notes

examples:
  sego log github.com/gorilla/mux:mux.go
  sego log github.com/gorilla/mux@v1.7.4..v1.8.0 mux.go
"#;

const DIFF_OPERANDS: &str = "\
operands:
  REPO        host/owner/name, owner/name, or a GitHub, GitLab or
              Sourcegraph URL
  REPO@A...B  changes from the merge base of A and B to B, as git
              diff A...B
  REPO@REV    the change REV made, against its first parent; a root
              commit has none on Sourcegraph
  PATH...     paths or directory prefixes; the file list, counts and patch
              cover only these
";

const DIFF_FACTS: &str = r#"text:
  REPO@A...B  N files  +A -D  base COMMIT  rev A...B, then a row per
  file, "STATUS  +A -D  PATH", with STATUS A, D, M or R (OLD -> NEW),
  then the patch of named paths, indented four spaces. One path joins
  the header as REPO@A...B:PATH; two or more add "paths P...".
  base is A...B's merge base, or REV's first parent; mirrored T as in
  sego show --help.
  Text prints 200 file rows; --json carries every file.
  With PATH operands, a rename shows as R only when both its paths are
  named.

json:
  repo rev_a rev_b commit_a commit_b base files_total files_more added
  deleted files[{status old_path new_path added deleted}] paths
  patch, unindented, or null without PATH; mirrored_at notes

examples:
  sego diff github.com/gorilla/mux@v1.7.4...v1.8.0
  sego diff github.com/gorilla/mux@v1.7.4...v1.8.0 mux.go
"#;

/// The sections every command's help ends with.
const EVERY_COMMAND: &str = r"errors:
  An error's indented lines state facts, such as the default branch, and
  can name the sego command or filter that finds what it couldn't.

exit codes:
  0  answered, even when the answer is empty or a lower bound
  1  not found: what the command line names doesn't exist on
     Sourcegraph; asking again won't help, a different address may
  2  usage error: the command line is malformed, names what no mirror
     can hold, or is ambiguous, and must change
  3  failure: Sourcegraph or the network didn't answer, or the answer
     couldn't be written; asking again later may help

escapes:
  Text from Sourcegraph prints a control, separator or invisible
  character as \u{h}, and a byte that isn't UTF-8 as \x{hh}; a word
  printed for pasting is quoted for bash. --json keeps the exact text.
";

/// A command's help: its usage, then `operands`, `options` and `facts`,
/// one blank line apart.
fn command_help(synopsis: &str, operands: &str, options: &str, facts: &str) -> String {
    format!("{}\n\n{operands}\n{options}\n{facts}\n{EVERY_COMMAND}", usage_block(synopsis))
}

pub fn search() -> String {
    command_help(SEARCH_SYNOPSIS, SEARCH_QUERY, OPTIONS, SEARCH_FACTS)
}

pub fn show() -> String {
    let addresses = format!(
        "addresses:\n{REPO}{REV}  @REV     with no :PATH, the commit: its \
         message and changed files\n{PATH_AND_LINES}"
    );
    command_help(SHOW_SYNOPSIS, &addresses, OPTIONS, SHOW_FACTS)
}

pub fn ls() -> String {
    let addresses = format!(
        "addresses:\n{REPO}{REV}  :PATH    a directory from the repository \
         root; the root without it\n"
    );
    command_help(LS_SYNOPSIS, &addresses, LS_OPTIONS, LS_FACTS)
}

pub fn refs() -> String {
    let operands = format!("operands:\n{REPO}{REFS_TEXT}");
    command_help(REFS_SYNOPSIS, &operands, OPTIONS, REFS_FACTS)
}

pub fn log() -> String {
    command_help(LOG_SYNOPSIS, LOG_OPERANDS, LOG_OPTIONS, LOG_FACTS)
}

pub fn diff() -> String {
    command_help(DIFF_SYNOPSIS, DIFF_OPERANDS, OPTIONS, DIFF_FACTS)
}

/// The help for `topic`, a command's name, or the general help for `None`.
pub fn text(topic: Option<&str>) -> String {
    if let Some(entry) = topic.and_then(|word| COMMANDS.iter().find(|e| e.name == word)) {
        return (entry.help)();
    }
    format!(
        "\
sego searches and reads code on Sourcegraph, for agents.

usage: {SYNOPSIS}

commands:
  search QUERY...                code matching a Sourcegraph query
  show REPO[@REV]:PATH [LINES]   lines of a file
  show REPO@REV                  a commit: its message and changed files
  ls REPO[@REV][:PATH]           entries of a directory
  refs REPO [TEXT]               branches and tags
  log REPO[@REV][:PATH] [PATH]   commits, in git log order
  log REPO@A..B [PATH]           commits in B and not in A
  diff REPO@A...B [PATH...]      changed files, and the patch of named paths
  diff REPO@REV [PATH...]        the change one commit made

addresses:
{REPO}{REV}{PATH_AND_LINES}
{GENERAL_OUTPUT_AND_EXAMPLES}
{OPTIONS}
environment:
  SEGO_CACHE_DIR   where sego caches large files, or off (default
                   $XDG_CACHE_HOME/sego or ~/.cache/sego)

exit codes: 0 answered, 1 not found, 2 usage error, 3 failure
"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn topics() -> impl Iterator<Item = Option<&'static str>> {
        std::iter::once(None).chain(COMMANDS.iter().map(|entry| Some(entry.name)))
    }

    #[test]
    fn every_command_help_has_the_common_sections_in_order() {
        for entry in COMMANDS {
            let text = text(Some(entry.name));
            let operands = match entry.name {
                "search" => "\nquery:\n",
                "show" | "ls" => "\naddresses:\n",
                _ => "\noperands:\n",
            };
            let mut sections = vec!["usage: ", operands, "\noptions:\n"];
            if entry.name == "search" {
                sections.push("\ndefaults:\n");
            }
            sections.extend(["\ntext:\n", "\njson:\n", "\nexamples:\n"]);
            let at: Vec<Option<usize>> =
                sections.iter().map(|section| text.find(section)).collect();
            assert_eq!(at[0], Some(0), "{}", entry.name);
            assert!(at.iter().all(Option::is_some), "{}: {at:?}", entry.name);
            assert!(at.is_sorted(), "{}: {at:?}", entry.name);
        }
    }

    #[test]
    fn general_help_fits_50_lines() {
        assert!(text(None).lines().count() <= 50);
    }

    #[test]
    fn help_fits_79_columns() {
        for topic in topics() {
            for line in text(topic).lines() {
                assert!(line.chars().count() <= 79, "{topic:?}: {line}");
            }
        }
    }
}
