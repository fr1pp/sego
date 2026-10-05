pub mod blame;
pub mod compare;
pub mod list;
pub mod read;
pub mod search;

use std::fmt::{self, Write as _};

use crate::args::{Cli, ParseError};
use crate::diagnostic::{Diagnostic, Outcome};
use crate::escape::shell_word;
use crate::sourcegraph::Fetch;

pub const COMMANDS: &[&str] = &["search", "list", "read", "blame", "compare"];

/// A command's help, split around its synopsis, which usage errors show
/// too.
pub struct Help {
    /// What the command does: the paragraph before `Usage:`.
    pub about: &'static str,
    /// The synopsis after `sego <command> `.
    pub synopsis: &'static str,
    /// Everything after the synopsis.
    pub details: &'static str,
}

const ABOUT: &str = "\
sego finds and inspects public source code at any revision, through
Sourcegraph's GraphQL API.
";

const DETAILS: &str = "\
Commands:
  search   where is it? Matching code, paths, symbols, commits and repos
  list     what is here? The entries of a directory
  read     what does it say? The lines of a file
  blame    why, and when? The commits that last changed a file's lines
  compare  what changed? The commits and files changed in a range or a commit

Options:
  --json       write the outcome as one JSON document on stdout
  --depth N    list only: entries down to depth N, at least 1 (default 1)
  --help       print this help, or a command's help after its name
  --version    print sego's version

Options may appear anywhere before --; every other token is an operand, so
search negations such as -file:_test need no escaping.

Output:
  An answer goes to stdout; anything else is reported on stderr. With
  --json, a non-answer's document also goes to stdout.

Environment:
  SEGO_ENDPOINT   the Sourcegraph instance (default https://sourcegraph.com)

Exit codes:
  0  answered      use it
  1  answered no   ask something else
  2  invalid       fix the command
  3  unanswered    retry, or narrow and retry
";

const VERSION: &str = concat!("sego ", env!("CARGO_PKG_VERSION"), "\n");

/// The top-level help, with every command's synopsis.
fn help() -> String {
    let mut usage = String::new();
    for line in synopses() {
        let _ = writeln!(usage, "  {line}");
    }
    format!("{ABOUT}\nUsage:\n{usage}  sego <command> --help\n\n{DETAILS}")
}

/// The synopsis a usage error shows: the line of `command`, or every
/// command's line when there is none.
fn synopsis(command: Option<Command>) -> Vec<String> {
    match command {
        Some(command) => vec![command.synopsis(0)],
        None => synopses(),
    }
}

/// Every command's synopsis, in the order of `COMMANDS`, with the names
/// padded to the longest so the operands line up.
fn synopses() -> Vec<String> {
    let width = COMMANDS.iter().map(|name| name.len()).max().unwrap_or(0);
    COMMANDS
        .iter()
        .filter_map(|&name| Command::parse(name))
        .map(|command| command.synopsis(width))
        .collect()
}

/// An answer, ready to print.
pub trait Present {
    fn outcome(&self) -> Outcome;
    fn text(&self) -> String;
    fn json(&self) -> String;
}

/// Help or the version: the same text with or without `--json`.
pub struct Plain(String);

impl Present for Plain {
    fn outcome(&self) -> Outcome {
        Outcome::Answered
    }

    fn text(&self) -> String {
        self.0.clone()
    }

    fn json(&self) -> String {
        self.0.clone()
    }
}

/// A command sego runs.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Search,
    List,
    Read,
    Blame,
    Compare,
}

impl Command {
    fn parse(name: &str) -> Option<Command> {
        match name {
            "search" => Some(Command::Search),
            "list" => Some(Command::List),
            "read" => Some(Command::Read),
            "blame" => Some(Command::Blame),
            "compare" => Some(Command::Compare),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Command::Search => "search",
            Command::List => "list",
            Command::Read => "read",
            Command::Blame => "blame",
            Command::Compare => "compare",
        }
    }

    fn parts(self) -> &'static Help {
        match self {
            Command::Search => &search::HELP,
            Command::List => &list::HELP,
            Command::Read => &read::HELP,
            Command::Blame => &blame::HELP,
            Command::Compare => &compare::HELP,
        }
    }

    /// `sego <name> <synopsis>`, the name padded to at least `width`
    /// columns.
    fn synopsis(self, width: usize) -> String {
        format!("sego {:width$} {}", self.name(), self.parts().synopsis)
    }

    fn help(self) -> String {
        let Help { about, details, .. } = self.parts();
        format!("{about}\nUsage:\n  {}\n\n{details}", self.synopsis(0))
    }

    /// Checks `cli`'s operands as far as possible without fetching
    /// anything.
    pub fn validate(self, cli: &Cli) -> Result<(), Box<dyn Diagnostic>> {
        match self {
            Command::Search => {
                search::request(cli)?;
            }
            Command::List => {
                list::request(cli)?;
            }
            Command::Read => {
                read::request(cli)?;
            }
            Command::Blame => {
                blame::request(cli)?;
            }
            Command::Compare => {
                compare::request(cli)?;
            }
        }
        Ok(())
    }

    pub fn run(
        self,
        cli: &Cli,
        fetch: Fetch,
    ) -> Result<Box<dyn Present>, Box<dyn Diagnostic>> {
        match self {
            Command::Search => search::run(cli, fetch),
            Command::List => list::run(cli, fetch),
            Command::Read => read::run(cli, fetch),
            Command::Blame => blame::run(cli, fetch),
            Command::Compare => compare::run(cli, fetch),
        }
    }
}

