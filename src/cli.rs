use std::ffi::OsString;

use lexopt::Arg;

use crate::address::{self, Address, LineRange, Repo, Rev};
use crate::error::{Error, usage_block};

pub const SYNOPSIS: &str = "sego COMMAND [--json] ...";

pub const SEARCH_SYNOPSIS: &str = "sego search [--json] QUERY...";
pub const SHOW_SYNOPSIS: &str = "\
sego show [--json] REPO[@REV]:PATH [LINES]\n\
sego show [--json] REPO@REV";
pub const LS_SYNOPSIS: &str =
    "sego ls [--json] [--recursive] REPO[@REV][:PATH]";
pub const REFS_SYNOPSIS: &str = "sego refs [--json] REPO [TEXT]";
pub const LOG_SYNOPSIS: &str = "\
sego log [--json] REPO[@REV][:PATH] [PATH]\n\
sego log [--json] REPO@A..B[:PATH] [PATH]";
pub const DIFF_SYNOPSIS: &str = "\
sego diff [--json] REPO@A...B[:PATH] [PATH...]\n\
sego diff [--json] REPO@REV[:PATH] [PATH...]";

/// A command word, its synopsis and the text of its `--help`.
pub struct CommandEntry {
    pub name: &'static str,
    pub synopsis: &'static str,
    pub help: fn() -> String,
}

/// Every command. Parsing, the commands fact, the synopsis lookup and
/// `help` all read this table, so a command that is listed has a help topic.
pub const COMMANDS: &[CommandEntry] = &[
    CommandEntry {
        name: "search",
        synopsis: SEARCH_SYNOPSIS,
        help: search_help,
    },
    CommandEntry { name: "show", synopsis: SHOW_SYNOPSIS, help: show_help },
    CommandEntry { name: "ls", synopsis: LS_SYNOPSIS, help: ls_help },
    CommandEntry { name: "refs", synopsis: REFS_SYNOPSIS, help: refs_help },
    CommandEntry { name: "log", synopsis: LOG_SYNOPSIS, help: log_help },
    CommandEntry { name: "diff", synopsis: DIFF_SYNOPSIS, help: diff_help },
];

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Search { query: String, json: bool },
    Show { address: Address, lines: Option<LineRange>, json: bool },
    Ls { address: Address, recursive: bool, json: bool },
    Refs { repo: Repo, text: Option<String>, json: bool },
    ShowCommit { address: Address, json: bool },
    Log { address: Address, path: Option<String>, json: bool },
    Diff { address: Address, paths: Vec<String>, json: bool },
    Help(Option<&'static str>),
    Version,
}

#[derive(Default)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "one independent switch per option"
)]
struct Flags {
    json: bool,
    recursive: bool,
    help: bool,
    version: bool,
}

/// `args` excludes the program name.
pub fn parse(args: &[OsString]) -> Result<Command, Error> {
    let args = utf8(args)?;
    let mut parser = lexopt::Parser::from_args(&args);
    let mut flags = Flags::default();
    let mut operands: Vec<String> = Vec::new();
    loop {
        let synopsis = synopsis_of(operands.first().map(String::as_str));
        if operands.first().is_some_and(|word| word == "search")
            && let Some(mut raw) = parser.try_raw_args()
            && let Some(token) = raw.next_if(is_query_dash)
        {
            operands.push(token.to_string_lossy().into_owned());
            continue;
        }
        let twice =
            |name: &str| Error::usage(format!("{name} given twice"), synopsis);
        let arg = match parser.next() {
            Ok(Some(arg)) => arg,
            Ok(None) => break,
            Err(lexopt::Error::UnexpectedValue { option, .. }) => {
                return Err(Error::usage(
                    format!("{option} takes no value"),
                    synopsis,
                ));
            }
            Err(other) => {
                return Err(Error::usage(other.to_string(), synopsis));
            }
        };
        match arg {
            Arg::Long("json") if flags.json => return Err(twice("--json")),
            Arg::Long("json") => flags.json = true,
            Arg::Long("recursive") if flags.recursive => {
                return Err(twice("--recursive"));
            }
            Arg::Long("recursive") => flags.recursive = true,
            Arg::Long("help") if flags.help => return Err(twice("--help")),
            Arg::Short('h') if flags.help => return Err(twice("-h")),
            Arg::Long("help") | Arg::Short('h') => flags.help = true,
            Arg::Long("version") if flags.version => {
                return Err(twice("--version"));
            }
            Arg::Long("version") => flags.version = true,
            Arg::Long(name) => {
                return Err(unknown(&format!("--{name}"), synopsis));
            }
            Arg::Short(c) => {
                return Err(unknown(&short_token(&args, c), synopsis));
            }
            Arg::Value(value) => {
                operands.push(value.to_string_lossy().into_owned());
            }
        }
    }
    let command = match operands.split_first() {
        Some((word, rest)) => {
            match COMMANDS.iter().find(|entry| entry.name == word) {
                Some(entry) => Some((entry.name, rest)),
                None => return Err(unknown_command(word)),
            }
        }
        None => None,
    };
    if flags.help {
        return Ok(Command::Help(command.map(|(topic, _)| topic)));
    }
    if flags.version {
        return Ok(Command::Version);
    }
    let Some((name, rest)) = command else {
        return Err(
            Error::usage("no command given", SYNOPSIS).fact(commands_fact())
        );
    };
    if flags.recursive && name != "ls" {
        return Err(Error::usage(
            format!("{name} takes no --recursive"),
            synopsis_of(Some(name)),
        ));
    }
    match name {
        "search" => parse_search(rest, flags.json),
        "show" => parse_show(rest, flags.json),
        "ls" => parse_ls(rest, flags.recursive, flags.json),
        "refs" => parse_refs(rest, flags.json),
        "log" => parse_log(rest, flags.json),
        "diff" => parse_diff(rest, flags.json),
        other => Err(unknown_command(other)),
    }
}

