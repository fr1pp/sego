pub mod list;
pub mod read;
pub mod search;

use crate::args::{Cli, UsageError};
use crate::diagnostic::{Diagnostic, Outcome};
use crate::escape::shell_word;
use crate::sourcegraph::Fetch;

pub const COMMANDS: &[&str] = &["search", "list", "read"];

const HELP: &str = "\
sego finds and inspects public source code at any revision, through
Sourcegraph's GraphQL API.

Usage:
  sego search [--json] <query>...
  sego list   [--json] [--depth N] <repo>[@<rev>] [<path>]
  sego read   [--json] <repo>[@<rev>] <path> [<lines>]
  sego <command> --help

Commands:
  search   where is it? Matching code, paths, symbols, commits and repos
  list     what is here? The entries of a directory
  read     what does it say? The lines of a file

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

/// An answer, ready to print.
pub trait Present {
    fn outcome(&self) -> Outcome;
    fn text(&self) -> String;
    fn json(&self) -> String;
}

/// Help or the version: the same text with or without `--json`.
struct Plain(&'static str);

impl Present for Plain {
    fn outcome(&self) -> Outcome {
        Outcome::Answered
    }

    fn text(&self) -> String {
        self.0.to_owned()
    }

    fn json(&self) -> String {
        self.0.to_owned()
    }
}

/// A command sego runs.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Command {
    Search,
    List,
    Read,
}

impl Command {
    fn parse(name: &str) -> Option<Command> {
        match name {
            "search" => Some(Command::Search),
            "list" => Some(Command::List),
            "read" => Some(Command::Read),
            _ => None,
        }
    }

    fn help(self) -> &'static str {
        match self {
            Command::Search => search::HELP,
            Command::List => list::HELP,
            Command::Read => read::HELP,
        }
    }
}

/// What an invocation asks for.
enum Action {
    /// Help for a command, or the top-level help.
    Help(Option<Command>),
    Version,
    Run(Command),
}

/// What `cli` asks for. A usage error's hint is the command's help, or the
/// top-level help when there is no known command.
fn action(cli: &Cli) -> Result<Action, UsageError> {
    let invalid =
        |message: String| UsageError::new(message).with_hint(cli.help_hint());
    let command = match cli.command.as_deref() {
        Some(name) => match Command::parse(name) {
            Some(command) => Some(command),
            None => {
                return Err(invalid(format!(
                    "unknown command {}",
                    shell_word(name)
                )));
            }
        },
        None => None,
    };
    if let Some(command) = command
        && command != Command::List
        && cli.depth.is_some()
    {
        return Err(invalid("--depth applies only to list".to_owned()));
    }
    if cli.help {
        Ok(Action::Help(command))
    } else if cli.version {
        Ok(Action::Version)
    } else {
        match command {
            Some(command) => Ok(Action::Run(command)),
            None => Err(invalid(
                "missing command; expected search, list or read".to_owned(),
            )),
        }
    }
}

/// Whether the invocation runs a command, which needs the upstream.
pub fn needs_upstream(cli: &Cli) -> bool {
    matches!(action(cli), Ok(Action::Run(_)))
}

/// Checks the invocation as far as possible without fetching anything.
pub fn validate(cli: &Cli) -> Result<(), Box<dyn Diagnostic>> {
    match action(cli)? {
        Action::Run(Command::Search) => {
            search::request(cli)?;
        }
        Action::Run(Command::List) => {
            list::request(cli)?;
        }
        Action::Run(Command::Read) => {
            read::request(cli)?;
        }
        Action::Help(_) | Action::Version => {}
    }
    Ok(())
}