/// An invocation that can't run as written, and the synopsis it should
/// have followed.
#[derive(Debug)]
pub struct UsageError {
    message: String,
    usage: Vec<String>,
}

impl UsageError {
    /// An error in an invocation of `command`, or of sego when there is no
    /// command.
    fn new(message: impl Into<String>, command: Option<Command>) -> Self {
        UsageError { message: message.into(), usage: synopsis(command) }
    }
}

/// An unknown or absent command name gives every synopsis.
impl From<ParseError> for UsageError {
    fn from(ParseError { message, command }: ParseError) -> Self {
        UsageError::new(message, command.as_deref().and_then(Command::parse))
    }
}

impl fmt::Display for UsageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl Diagnostic for UsageError {
    fn code(&self) -> &'static str {
        "usage"
    }

    fn outcome(&self) -> Outcome {
        Outcome::Invalid
    }

    fn usage(&self) -> Vec<String> {
        self.usage.clone()
    }
}

/// What an invocation asks for.
pub enum Action {
    /// Help or the version, which need no upstream.
    Print(Plain),
    Run(Command),
}

/// What `cli` asks for.
pub fn action(cli: &Cli) -> Result<Action, UsageError> {
    let command = match cli.command.as_deref() {
        Some(name) => match Command::parse(name) {
            Some(command) => Some(command),
            None => {
                return Err(UsageError::new(
                    format!("unknown command {}", shell_word(name)),
                    None,
                ));
            }
        },
        None => None,
    };
    if let Some(command) = command
        && command != Command::List
        && cli.depth.is_some()
    {
        let message = "--depth applies only to list";
        return Err(UsageError::new(message, Some(command)));
    }
    if cli.help {
        let text = command.map_or_else(help, Command::help);
        Ok(Action::Print(Plain(text)))
    } else if cli.version {
        Ok(Action::Print(Plain(VERSION.to_owned())))
    } else {
        match command {
            Some(command) => Ok(Action::Run(command)),
            None => Err(UsageError::new(
                "missing command; expected search, list, read, blame or \
                 compare",
                None,
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::io::{self, Write};
    use std::process;

    use expect_test::expect;
    use serde_json::Value;

    use super::*;
    use crate::address::{CommitId, Pinned, Range, RepoName};
    use crate::args;
    use crate::diagnostic::report;
    use crate::domain::{
        self, Alert, Author, CommitCount, CommitDetail, CommitHit,
        CommitSummary, Comparison, CutShort, DiffHunk, Entry, EntryKind, File,
        FileChange, FileHit, FileStatus, Filter, Hit, Hunk, Line,
        LinesCommand, Listing, NotAFile, RepoHit, SearchReport, Side, Skipped,
        Span, Submodule, Symbol, Term, Tree, Unavailable,
    };
    use crate::escape::{CORPUS, is_unsafe};
    use crate::output::Output;
    use crate::render;
    use crate::sourcegraph::FetchError;
    use crate::sourcegraph::blame::timed_out;
    use crate::sourcegraph::search::SearchProblem;
    use crate::sourcegraph::transport::{InvalidEndpoint, Transport};
    use crate::sourcegraph::vocabulary::ALERT_BAD_QUERY;

    /// Spec §3.4: the exit code of each diagnostic code sego has.
    const EXIT_CODES: &[(&str, u8)] = &[
        ("repo_not_found", 1),
        ("revision_not_found", 1),
        ("path_not_found", 1),
        ("not_a_file", 1),
        ("not_a_directory", 1),
        ("too_large", 1),
        ("out_of_range", 1),
        ("no_parent", 1),
        ("usage", 2),
        ("bad_query", 2),
        ("incomplete", 3),
        ("transport", 3),
        ("bad_response", 3),
        ("server_error", 3),
        ("schema_mismatch", 3),
        ("output", 3),
    ];

    /// Checks `cli` as main does before it builds the transport.
    fn validate(cli: &Cli) -> Result<(), Box<dyn Diagnostic>> {
        match action(cli)? {
            Action::Print(_) => Ok(()),
            Action::Run(command) => command.validate(cli),
        }
    }

    /// The commands that synopsis `lines` are for.
    fn commands(lines: &[String]) -> String {
        let names = lines.iter().filter_map(|line| {
            line.strip_prefix("sego ")?.split_whitespace().next()
        });
        names.collect::<Vec<_>>().join(" ")
    }

    #[test]
    fn every_command_name_parses() {
        for &name in COMMANDS {
            assert!(Command::parse(name).is_some(), "{name}");
        }
    }

    #[test]
    fn synopses_align_only_when_there_are_several() {
        let commands =
            ["search", "list", "read", "blame", "compare"].map(Some);
        let mut text = String::new();
        for command in commands.into_iter().chain([None, Some("serch")]) {
            for line in synopsis(command.and_then(Command::parse)) {
                text.push_str(&line);
                text.push('\n');
            }
        }
        expect![[r#"
            sego search [--json] <query>...
            sego list [--json] [--depth N] <repo>[@<rev>] [<path>]
            sego read [--json] <repo>[@<rev>] <path> [<lines>]
            sego blame [--json] <repo>[@<rev>] <path> <lines>
            sego compare [--json] <repo>@[<base>...]<head> [<path>]
            sego search  [--json] <query>...
            sego list    [--json] [--depth N] <repo>[@<rev>] [<path>]
            sego read    [--json] <repo>[@<rev>] <path> [<lines>]
            sego blame   [--json] <repo>[@<rev>] <path> <lines>
            sego compare [--json] <repo>@[<base>...]<head> [<path>]
            sego search  [--json] <query>...
            sego list    [--json] [--depth N] <repo>[@<rev>] [<path>]
            sego read    [--json] <repo>[@<rev>] <path> [<lines>]
            sego blame   [--json] <repo>[@<rev>] <path> <lines>
            sego compare [--json] <repo>@[<base>...]<head> [<path>]
        "#]]
        .assert_eq(&text);
    }

    #[test]
    fn usage_errors_show_the_synopsis_of_the_command() {
        let invocations: [&[&str]; 8] = [
            &["serch", "x"],
            &["serch"],
            &["serch", "x", "--depth", "2"],
            &["serch", "--help", "--depth", "2"],
            &["read", "x/y", "a.go", "--depth", "2"],
            &["read", "x/y", "--depth", "2"],
            &["search", "x", "--depth", "2"],
            &["--depth", "2"],
        ];
        let errors: Vec<String> = invocations
            .iter()
            .map(|argv| {
                let argv = argv.iter().map(OsString::from);
                let cli = args::parse(argv).unwrap();
                let error = validate(&cli).err().unwrap();
                assert_eq!(error.hints(), Vec::<String>::new());
                format!("{error} | {}\n", commands(&error.usage()))
            })
            .collect();
        expect![[r#"
            unknown command serch | search list read blame compare
            unknown command serch | search list read blame compare
            unknown command serch | search list read blame compare
            unknown command serch | search list read blame compare
            --depth applies only to list | read
            --depth applies only to list | read
            --depth applies only to list | search
            missing command; expected search, list, read, blame or compare | search list read blame compare
        "#]]
        .assert_eq(&errors.concat());
    }

    #[test]
    fn parse_errors_show_the_synopsis_of_the_command_read_so_far() {
        let invocations: &[&[&str]] = &[
            &["read", "x", "a", "--jsno"],
            &["--jsno", "read", "x"],
            &["read", "x/y", "--weird.go", "1-2"],
            &["search", "--weird", "x"],
            &["search", "--a b"],
            &["search", "--=x"],
            &["list", "x", "--depth"],
            &["list", "x", "--depth", "0"],
            &["read", "x", "--depth=x"],
            &["--depth", "99999999999", "list", "x"],
            &["--json=1", "search", "x"],
            &["search", "x", "--help=x"],
        ];
        let errors: Vec<String> = invocations
            .iter()
            .map(|argv| {
                let argv = argv.iter().map(OsString::from);
                let error = UsageError::from(args::parse(argv).unwrap_err());
                assert_eq!(error.hints(), Vec::<String>::new());
                format!("{error} | {}\n", commands(&error.usage()))
            })
            .collect();
        expect![[r#"
            unknown option --jsno | read
            unknown option --jsno | search list read blame compare
            unknown option --weird.go | read
            unknown option --weird | search
            unknown option '--a b' | search
            unknown option '--' | search
            --depth needs a value | list
            invalid --depth 0; expected a number from 1 to 4294967295 | list
            invalid --depth x; expected a number from 1 to 4294967295 | read
            invalid --depth 99999999999; expected a number from 1 to 4294967295 | search list read blame compare
            --json takes no value | search list read blame compare
            --help takes no value | search
        "#]]
        .assert_eq(&errors.concat());
    }

    /// The corpus without the empty string, which is never a file's path
    /// or a query.
    fn words() -> impl Iterator<Item = &'static str> {
        CORPUS.iter().copied().filter(|word| !word.is_empty())
    }

    /// Every word of the corpus, and a blank one.
    fn blank_and_corpus() -> Vec<String> {
        let mut all = vec![" ".to_owned()];
        all.extend(CORPUS.iter().map(|&word| word.to_owned()));
        all
    }

    fn at() -> Pinned {
        Pinned {
            repo: RepoName::parse("github.com/gorilla/mux").unwrap(),
            commit: CommitId(
                "a7962380ca08b5a188038c69871b8d3fbdf31e89".into(),
            ),
        }
    }

    /// The diagnostic sego reports for `argv`, which must be invalid.
    fn usage(argv: &[OsString]) -> Box<dyn Diagnostic> {
        match args::parse(argv.to_vec()) {
            Err(error) => Box::new(UsageError::from(error)),
            Ok(cli) => match validate(&cli) {
                Err(diagnostic) => diagnostic,
                Ok(()) => panic!("{argv:?} is valid"),
            },
        }
    }

    #[test]
    fn each_help_shows_the_synopsis_its_usage_errors_show() {
        for &name in COMMANDS {
            let command = Command::parse(name).unwrap();
            let lines = usage(&[name.into()]).usage();
            let [line] = <[String; 1]>::try_from(lines)
                .unwrap_or_else(|lines| panic!("{lines:?}"));
            let block = format!("\nUsage:\n  {line}\n\n");
            assert!(command.help().contains(&block), "{name}");
        }
        let mut lines = String::new();
        for line in usage(&[]).usage() {
            let _ = writeln!(lines, "  {line}");
        }
        assert!(help().contains(&format!("\nUsage:\n{lines}")));
    }

    fn usage_errors() -> Vec<Box<dyn Diagnostic>> {
        let invalid: &[&[&str]] = &[
            &[],
            &["frobnicate"],
            &["serch"],
            &["serch", "x"],
            &["serch", "x", "--depth", "2"],
            &["serch", "--help", "--depth", "2"],
            &["--jsno"],
            &["--verison"],
            &["read", "x/y", "a.go", "--jsno"],
            &["read", "x/y", "--jsno"],
            &["read", "x/y", "--weird.go", "1-2", "--json"],
            &["search", "x", "--dpeth", "2"],
            &["list", "x/y", "--depht=0"],
            &["--json=1"],
            &["list", "x/y", "--depth"],
            &["list", "x/y", "--depth", "0"],
            &["read", "x/y", "--depth", "2"],
            &["search", "x", "--depth", "2"],
            &["search"],
            &["search", " "],
            &["list"],
            &["list", "x/y", "a", "b"],
            &["list", "x/y@"],
            &["read", "x/y"],
            &["read", "x/y", "a", "1", "2"],
            &["read", "x/y@a..b", "a"],
            &["read", "x/y", "/"],
            &["read", "x/y", "a", "0"],
            &["read", "x/y", "a", "5-1"],
            &["read", "x/y", "a", "z"],
            &["blame", "x/y", "a"],
            &["blame", "x/y", "a", "1", "2"],
            &["blame", "x/y", "/", "1"],
            &["blame", "x/y", "a", "0"],
            &["blame", "x/y", "a", "5-1"],
            &["blame", "x/y", "a", "1", "--depth", "2"],
            &["compare"],
            &["compare", "x/y"],
            &["compare", "x/y@"],
            &["compare", "x/y@a..b"],
            &["compare", "x/y@...b"],
            &["compare", "x/y@a..."],
            &["compare", "x/y@a...b", "c", "d"],
            &["compare", "x/y@a", "--depth", "2"],
        ];
        let mut argvs: Vec<Vec<OsString>> = invalid
            .iter()
            .map(|argv| argv.iter().map(OsString::from).collect())
            .collect();
        for &word in CORPUS {
            argvs.extend([
                vec!["serch".into(), word.into()],
                vec!["serch".into(), "--".into(), word.into()],
                vec!["read".into(), word.into(), "--depth=2".into()],
            ]);
        }
        for word in words() {
            let option = || OsString::from(format!("--{word}"));
            argvs.extend([
                vec!["list".into(), "x/y".into(), option()],
                vec!["read".into(), "x/y".into(), option(), "1".into()],
                vec!["compare".into(), "x/y@a...b".into(), option()],
            ]);
        }
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt as _;

            let bad = OsString::from_vec(vec![0x66, 0xff]);
            argvs.push(vec!["read".into(), bad]);
        }
        argvs.iter().map(Vec::as_slice).map(usage).collect()
    }

    fn fetch_errors() -> Vec<FetchError> {
        let mut all = Vec::new();
        for &word in CORPUS {
            let text = || word.to_owned();
            all.extend([
                FetchError::Transport {
                    url: text(),
                    cause: text(),
                    timed_out: false,
                    trace: Some(text()),
                },
                FetchError::Transport {
                    url: text(),
                    cause: text(),
                    timed_out: true,
                    trace: None,
                },
                FetchError::Status {
                    status: 302,
                    excerpt: text(),
                    location: Some(text()),
                    trace: Some(text()),
                },
                FetchError::Status {
                    status: 502,
                    excerpt: format!("{word}\n{word}"),
                    location: None,
                    trace: None,
                },
                FetchError::NotJson { excerpt: text(), trace: Some(text()) },
                FetchError::Oversized { trace: Some(text()) },
                FetchError::Empty { trace: Some(text()) },
                FetchError::Field { cause: text(), trace: None },
                FetchError::Server {
                    errors: vec![text(), text()],
                    trace: None,
                },
                FetchError::Schema { errors: vec![text()], trace: None },
            ]);
        }
        all
    }

    fn unavailable() -> Vec<Unavailable> {
        let mut all = Vec::new();
        for word in words() {
            let path = || word.to_owned();
            all.extend([
                Unavailable::RepoNotFound {
                    repo: "github.com/gorilla/nope".into(),
                },
                Unavailable::RevisionNotFound {
                    repo: "github.com/gorilla/mux".into(),
                    rev: path(),
                    side: None,
                },
                Unavailable::RevisionNotFound {
                    repo: "github.com/gorilla/mux".into(),
                    rev: path(),
                    side: Some(Side::Base),
                },
                Unavailable::RevisionNotFound {
                    repo: "github.com/gorilla/mux".into(),
                    rev: path(),
                    side: Some(Side::Head),
                },
                Unavailable::RevisionNotFound {
                    repo: "github.com/gorilla/mux".into(),
                    rev: format!("{word}...{word}"),
                    side: Some(Side::Both),
                },
                Unavailable::PathNotFound { at: at(), path: path() },
                Unavailable::PathNotFound {
                    at: at(),
                    path: format!("{word}/{word}"),
                },
                Unavailable::NotAFile {
                    at: at(),
                    path: path(),
                    what: NotAFile::Directory,
                },
                Unavailable::NotAFile {
                    at: at(),
                    path: path(),
                    what: NotAFile::Submodule,
                },
                Unavailable::NotADirectory { at: at(), path: path() },
                Unavailable::TooLarge {
                    at: at(),
                    path: path(),
                    byte_size: 3_057_771,
                },
                Unavailable::OutOfRange {
                    at: at(),
                    path: path(),
                    start: 700,
                    total_lines: 601,
                    command: LinesCommand::Read,
                },
                Unavailable::OutOfRange {
                    at: at(),
                    path: path(),
                    start: 2,
                    total_lines: 0,
                    command: LinesCommand::Read,
                },
                Unavailable::OutOfRange {
                    at: at(),
                    path: path(),
                    start: 700,
                    total_lines: 601,
                    command: LinesCommand::Blame,
                },
                Unavailable::NoParent { at: at() },
            ]);
        }
        all
    }

    fn search_problems() -> Vec<SearchProblem> {
        let mut all = Vec::new();
        for word in words() {
            all.extend([
                SearchProblem::BadQuery {
                    alert: Alert {
                        title: ALERT_BAD_QUERY.to_owned(),
                        description: Some(word.to_owned()),
                        proposed: blank_and_corpus(),
                    },
                },
                SearchProblem::Incomplete {
                    query: format!("{word} timeout:30s"),
                    reason: word.to_owned(),
                    trace: Some(word.to_owned()),
                },
                SearchProblem::Incomplete {
                    query: format!("{word} timeout:2m"),
                    reason: word.to_owned(),
                    trace: None,
                },
                SearchProblem::Incomplete {
                    query: word.to_owned(),
                    reason: word.to_owned(),
                    trace: None,
                },
            ]);
        }
        all
    }

    /// `$names`, the names of `$enum`'s variants, and `$variant`, a
    /// value's variant name, from one list. `$variant`'s match is
    /// exhaustive, so a new variant doesn't compile until it is listed,
    /// which also puts it in `$names`; `every_variant_has_a_fixture` then
    /// fails until the fixtures have one.
    macro_rules! variants {
        (
            $names:ident,
            $variant:ident,
            $enum:ident { $($v:ident),+ $(,)? }
        ) => {
            const $names: &[&str] =
                &[$(concat!(stringify!($enum), "::", stringify!($v))),+];

            fn $variant(value: &$enum) -> &'static str {
                match value {
                    $($enum::$v { .. } => {
                        concat!(stringify!($enum), "::", stringify!($v))
                    })+
                }
            }
        };
    }

    variants!(
        FETCH_ERRORS,
        fetch_error_variant,
        FetchError {
            Transport,
            Status,
            NotJson,
            Oversized,
            Empty,
            Field,
            Server,
            Schema,
        }
    );

    variants!(
        UNAVAILABLE,
        unavailable_variant,
        Unavailable {
            RepoNotFound,
            RevisionNotFound,
            PathNotFound,
            NotAFile,
            NotADirectory,
            TooLarge,
            OutOfRange,
            NoParent,
        }
    );

    variants!(
        SEARCH_PROBLEMS,
        search_problem_variant,
        SearchProblem { BadQuery, Incomplete }
    );

    #[test]
    fn every_variant_has_a_fixture() {
        let mut seen: Vec<&str> = fetch_errors()
            .iter()
            .map(fetch_error_variant)
            .chain(unavailable().iter().map(unavailable_variant))
            .chain(search_problems().iter().map(search_problem_variant))
            .collect();
        seen.sort_unstable();
        seen.dedup();
        let mut expected =
            [FETCH_ERRORS, UNAVAILABLE, SEARCH_PROBLEMS].concat();
        expected.sort_unstable();
        assert_eq!(seen, expected);
    }

    /// Writes nothing, failing with its message.
    struct Failing(String);

    impl Write for Failing {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::Error::other(self.0.clone()))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    /// One instance of every diagnostic variant, over the corpus.
    fn diagnostics() -> Vec<Box<dyn Diagnostic>> {
        let mut all = usage_errors();
        all.extend(other_diagnostics());
        all
    }

    /// Every diagnostic variant but usage errors, over the corpus.
    fn other_diagnostics() -> Vec<Box<dyn Diagnostic>> {
        let mut all: Vec<Box<dyn Diagnostic>> = Vec::new();
        all.extend(fetch_errors().into_iter().map(Box::from));
        all.extend(unavailable().into_iter().map(Box::from));
        all.extend(search_problems().into_iter().map(Box::from));
        all.push(Box::new(InvalidEndpoint::NotUtf8));
        for &word in CORPUS {
            all.push(timed_out(FetchError::Transport {
                url: word.to_owned(),
                cause: word.to_owned(),
                timed_out: true,
                trace: Some(word.to_owned()),
            }));
            let endpoint = Transport::new(word).err().unwrap();
            all.push(Box::new(endpoint));
            let mut output = Output::new(Failing(word.to_owned()));
            output.write("x").unwrap();
            all.push(Box::new(output.flush().unwrap_err()));
        }
        all
    }

    fn search_report(
        word: &str,
        hits: Vec<Hit>,
        retry: Option<String>,
    ) -> SearchReport {
        let text = || word.to_owned();
        let filter = |kind| Filter {
            kind: Term { value: kind, raw: None },
            value: format!("{kind}:{word}"),
            count: 1,
            limit_hit: false,
        };
        SearchReport {
            query: text(),
            match_count: 1,
            approximate: Some(text()),
            limit_hit: true,
            skipped: Skipped {
                timed_out: vec![text()],
                cloning: vec![text()],
                missing: vec![text()],
            },
            cut_short: Some(CutShort { reasons: vec![text()], retry }),
            filters: vec![
                filter("repo"),
                filter("utility"),
                Filter {
                    kind: Term { value: "other", raw: Some(text()) },
                    value: text(),
                    count: 1,
                    limit_hit: false,
                },
            ],
            alert: Some(Alert {
                title: text(),
                description: Some(text()),
                proposed: blank_and_corpus(),
            }),
            hits,
        }
    }

    fn hits(word: &str) -> Vec<Hit> {
        let text = || word.to_owned();
        let line = |number, spans| Line { number, text: text(), spans };
        let symbol = Symbol {
            line: 1,
            kind: "function",
            name: text(),
            container: Some(text()),
        };
        vec![
            Hit::File(FileHit {
                at: at(),
                path: text(),
                path_match: true,
                binary: false,
                lines: vec![
                    line(1, vec![Span { start: 0, end: 1 }]),
                    line(3, Vec::new()),
                ],
                symbols: vec![symbol],
            }),
            Hit::Directory { at: at(), path: text() },
            Hit::Commit(CommitHit {
                at: at(),
                date: text(),
                author: text(),
                subject: text(),
                diff: Some(format!("{word}\n{word}")),
            }),
            Hit::Commit(CommitHit {
                at: at(),
                date: text(),
                author: text(),
                subject: text(),
                diff: None,
            }),
            Hit::Repository(RepoHit {
                repo: RepoName::parse("github.com/gorilla/mux").unwrap(),
                description: text(),
                stars: 1,
            }),
        ]
    }

    /// Blames of the corpus: one with its lines, in two hunks, the first
    /// of one line and the second of the rest; one over the read cap, in
    /// one hunk without lines; and one of a binary file, with no hunks.
    fn blames(word: &str) -> Vec<domain::Blame> {
        let text = || word.to_owned();
        let end = u32::try_from(CORPUS.len()).unwrap();
        let hunk = |start, end| Hunk {
            start,
            end,
            commit: at().commit,
            path: text(),
            date: text(),
            author: Author { name: text(), email: text() },
            subject: text(),
        };
        let lines = CORPUS.iter().map(|&line| line.to_owned()).collect();
        let blame = |hunks, lines, binary| domain::Blame {
            at: at(),
            path: text(),
            byte_size: 1,
            total_lines: end,
            binary,
            start: 1,
            end,
            hunks,
            lines,
        };
        vec![
            blame(vec![hunk(1, 1), hunk(2, end)], Some(lines), false),
            blame(vec![hunk(1, end)], None, false),
            blame(Vec::new(), Some(Vec::new()), true),
        ]
    }

    /// Comparisons of the corpus: a range narrowed to a file, with a file
    /// of each status, a hunk of the corpus and both lists cut; a single
    /// commit with the corpus as its message; and a range with no commits,
    /// whose reverse has some.
    fn comparisons(word: &str) -> Vec<Comparison> {
        let text = || word.to_owned();
        let file = |status, old_path, readable_at| FileChange {
            path: text(),
            old_path,
            status,
            binary: status == FileStatus::Modified,
            added: 1,
            deleted: 1,
            readable_at,
        };
        let lines = CORPUS.iter().map(|&line| format!("+{line}")).collect();
        let comparison = |commit, ahead, behind| Comparison {
            repo: at().repo,
            base: Some(at().commit),
            head: at().commit,
            commit,
            ahead,
            behind,
            commits: vec![CommitSummary {
                commit: at().commit,
                date: text(),
                author: text(),
                subject: text(),
            }],
            commit_count: CommitCount::MoreThan(1),
            files_changed: 5,
            added: 1,
            deleted: 1,
            files: vec![
                file(FileStatus::Added, None, Some(at().commit)),
                file(FileStatus::Deleted, None, Some(at().commit)),
                file(FileStatus::Deleted, None, None),
                file(FileStatus::Renamed, Some(text()), Some(at().commit)),
                file(FileStatus::Modified, None, Some(at().commit)),
            ],
            files_truncated: true,
            path: Some(text()),
            path_is_file: true,
            hunks: Vec::new(),
        };
        let range = Comparison {
            hunks: vec![DiffHunk {
                old_start: 1,
                old_lines: 0,
                new_start: 1,
                new_lines: u32::try_from(CORPUS.len()).unwrap(),
                section: Some(text()),
                lines,
                no_newline_old: true,
                no_newline_new: true,
            }],
            ..comparison(None, 2, 0)
        };
        let detail = CommitDetail {
            date: text(),
            author: text(),
            message: CORPUS.join("\n"),
            parents: vec![at().commit, at().commit],
        };
        let single = Comparison {
            base: None,
            commits: Vec::new(),
            commit_count: CommitCount::Exact(1),
            ..comparison(Some(detail), 1, 0)
        };
        let empty = Comparison {
            commits: Vec::new(),
            commit_count: CommitCount::Exact(0),
            files_changed: 0,
            files: Vec::new(),
            files_truncated: false,
            ..comparison(None, 0, 1)
        };
        vec![range, single, empty]
    }

    /// A file, a listing, a submodule, searches with and without hits,
    /// blames and comparisons, for each word of the corpus.
    fn answers() -> Vec<Box<dyn Present>> {
        let mut all: Vec<Box<dyn Present>> = Vec::new();
        for word in words() {
            let text = || word.to_owned();
            for blame in blames(word) {
                all.push(Box::new(blame::Blame { blame }));
            }
            for comparison in comparisons(word) {
                let range = Range::parse("github.com/gorilla/mux@a...b");
                let range = range.unwrap();
                all.push(Box::new(compare::Compare { comparison, range }));
            }
            let file = File {
                at: at(),
                path: text(),
                byte_size: 1,
                total_lines: 100,
                content_type: text(),
                binary: false,
                start: 1,
                lines: CORPUS.iter().map(|&line| line.to_owned()).collect(),
            };
            all.push(Box::new(read::Read { file, capped: true }));
            let submodule =
                |target| EntryKind::Submodule { url: text(), target };
            let entries = vec![
                Entry { path: text(), kind: EntryKind::Directory },
                Entry { path: text(), kind: EntryKind::File },
                Entry { path: text(), kind: submodule(Some(at())) },
                Entry { path: text(), kind: submodule(None) },
            ];
            let listing =
                Listing { at: at(), path: text(), entries, truncated: true };
            let tree = Tree::Listing(listing);
            all.push(Box::new(list::List { tree, depth: 2 }));
            let submodule = Submodule {
                at: at(),
                path: text(),
                url: text(),
                target: None,
            };
            let tree = Tree::Submodule(submodule);
            all.push(Box::new(list::List { tree, depth: 1 }));
            for (hits, retry) in
                [(hits(word), Some(word.to_owned())), (Vec::new(), None)]
            {
                let report = search_report(word, hits, retry);
                all.push(Box::new(search::Search { report }));
            }
        }
        all
    }

    /// Every string in a `hints` array anywhere in `value`.
    fn json_hints(value: &Value, hints: &mut Vec<String>) {
        match value {
            Value::Object(object) => {
                for (key, value) in object {
                    match (key.as_str(), value) {
                        ("hints", Value::Array(items)) => hints.extend(
                            items
                                .iter()
                                .filter_map(Value::as_str)
                                .map(str::to_owned),
                        ),
                        _ => json_hints(value, hints),
                    }
                }
            }
            Value::Array(items) => {
                for item in items {
                    json_hints(item, hints);
                }
            }
            _ => {}
        }
    }

    /// `line` split into words by bash, as an agent's shell would.
    fn shell_words(line: &str) -> Vec<String> {
        let script = format!(r"shopt -s failglob; printf '%s\0' {line}");
        let output =
            process::Command::new("bash").args(["-c", &script]).output();
        let output = output.unwrap();
        assert!(output.status.success(), "{script}: {output:?}");
        let words = String::from_utf8(output.stdout).unwrap();
        words.split_terminator('\0').map(str::to_owned).collect()
    }

    /// The commands each diagnostic code's hints may run, and each answer
    /// kind's.
    const HINT_COMMANDS: &[(&str, &str)] = &[
        ("repo_not_found", "search"),
        ("revision_not_found", "list"),
        ("path_not_found", "list"),
        ("not_a_file", "list"),
        ("not_a_directory", "read"),
        ("out_of_range", "read"),
        ("out_of_range", "blame"),
        ("no_parent", "list"),
        ("bad_query", "search"),
        ("incomplete", "search"),
        ("file", "read"),
        ("listing", "list"),
        ("search", "search"),
        ("comparison", "compare"),
    ];

    #[test]
    fn every_hint_is_a_valid_sego_invocation_of_its_command() {
        let mut hints: Vec<(String, String)> = Vec::new();
        for diagnostic in diagnostics() {
            let code = diagnostic.code().to_owned();
            hints.extend(
                diagnostic.hints().into_iter().map(|h| (code.clone(), h)),
            );
        }
        for answer in answers() {
            let document: Value =
                serde_json::from_str(&answer.json()).unwrap();
            let kind = document["kind"].as_str().unwrap().to_owned();
            let mut found = Vec::new();
            json_hints(&document, &mut found);
            hints.extend(found.into_iter().map(|h| (kind.clone(), h)));
        }
        hints.sort();
        hints.dedup();
        let wrong: Vec<String> = hints
            .iter()
            .filter_map(|(code, hint)| {
                let words = shell_words(hint);
                let error = invocation_error(&words).or_else(|| {
                    let commands: Vec<&str> = HINT_COMMANDS
                        .iter()
                        .filter(|row| row.0 == code)
                        .map(|row| row.1)
                        .collect();
                    if commands.is_empty() {
                        return Some(format!("no command for {code}"));
                    }
                    let command = words[1].as_str();
                    (!commands.contains(&command))
                        .then(|| format!("not {}", commands.join(" or ")))
                })?;
                Some(format!("{code}: {hint}: {error}"))
            })
            .collect();
        assert_eq!(wrong, Vec::<String>::new());
    }

    /// Why `words` isn't a sego command that runs, if it isn't.
    fn invocation_error(words: &[String]) -> Option<String> {
        let Some(("sego", argv)) =
            words.split_first().map(|(sego, argv)| (sego.as_str(), argv))
        else {
            return Some("not a sego command".to_owned());
        };
        match args::parse(argv.iter().map(OsString::from)) {
            Err(error) => Some(error.message),
            Ok(cli) => validate(&cli).err().map(|error| error.to_string()),
        }
    }

    /// The codes of diagnostics raised on sego's side, not the upstream's
    /// answer, so they never have hints.
    const OWN_CODES: &[&str] = &[
        "usage",
        "transport",
        "bad_response",
        "server_error",
        "schema_mismatch",
        "output",
    ];

    /// The codes of diagnostics that report the upstream's answer.
    const UPSTREAM_CODES: &[&str] = &[
        "repo_not_found",
        "revision_not_found",
        "path_not_found",
        "not_a_file",
        "not_a_directory",
        "too_large",
        "out_of_range",
        "no_parent",
        "bad_query",
        "incomplete",
    ];

    #[test]
    fn every_code_is_own_or_upstream() {
        for &(code, _) in EXIT_CODES {
            let own = OWN_CODES.contains(&code);
            let upstream = UPSTREAM_CODES.contains(&code);
            assert!(own != upstream, "{code}");
        }
        let listed = OWN_CODES.iter().chain(UPSTREAM_CODES);
        for code in listed {
            assert!(EXIT_CODES.iter().any(|row| row.0 == *code), "{code}");
        }
    }

    #[test]
    fn hints_come_only_from_the_upstream() {
        for diagnostic in diagnostics() {
            if OWN_CODES.contains(&diagnostic.code()) {
                assert_eq!(diagnostic.hints(), Vec::<String>::new());
            }
        }
    }

    #[test]
    fn only_usage_errors_show_a_synopsis() {
        for diagnostic in usage_errors() {
            let usage = diagnostic.usage();
            assert!(!usage.is_empty(), "{diagnostic}");
            assert!(usage.iter().all(|line| line.starts_with("sego ")));
        }
        for diagnostic in other_diagnostics() {
            assert_eq!(diagnostic.usage(), Vec::<String>::new());
        }
    }

    #[test]
    fn every_message_is_one_line() {
        for diagnostic in diagnostics() {
            let text = report(&*diagnostic);
            assert!(text.ends_with('\n'), "{text:?}");
            let lines: Vec<&str> = text.split_terminator('\n').collect();
            let (first, rest) = lines.split_first().unwrap();
            let causes = if diagnostic.outcome() == Outcome::AnsweredNo {
                let prefixes = ["error:", "caused by:", "hint:"];
                assert!(
                    !prefixes.iter().any(|prefix| first.starts_with(prefix)),
                    "{text}"
                );
                0
            } else {
                assert!(first.starts_with("error: "), "{text}");
                diagnostic.causes().len()
            };
            let usage = diagnostic.usage().len();
            let hints = diagnostic.hints().len();
            assert_eq!(rest.len(), causes + usage + hints, "{text}");
            let (cause_lines, rest) = rest.split_at(causes);
            let (usage_lines, hint_lines) = rest.split_at(usage);
            assert!(
                cause_lines.iter().all(|line| line.starts_with("caused by: ")),
                "{text}"
            );
            for (i, line) in usage_lines.iter().enumerate() {
                let prefix =
                    if i == 0 { "usage: sego " } else { "       sego " };
                assert!(line.starts_with(prefix), "{text}");
            }
            assert!(
                hint_lines.iter().all(|line| line.starts_with("hint: ")),
                "{text}"
            );
        }
    }

    #[test]
    fn exit_codes_match_outcomes() {
        let mut seen = Vec::new();
        for diagnostic in diagnostics() {
            let code = diagnostic.code();
            let row = EXIT_CODES.iter().find(|&&(row, _)| row == code);
            let exit = diagnostic.outcome().exit_code();
            assert_eq!(row.map(|&(_, exit)| exit), Some(exit), "{code}");
            seen.push(code);
        }
        for &(code, _) in EXIT_CODES {
            assert!(seen.contains(&code), "no {code} diagnostic");
        }
    }

    /// Asserts that no line of `text` has an unsafe character.
    fn assert_safe(text: &str) {
        for line in text.split_terminator('\n') {
            assert!(!line.chars().any(is_unsafe), "{line:?}");
        }
    }

    /// Asserts that `document` is one line, ending in a newline, that is
    /// one JSON object of `version` 1.
    fn assert_one_document(document: &str) {
        let line = document.strip_suffix('\n');
        let line = line.unwrap_or_else(|| panic!("{document:?}"));
        assert!(!line.contains('\n'), "{document:?}");
        let value: Value = serde_json::from_str(line)
            .unwrap_or_else(|error| panic!("{document}: {error}"));
        assert!(value.is_object(), "{document}");
        assert_eq!(value["version"], 1, "{document}");
    }

    #[test]
    fn no_unsafe_character_reaches_any_output() {
        for answer in answers() {
            assert_safe(&answer.text());
            assert_safe(&answer.json());
        }
        for diagnostic in diagnostics() {
            assert_safe(&report(&*diagnostic));
            assert_safe(&render::json::diagnostic(&*diagnostic));
        }
    }

    #[test]
    fn json_output_is_exactly_one_document() {
        for answer in answers() {
            assert_one_document(&answer.json());
        }
        for diagnostic in diagnostics() {
            assert_one_document(&render::json::diagnostic(&*diagnostic));
        }
    }
}