/// A token with exactly one leading dash, other than `-h`, is query text.
fn is_query_dash(token: &std::ffi::OsStr) -> bool {
    token.to_str().is_none_or(|text| {
        text.len() > 1
            && text.starts_with('-')
            && !text.starts_with("--")
            && text != "-h"
    })
}

fn parse_search(operands: &[String], json: bool) -> Result<Command, Error> {
    let query = operands.join(" ");
    if query.trim().is_empty() {
        return Err(Error::usage("search needs a query", SEARCH_SYNOPSIS));
    }
    Ok(Command::Search { query, json })
}

fn utf8(args: &[OsString]) -> Result<Vec<String>, Error> {
    args.iter()
        .enumerate()
        .map(|(i, arg)| {
            arg.to_str().map(str::to_owned).ok_or_else(|| {
                Error::usage(
                    format!("argument {} is not valid UTF-8", i + 1),
                    synopsis_before(&args[..i]),
                )
            })
        })
        .collect()
}

/// The synopsis of the command named among `earlier` arguments, which are
/// all valid UTF-8.
fn synopsis_before(earlier: &[OsString]) -> &'static str {
    let mut options_ended = false;
    for arg in earlier {
        let word = arg.to_string_lossy();
        if options_ended || !word.starts_with('-') {
            return synopsis_of(Some(&word));
        }
        options_ended = word == "--";
    }
    SYNOPSIS
}

fn synopsis_of(command: Option<&str>) -> &'static str {
    command
        .and_then(|word| COMMANDS.iter().find(|entry| entry.name == word))
        .map_or(SYNOPSIS, |entry| entry.synopsis)
}

fn commands_fact() -> String {
    let names: Vec<&str> = COMMANDS.iter().map(|entry| entry.name).collect();
    format!("commands: {}", names.join(", "))
}

fn unknown_command(word: &str) -> Error {
    Error::usage(format!("unknown command '{word}'"), SYNOPSIS)
        .fact(commands_fact())
}

/// lexopt splits `-json` into short options; the whole token is what the
/// caller typed.
fn short_token(args: &[String], c: char) -> String {
    args.iter()
        .take_while(|arg| *arg != "--")
        .find(|arg| {
            arg.strip_prefix('-')
                .is_some_and(|rest| !rest.starts_with('-') && rest.contains(c))
        })
        .cloned()
        .unwrap_or_else(|| format!("-{c}"))
}

fn unknown(option: &str, synopsis: &'static str) -> Error {
    let error = Error::usage(format!("unknown option {option}"), synopsis);
    let fact = match option {
        "--limit" | "--count" => {
            format!("sego has no {option}; count: is a query filter")
        }
        "--context" => "sego has no --context; search prints one line of \
                        context, show prints any range"
            .to_owned(),
        "--repo" | "--lang" | "--file" => {
            let filter = &option[2..];
            format!("sego has no {option}; {filter}: is a query filter")
        }
        _ => return error,
    };
    error.fact(fact)
}