pub fn run(
    cli: &Cli,
    fetch: Fetch,
) -> Result<Box<dyn Present>, Box<dyn Diagnostic>> {
    match action(cli)? {
        Action::Help(None) => Ok(Box::new(Plain(HELP))),
        Action::Help(Some(command)) => Ok(Box::new(Plain(command.help()))),
        Action::Version => Ok(Box::new(Plain(VERSION))),
        Action::Run(Command::Search) => search::run(cli, fetch),
        Action::Run(Command::List) => list::run(cli, fetch),
        Action::Run(Command::Read) => read::run(cli, fetch),
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::io::{self, Write};
    use std::os::unix::ffi::OsStringExt as _;
    use std::process;

    use expect_test::expect;
    use serde_json::Value;

    use super::*;
    use crate::address::{CommitId, Pinned, RepoName};
    use crate::args;
    use crate::diagnostic::{SearchProblem, report};
    use crate::domain::{
        Alert, CommitHit, Entry, EntryKind, File, FileHit, Filter, Hit, Line,
        Listing, NotAFile, RepoHit, SearchReport, Skipped, Span, Submodule,
        Symbol, Term, Tree, Unavailable,
    };
    use crate::escape::{CORPUS, is_unsafe};
    use crate::output::Output;
    use crate::render;
    use crate::sourcegraph::FetchError;
    use crate::sourcegraph::transport::Transport;
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
        ("usage", 2),
        ("bad_query", 2),
        ("incomplete", 3),
        ("transport", 3),
        ("bad_response", 3),
        ("server_error", 3),
        ("schema_mismatch", 3),
        ("output", 3),
    ];

    #[test]
    fn every_command_name_parses() {
        for &name in COMMANDS {
            assert!(Command::parse(name).is_some(), "{name}");
        }
    }

    #[test]
    fn usage_errors_hint_the_command_help() {
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
                format!("{error} | {}\n", error.hints().join(" | "))
            })
            .collect();
        expect![[r#"
            unknown command serch | sego --help
            unknown command serch | sego --help
            unknown command serch | sego --help
            unknown command serch | sego --help
            --depth applies only to list | sego read --help
            --depth applies only to list | sego read --help
            --depth applies only to list | sego search --help
            missing command; expected search, list or read | sego --help
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
            Err(error) => Box::new(error),
            Ok(cli) => match validate(&cli) {
                Err(diagnostic) => diagnostic,
                Ok(()) => panic!("{argv:?} is valid"),
            },
        }
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
            ]);
        }
        argvs.push(vec!["read".into(), OsString::from_vec(vec![0x66, 0xff])]);
        argvs.iter().map(Vec::as_slice).map(usage).collect()
    }

    fn fetch_errors() -> Vec<FetchError> {
        let mut all = Vec::new();
        for &word in CORPUS {
            let text = || word.to_owned();
            all.extend([
                FetchError::Transport { url: text(), cause: text() },
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
                },
                Unavailable::OutOfRange {
                    at: at(),
                    path: path(),
                    start: 2,
                    total_lines: 0,
                },
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

    /// Writes nothing, failing with `cause`.
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
        all.extend(fetch_errors().into_iter().map(Box::from));
        all.extend(unavailable().into_iter().map(Box::from));
        all.extend(search_problems().into_iter().map(Box::from));
        for &word in CORPUS {
            let endpoint = Transport::new(word).err().unwrap();
            all.push(Box::new(endpoint));
            let mut output = Output::new(Failing(word.to_owned()));
            output.write("x").unwrap();
            all.push(Box::new(output.flush().unwrap_err()));
        }
        all
    }

    fn search_report(word: &str, hits: Vec<Hit>) -> SearchReport {
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
                unsupported: 2,
            },
            filters: vec![filter("repo"), filter("utility")],
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
            kind: Term { value: "other", raw: Some(text()) },
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

    /// A file, a listing, a submodule and searches with and without hits,
    /// for each word of the corpus.
    fn answers() -> Vec<Box<dyn Present>> {
        let mut all: Vec<Box<dyn Present>> = Vec::new();
        for word in words() {
            let text = || word.to_owned();
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
            for hits in [hits(word), Vec::new()] {
                let report = search_report(word, hits);
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
        let script = format!(r"printf '%s\0' {line}");
        let output =
            process::Command::new("bash").args(["-c", &script]).output();
        let output = output.unwrap();
        assert!(output.status.success(), "{script}: {output:?}");
        let words = String::from_utf8(output.stdout).unwrap();
        words.split_terminator('\0').map(str::to_owned).collect()
    }

    #[test]
    fn every_hint_is_a_valid_sego_invocation() {
        let mut hints: Vec<String> =
            diagnostics().iter().flat_map(|d| d.hints()).collect();
        for answer in answers() {
            let document = serde_json::from_str(&answer.json()).unwrap();
            json_hints(&document, &mut hints);
        }
        hints.sort();
        hints.dedup();
        let invalid: Vec<String> = hints
            .iter()
            .filter_map(|hint| {
                let error = invocation_error(&shell_words(hint))?;
                Some(format!("{hint}: {error}"))
            })
            .collect();
        assert_eq!(invalid, Vec::<String>::new());
    }

    /// Why `words` isn't a sego command that runs, if it isn't.
    fn invocation_error(words: &[String]) -> Option<String> {
        let Some(("sego", argv)) =
            words.split_first().map(|(sego, argv)| (sego.as_str(), argv))
        else {
            return Some("not a sego command".to_owned());
        };
        match args::parse(argv.iter().map(OsString::from)) {
            Err(error) => Some(error.to_string()),
            Ok(cli) => validate(&cli).err().map(|error| error.to_string()),
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
            let hints = diagnostic.hints().len();
            assert_eq!(rest.len(), causes + hints, "{text}");
            let (cause_lines, hint_lines) = rest.split_at(causes);
            assert!(
                cause_lines.iter().all(|line| line.starts_with("caused by: ")),
                "{text}"
            );
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
