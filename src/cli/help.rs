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
  as "sego: note: ...". --json prints one JSON document instead;
  "sego COMMAND --help" lists its fields.

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
  results or 20 s; count:N and timeout:60s raise those limits. By default,
  Sourcegraph excludes archived repositories and forks; add archived:yes or
  fork:yes to include them. Files too large to search are skipped, and
  sego show reads any file; index:no reads files skipped for too many
  distinct trigrams.

text:
  N UNIT  R repos  T s      UNIT is matches, commits, files,
                            directories, symbols or repos; N+ when
                            more may exist; "stopped at T s" when the
                            search ran out of time
  query  QUERY              the query as sent
  widen  FILTER (N repos)   repositories in scope left out by default
  REPO@COMMIT:PATH          a file with results, then "N:text" for a
                            matching line and "N-text" for context
  A match is one matched range; a line can hold several.
  A diff result previews at most 5 files and 3 hunks a file.
  Text prints the first 100 matches; --json carries every result fetched.
  Notes on stderr name skipped files; at a pin, a 0 says it can't list them.

json:
  query pattern_type unit count at_least stopped elapsed_ms limit_ms
  widen[{filter repos}] unindexed[{repo commit path bytes}] alert
  results[]: kind (line, path, binary, symbol, commit or repo) and repo;
    file kinds add commit rev path; line adds line text match ranges
    chunk, with match false on a context line; symbol adds line name
    symbol_kind container; commit adds commit date author subject lines
    highlights preview_limited; date is the UTC day of the author date
  server

examples:
  sego search 'repo:^github\.com/gorilla/mux$ "func NewRouter"'
  sego search --json 'repo:^github\.com/gorilla/mux$ StrictSlash count:100'
"#;

const SHOW_FACTS: &str = r#"text:
  A file: REPO@COMMIT:PATH  lines A-B of N  rev REV, then a row per line,
  "N:text". Text shows lines 1-500 when LINES is not given, and 1- shows
  the whole file; --json carries the same lines.
  A commit: REPO@COMMIT  DATE  AUTHOR  parents P  N files  +A -D, then
  the message, indented, and a row per file, "STATUS  +A -D  PATH".

json:
  A file: repo rev commit path total_lines byte_size binary crlf
  lfs_size submodule start end lines[{line text}] via_raw
  A commit: repo rev commit author author_date committer committer_date
  parents subject message files[{status old_path new_path added deleted}]
  files_total added deleted

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
  REPO@COMMIT[:PATH]  N entries  D dirs  F files  S submodules  rev REV,
  then a row per entry, "KIND  PATH", with KIND dir, file or submodule.
  Paths are from the repository root, sorted segment by segment.
  Text prints 200 entries; --json carries every entry.

json:
  repo rev commit path recursive
  entries[{path kind submodule_url submodule_target submodule_commit}]

examples:
  sego ls github.com/gorilla/mux@v1.8.0
  sego ls --recursive github.com/gorilla/mux@v1.8.0:.github
"#;

const REFS_TEXT: &str = concat!(
    "  TEXT     only refs whose name contains TEXT, in any case: 1.8\n",
    "           matches v1.8.0, 1.8.0 and release-1.8 alike\n",
);

const REFS_FACTS: &str = r#"text:
  REPO  N branches  M tags, or with TEXT, "X of Y branches match" and
  "X of Y tags match", then a row per ref, "KIND  COMMIT  DATE  NAME",
  the default branch marked.
  Branches come default first, then newest first; tags newest first.
  Text prints 100 branches and 100 tags; --json carries up to 17000 of
  each.

json:
  repo query default_branch branches_total tags_total branches_all
  tags_all branches_more tags_more refs[{kind name commit date}]

examples:
  sego refs github.com/gorilla/mux 1.8
  sego refs --json github.com/gorilla/mux | jq -r '.refs[].name'
"#;

const LOG_OPERANDS: &str = "\
operands:
  REPO       host/owner/name, owner/name, or a GitHub, GitLab or
             Sourcegraph URL
  REPO@REV   commits reachable from REV, newest first; the default
             branch without @REV
  REPO@A..B  commits reachable from B and not from A, as git log A..B
  PATH       only commits that changed PATH (one path)
";

const LOG_FACTS: &str = r#"text:
  REPO@COMMIT[:PATH]  N commits  rev REV, then a row per commit,
  "COMMIT  DATE  AUTHOR  SUBJECT", with "(tag: NAME)" before the subject
  of a tagged commit.
  Text and JSON list the newest 150 commits of a REV; a range lists them
  all in JSON.

json:
  repo rev commit rev_a rev_b commit_a commit_b path total more
  commits[{commit date author subject parents tags}]; date is the UTC day
  of the author date

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
  REPO@REV    the change REV made, against its first parent
  PATH...     paths or directory prefixes; the file list, counts and patch
              cover only these
";

const DIFF_FACTS: &str = r#"text:
  REPO@A...B  N files  +A -D  base COMMIT  rev A...B, then a row per
  file, "STATUS  +A -D  PATH", with STATUS A, D, M or R (OLD -> NEW),
  then the patch of named paths, indented four spaces. One path joins
  the header as REPO@A...B:PATH; two or more add "paths P...".
  Text prints 200 file rows; --json carries every file.
  With PATH operands, a rename shows as R only when both its paths are
  named.

json:
  repo rev_a rev_b commit_a commit_b base files_total added deleted
  files[{status old_path new_path added deleted}]
  patch, unindented, or null without PATH

examples:
  sego diff github.com/gorilla/mux@v1.7.4...v1.8.0
  sego diff github.com/gorilla/mux@v1.7.4...v1.8.0 mux.go
"#;

/// A command's help: its usage, then `operands`, `options` and `facts`,
/// one blank line apart.
fn command_help(synopsis: &str, operands: &str, options: &str, facts: &str) -> String {
    format!("{}\n\n{operands}\n{options}\n{facts}", usage_block(synopsis))
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
    command_help(LOG_SYNOPSIS, LOG_OPERANDS, OPTIONS, LOG_FACTS)
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
sego searches code through Sourcegraph, for agents.

usage: {SYNOPSIS}

commands:
  search QUERY...                code matching a Sourcegraph query
  show REPO[@REV]:PATH [LINES]   lines of a file
  show REPO@REV                  a commit: its message and changed files
  ls REPO[@REV][:PATH]           entries of a directory
  refs REPO [TEXT]               branches and tags
  log REPO[@REV][:PATH] [PATH]   commits, newest first
  log REPO@A..B [PATH]           commits in B and not in A
  diff REPO@A...B [PATH...]      changed files, and the patch of named paths
  diff REPO@REV [PATH...]        the change one commit made

addresses:
{REPO}{REV}{PATH_AND_LINES}
{GENERAL_OUTPUT_AND_EXAMPLES}
{OPTIONS}
environment:
  SEGO_ENDPOINT    the Sourcegraph instance (default
                   https://sourcegraph.com)
  SEGO_CACHE_DIR   where sego caches large files, or off (default
                   $XDG_CACHE_HOME/sego or ~/.cache/sego)

exit codes: 0 answered, 1 not answered, 2 the command line is wrong
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