fn parse_show(operands: &[String], json: bool) -> Result<Command, Error> {
    let usage = |message: String| Error::usage(message, SHOW_SYNOPSIS);
    let [token, rest @ ..] = operands else {
        return Err(usage(
            "show needs an address, like REPO[@REV]:PATH".to_owned(),
        ));
    };
    if rest.len() > 1 {
        return Err(usage(
            "show takes an address and at most one LINES operand".to_owned(),
        ));
    }
    let address = address::parse(token).map_err(|e| usage(e.to_string()))?;
    if let Some(message) = range_message(&address.rev) {
        return Err(usage(message));
    }
    match (&address.rev, address.path.as_deref()) {
        (Rev::Split(_), _) => {}
        (_, Some("")) => {
            return Err(usage(
                "show needs a file's path, like REPO[@REV]:PATH".to_owned(),
            ));
        }
        (Rev::One(_), None) => {
            if !rest.is_empty() || address.lines.is_some() {
                return Err(usage("a commit takes no LINES".to_owned()));
            }
            return Ok(Command::ShowCommit { address, json });
        }
        (_, None) => {
            return Err(usage(
                "show needs a file's path or a revision, like \
                 REPO[@REV]:PATH or REPO@REV"
                    .to_owned(),
            ));
        }
        (_, Some(_)) => {}
    }
    let operand = rest
        .first()
        .map(|text| address::parse_lines(text))
        .transpose()
        .map_err(|e| usage(e.to_string()))?;
    let lines = match (address.lines, operand) {
        (Some(url), Some(operand)) if url != operand => {
            return Err(usage(format!(
                "the URL's lines {} and the LINES operand {} disagree",
                url.display(),
                operand.display()
            )));
        }
        (url, operand) => url.or(operand),
    };
    Ok(Command::Show { address, lines, json })
}

fn range_message(rev: &Rev) -> Option<String> {
    let Rev::Range { base, head, dots } = rev else {
        return None;
    };
    Some(format!(
        "a range is only valid in log (A..B) and diff (A...B): \
         {base}{}{head}",
        dots.as_str()
    ))
}

fn parse_ls(
    operands: &[String],
    recursive: bool,
    json: bool,
) -> Result<Command, Error> {
    let usage = |message: String| Error::usage(message, LS_SYNOPSIS);
    let [token] = operands else {
        return Err(usage(if operands.is_empty() {
            "ls needs an address, like REPO[@REV][:PATH]".to_owned()
        } else {
            "ls takes one address".to_owned()
        }));
    };
    let address = address::parse(token).map_err(|e| usage(e.to_string()))?;
    if let Some(message) = range_message(&address.rev) {
        return Err(usage(message));
    }
    if address.lines.is_some() {
        return Err(usage("ls takes no LINES".to_owned()));
    }
    Ok(Command::Ls { address, recursive, json })
}

fn parse_refs(operands: &[String], json: bool) -> Result<Command, Error> {
    let usage = |message: String| Error::usage(message, REFS_SYNOPSIS);
    let [token, rest @ ..] = operands else {
        return Err(usage("refs needs a repository, like REPO".to_owned()));
    };
    if rest.len() > 1 {
        return Err(usage(
            "refs takes a repository and at most one TEXT".to_owned(),
        ));
    }
    let address = address::parse(token).map_err(|e| usage(e.to_string()))?;
    let has_path =
        address.path.as_deref().is_some_and(|path| !path.is_empty());
    if address.rev != Rev::Default || has_path {
        return Err(usage(format!(
            "refs names a repository, not a revision or path: {token}"
        )));
    }
    let text = rest.first().cloned();
    if text.as_deref().is_some_and(|text| text.trim().is_empty()) {
        return Err(usage("the TEXT operand is empty".to_owned()));
    }
    Ok(Command::Refs { repo: address.repo, text, json })
}

/// The checks `log` and `diff` share on their address; `command` names the
/// command in the messages.
fn history_address(
    token: &str,
    command: &str,
    synopsis: &'static str,
) -> Result<Address, Error> {
    let usage = |message: String| Error::usage(message, synopsis);
    let address = address::parse(token).map_err(|e| usage(e.to_string()))?;
    if matches!(address.rev, Rev::Split(_)) {
        return Err(usage(format!(
            "{command} can't split a branch or tag from the path in a tree \
             or blob URL"
        )));
    }
    if address.lines.is_some() {
        return Err(usage(format!("{command} takes no LINES")));
    }
    Ok(address)
}

/// The path an operand or address names, or `None` for the root.
fn named_path(path: &str) -> Option<String> {
    Some(address::parse_path(path)).filter(|path| !path.is_empty())
}

fn parse_log(operands: &[String], json: bool) -> Result<Command, Error> {
    let usage = |message: &str| Error::usage(message, LOG_SYNOPSIS);
    let [token, rest @ ..] = operands else {
        return Err(usage("log needs an address, like REPO[@REV][:PATH]"));
    };
    if rest.len() > 1 {
        return Err(usage("log takes an address and at most one PATH"));
    }
    let mut address = history_address(token, "log", LOG_SYNOPSIS)?;
    if let Rev::Range { base, head, dots: address::Dots::Three } = &address.rev
    {
        if address.kind != address::UrlKind::Compare {
            return Err(usage("log takes A..B, not A...B"));
        }
        address.rev = Rev::Range {
            base: base.clone(),
            head: head.clone(),
            dots: address::Dots::Two,
        };
    }
    if rest.iter().any(String::is_empty) {
        return Err(usage("log takes no empty path"));
    }
    let in_address = address.path.as_deref().and_then(named_path);
    let operand = rest.first().and_then(|path| named_path(path));
    let path = match (in_address, operand) {
        (Some(_), Some(_)) => return Err(usage("log takes one path")),
        (path, None) | (None, path) => path,
    };
    Ok(Command::Log { address, path, json })
}

fn parse_diff(operands: &[String], json: bool) -> Result<Command, Error> {
    let usage = |message: &str| Error::usage(message, DIFF_SYNOPSIS);
    let [token, rest @ ..] = operands else {
        return Err(usage(
            "diff needs an address, like REPO@A...B or REPO@REV",
        ));
    };
    let address = history_address(token, "diff", DIFF_SYNOPSIS)?;
    match address.rev {
        Rev::Range { dots: address::Dots::Two, .. } => {
            return Err(usage(
                "diff takes A...B (Sourcegraph compares from the merge base)",
            ));
        }
        Rev::Default => {
            return Err(usage(
                "diff needs a revision or a range, like REPO@REV or \
                 REPO@A...B",
            ));
        }
        _ => {}
    }
    if rest.iter().any(String::is_empty) {
        return Err(usage("diff takes no empty path"));
    }
    let mut paths: Vec<String> = Vec::new();
    for path in address.path.iter().chain(rest).filter_map(|p| named_path(p)) {
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
    Ok(Command::Diff { address, paths, json })
}

// A string continued with `\` drops the next line's leading spaces, so a
// block whose first line is indented is joined with `concat!`.
const REPO: &str = concat!(
    "  REPO     host/owner/name, owner/name, or a GitHub, GitLab or\n",
    "           Sourcegraph URL\n",
);

const REV: &str =
    "  @REV     a branch, tag or commit; the default branch without it\n";

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
fn command_help(
    synopsis: &str,
    operands: &str,
    options: &str,
    facts: &str,
) -> String {
    format!("{}\n\n{operands}\n{options}\n{facts}", usage_block(synopsis))
}

fn search_help() -> String {
    command_help(SEARCH_SYNOPSIS, SEARCH_QUERY, OPTIONS, SEARCH_FACTS)
}

fn show_help() -> String {
    let addresses = format!(
        "addresses:\n{REPO}{REV}  @REV     with no :PATH, the commit: its \
         message and changed files\n{PATH_AND_LINES}"
    );
    command_help(SHOW_SYNOPSIS, &addresses, OPTIONS, SHOW_FACTS)
}

fn ls_help() -> String {
    let addresses = format!(
        "addresses:\n{REPO}{REV}  :PATH    a directory from the repository \
         root; the root without it\n"
    );
    command_help(LS_SYNOPSIS, &addresses, LS_OPTIONS, LS_FACTS)
}

fn refs_help() -> String {
    let operands = format!("operands:\n{REPO}{REFS_TEXT}");
    command_help(REFS_SYNOPSIS, &operands, OPTIONS, REFS_FACTS)
}

fn log_help() -> String {
    command_help(LOG_SYNOPSIS, LOG_OPERANDS, OPTIONS, LOG_FACTS)
}

fn diff_help() -> String {
    command_help(DIFF_SYNOPSIS, DIFF_OPERANDS, OPTIONS, DIFF_FACTS)
}

pub fn help(topic: Option<&str>) -> String {
    if let Some(entry) =
        topic.and_then(|word| COMMANDS.iter().find(|e| e.name == word))
    {
        return (entry.help)();
    }
    format!(
        "\
sego searches and reads public source code through Sourcegraph code search.

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
