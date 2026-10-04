use std::fmt::{self, Display, Write as _};

use crate::address::{LineRange, Pinned};
use crate::domain::{Alert, NotAFile, Unavailable};
use crate::escape::{command_line, sanitise, shell_word};
use crate::sourcegraph::search::with_longer_timeout;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Answered,
    AnsweredNo,
    Invalid,
    Unanswered,
}

impl Outcome {
    pub fn exit_code(self) -> u8 {
        match self {
            Outcome::Answered => 0,
            Outcome::AnsweredNo => 1,
            Outcome::Invalid => 2,
            Outcome::Unanswered => 3,
        }
    }
}

/// Anything that isn't an answer. `Display` writes the message.
pub trait Diagnostic: Display {
    fn code(&self) -> &'static str;
    fn outcome(&self) -> Outcome;
    fn causes(&self) -> Vec<String> {
        Vec::new()
    }
    fn hints(&self) -> Vec<String> {
        Vec::new()
    }
    fn detail(&self) -> Option<serde_json::Value> {
        None
    }
}

impl<D: Diagnostic + 'static> From<D> for Box<dyn Diagnostic> {
    fn from(diagnostic: D) -> Self {
        Box::new(diagnostic)
    }
}

/// The stderr text: an *answered no* as a plain sentence, anything else as
/// `error:` with its `caused by:` lines; then the `hint:` lines. Message and
/// causes are sanitised so each stays on one line; hints are already shell
/// words.
pub fn report(diagnostic: &dyn Diagnostic) -> String {
    let message = sanitise(&diagnostic.to_string());
    let mut text = String::new();
    if diagnostic.outcome() == Outcome::AnsweredNo {
        let _ = writeln!(text, "{message}");
    } else {
        let _ = writeln!(text, "error: {message}");
        for cause in diagnostic.causes() {
            let _ = writeln!(text, "caused by: {}", sanitise(&cause));
        }
    }
    for hint in diagnostic.hints() {
        let _ = writeln!(text, "hint: {hint}");
    }
    text
}

impl fmt::Display for Unavailable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Unavailable::RepoNotFound { repo } => {
                write!(f, "repository {} not found", shell_word(repo))
            }
            Unavailable::RevisionNotFound { repo, rev } => write!(
                f,
                "revision {} not found in {}",
                shell_word(rev),
                shell_word(repo)
            ),
            Unavailable::PathNotFound { at, path } => {
                write!(f, "{} not found in {}", shell_word(path), at.word())
            }
            Unavailable::NotAFile { at, path, what } => {
                let what = match what {
                    NotAFile::Directory => "directory",
                    NotAFile::Submodule => "submodule",
                };
                let (path, at) = (shell_word(path), at.word());
                write!(f, "{path} is a {what} in {at}")
            }
            Unavailable::NotADirectory { at, path } => {
                write!(f, "{} is a file in {}", shell_word(path), at.word())
            }
            Unavailable::TooLarge { at, path, byte_size } => write!(
                f,
                "{} in {} is {byte_size} bytes, over the upstream's 1.5 MiB \
                 read cap",
                shell_word(path),
                at.word()
            ),
            Unavailable::OutOfRange { at, path, start, total_lines } => {
                let lines = match total_lines {
                    1 => "line",
                    _ => "lines",
                };
                write!(
                    f,
                    "{} in {} has {total_lines} {lines}, so line {start} is \
                     past its end",
                    shell_word(path),
                    at.word()
                )
            }
        }
    }
}

impl Diagnostic for Unavailable {
    fn code(&self) -> &'static str {
        match self {
            Unavailable::RepoNotFound { .. } => "repo_not_found",
            Unavailable::RevisionNotFound { .. } => "revision_not_found",
            Unavailable::PathNotFound { .. } => "path_not_found",
            Unavailable::NotAFile { .. } => "not_a_file",
            Unavailable::NotADirectory { .. } => "not_a_directory",
            Unavailable::TooLarge { .. } => "too_large",
            Unavailable::OutOfRange { .. } => "out_of_range",
        }
    }

    fn outcome(&self) -> Outcome {
        Outcome::AnsweredNo
    }

    fn hints(&self) -> Vec<String> {
        let list = ["sego", "list"];
        let read = ["sego", "read"];
        // `repo@short`, unquoted: `command_line` quotes its operands.
        let pinned = |at: &Pinned| {
            format!("{}@{}", at.repo.as_str(), at.commit.short())
        };
        let hint = match self {
            Unavailable::RepoNotFound { repo } => {
                let name = repo.rsplit('/').next().unwrap_or(repo);
                let query = format!("type:repo {name}");
                command_line(&["sego", "search"], &[&query])
            }
            Unavailable::RevisionNotFound { repo, .. } => {
                command_line(&list, &[repo])
            }
            Unavailable::PathNotFound { at, path } => {
                let at = pinned(at);
                match path.rsplit_once('/') {
                    Some((parent, _)) => {
                        command_line(&list, &[&at, &format!("{parent}/")])
                    }
                    None => command_line(&list, &[&at]),
                }
            }
            Unavailable::NotAFile { at, path, what } => {
                let path = match what {
                    NotAFile::Directory => format!("{path}/"),
                    NotAFile::Submodule => path.clone(),
                };
                command_line(&list, &[&pinned(at), &path])
            }
            Unavailable::NotADirectory { at, path } => {
                command_line(&read, &[&pinned(at), path])
            }
            Unavailable::TooLarge { .. } => return Vec::new(),
            Unavailable::OutOfRange { at, path, total_lines, .. } => {
                if *total_lines == 0 {
                    return Vec::new();
                }
                let lines = LineRange { start: 1, end: Some(*total_lines) };
                command_line(&read, &[&pinned(at), path, &lines.to_string()])
            }
        };
        vec![hint]
    }
}

/// A search with no hits that has no answer: the upstream couldn't run the
/// query, or cut it short. `reason` says how.
#[derive(Debug)]
pub enum SearchProblem {
    BadQuery { alert: Alert },
    Incomplete { query: String, reason: String },
}

impl fmt::Display for SearchProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SearchProblem::BadQuery { .. } => {
                f.write_str("the upstream can't run this query")
            }
            SearchProblem::Incomplete { reason, .. } => {
                write!(f, "the search was cut short with no hits: {reason}")
            }
        }
    }
}

impl Diagnostic for SearchProblem {
    fn code(&self) -> &'static str {
        match self {
            SearchProblem::BadQuery { .. } => "bad_query",
            SearchProblem::Incomplete { .. } => "incomplete",
        }
    }

    fn outcome(&self) -> Outcome {
        match self {
            SearchProblem::BadQuery { .. } => Outcome::Invalid,
            SearchProblem::Incomplete { .. } => Outcome::Unanswered,
        }
    }

    /// The alert's description, the upstream's account of the problem.
    fn causes(&self) -> Vec<String> {
        match self {
            SearchProblem::BadQuery { alert } => {
                alert.description.iter().cloned().collect()
            }
            SearchProblem::Incomplete { .. } => Vec::new(),
        }
    }

    fn hints(&self) -> Vec<String> {
        let search = |query: &str| command_line(&["sego", "search"], &[query]);
        match self {
            SearchProblem::BadQuery { alert } => {
                alert.queries().map(search).collect()
            }
            SearchProblem::Incomplete { query, .. } => {
                let retry = with_longer_timeout(query);
                vec![search(retry.as_deref().unwrap_or(query))]
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fmt;

    use expect_test::expect;

    use super::*;
    use crate::address::{CommitId, RepoName};

    struct Sample {
        outcome: Outcome,
        message: String,
        causes: Vec<String>,
        hints: Vec<String>,
    }

    impl Sample {
        fn no() -> Self {
            Sample {
                outcome: Outcome::AnsweredNo,
                message: "repository github.com/gorilla/nope not found".into(),
                causes: Vec::new(),
                hints: vec!["sego search 'type:repo nope'".into()],
            }
        }

        fn failure() -> Self {
            Sample {
                outcome: Outcome::Unanswered,
                message: "the request to \
                          https://sourcegraph.com/.api/graphql failed"
                    .into(),
                causes: vec!["connection refused".into()],
                hints: vec!["sego read github.com/x/y mux.go".into()],
            }
        }
    }

    impl fmt::Display for Sample {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str(&self.message)
        }
    }

    impl Diagnostic for Sample {
        fn code(&self) -> &'static str {
            "sample"
        }

        fn outcome(&self) -> Outcome {
            self.outcome
        }

        fn causes(&self) -> Vec<String> {
            self.causes.clone()
        }

        fn hints(&self) -> Vec<String> {
            self.hints.clone()
        }
    }

    #[test]
    fn exit_codes() {
        let codes = [
            Outcome::Answered,
            Outcome::AnsweredNo,
            Outcome::Invalid,
            Outcome::Unanswered,
        ]
        .map(Outcome::exit_code);
        assert_eq!(codes, [0, 1, 2, 3]);
    }

    #[test]
    fn answered_no_is_a_plain_sentence() {
        expect![[r#"
            repository github.com/gorilla/nope not found
            hint: sego search 'type:repo nope'
        "#]]
        .assert_eq(&report(&Sample::no()));
    }

    #[test]
    fn errors_carry_causes_and_hints() {
        expect![[r#"
            error: the request to https://sourcegraph.com/.api/graphql failed
            caused by: connection refused
            hint: sego read github.com/x/y mux.go
        "#]]
        .assert_eq(&report(&Sample::failure()));
    }

    #[test]
    fn messages_and_causes_stay_on_one_line() {
        let sample = Sample {
            outcome: Outcome::Invalid,
            message: "bad\nthing\u{202e}".into(),
            causes: vec!["first\r\nsecond".into()],
            hints: Vec::new(),
        };
        expect![[r#"
            error: bad\x0athing\u{202e}
            caused by: first\x0d\x0asecond
        "#]]
        .assert_eq(&report(&sample));
    }

    #[test]
    fn unavailable_names_the_place_and_the_next_command() {
        let at = || Pinned {
            repo: RepoName::parse("github.com/gorilla/mux").unwrap(),
            commit: CommitId(
                "a7962380ca08b5a188038c69871b8d3fbdf31e89".into(),
            ),
        };
        let path = |path: &str| path.to_owned();
        let all = [
            Unavailable::RepoNotFound {
                repo: "github.com/gorilla/nope".into(),
            },
            Unavailable::RevisionNotFound {
                repo: "github.com/gorilla/mux".into(),
                rev: "it's".into(),
            },
            Unavailable::PathNotFound { at: at(), path: path("nope.go") },
            Unavailable::PathNotFound { at: at(), path: path("a b/c/d.go") },
            Unavailable::NotAFile {
                at: at(),
                path: path(".github"),
                what: NotAFile::Directory,
            },
            Unavailable::NotAFile {
                at: at(),
                path: path("vendor/x"),
                what: NotAFile::Submodule,
            },
            Unavailable::NotADirectory { at: at(), path: path("mux.go") },
            Unavailable::TooLarge {
                at: at(),
                path: path("big.go"),
                byte_size: 3_057_771,
            },
            Unavailable::OutOfRange {
                at: at(),
                path: path("mux.go"),
                start: 700,
                total_lines: 601,
            },
            Unavailable::OutOfRange {
                at: at(),
                path: path("one"),
                start: 2,
                total_lines: 1,
            },
            Unavailable::OutOfRange {
                at: at(),
                path: path("empty"),
                start: 2,
                total_lines: 0,
            },
        ];
        let mut text = String::new();
        for unavailable in &all {
            assert_eq!(unavailable.outcome(), Outcome::AnsweredNo);
            let _ = writeln!(text, "[{}]", unavailable.code());
            text.push_str(&report(unavailable));
        }
        expect![[r#"
            [repo_not_found]
            repository github.com/gorilla/nope not found
            hint: sego search 'type:repo nope'
            [revision_not_found]
            revision 'it'\''s' not found in github.com/gorilla/mux
            hint: sego list github.com/gorilla/mux
            [path_not_found]
            nope.go not found in github.com/gorilla/mux@a7962380ca08
            hint: sego list github.com/gorilla/mux@a7962380ca08
            [path_not_found]
            'a b/c/d.go' not found in github.com/gorilla/mux@a7962380ca08
            hint: sego list github.com/gorilla/mux@a7962380ca08 'a b/c/'
            [not_a_file]
            .github is a directory in github.com/gorilla/mux@a7962380ca08
            hint: sego list github.com/gorilla/mux@a7962380ca08 .github/
            [not_a_file]
            vendor/x is a submodule in github.com/gorilla/mux@a7962380ca08
            hint: sego list github.com/gorilla/mux@a7962380ca08 vendor/x
            [not_a_directory]
            mux.go is a file in github.com/gorilla/mux@a7962380ca08
            hint: sego read github.com/gorilla/mux@a7962380ca08 mux.go
            [too_large]
            big.go in github.com/gorilla/mux@a7962380ca08 is 3057771 bytes, over the upstream's 1.5 MiB read cap
            [out_of_range]
            mux.go in github.com/gorilla/mux@a7962380ca08 has 601 lines, so line 700 is past its end
            hint: sego read github.com/gorilla/mux@a7962380ca08 mux.go 1-601
            [out_of_range]
            one in github.com/gorilla/mux@a7962380ca08 has 1 line, so line 2 is past its end
            hint: sego read github.com/gorilla/mux@a7962380ca08 one 1
            [out_of_range]
            empty in github.com/gorilla/mux@a7962380ca08 has 0 lines, so line 2 is past its end
        "#]].assert_eq(&text);
    }

    #[test]
    fn the_defaults_add_nothing() {
        struct Bare;

        impl fmt::Display for Bare {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("bare")
            }
        }

        impl Diagnostic for Bare {
            fn code(&self) -> &'static str {
                "bare"
            }

            fn outcome(&self) -> Outcome {
                Outcome::Unanswered
            }
        }

        assert_eq!((Bare.code(), Bare.detail()), ("bare", None));
        assert_eq!(report(&Bare), "error: bare\n");
    }

    #[test]
    fn blank_proposed_queries_are_not_hints() {
        let problem = SearchProblem::BadQuery {
            alert: Alert {
                title: "Unable To Process Query".into(),
                description: None,
                proposed: vec![String::new(), " \t".into(), "x".into()],
            },
        };
        assert_eq!(problem.hints(), ["sego search x"]);
    }

    #[test]
    fn search_problems_name_the_next_search() {
        let all = [
            SearchProblem::BadQuery {
                alert: Alert {
                    title: "Unable To Process Query".into(),
                    description: Some(
                        "Error parsing regexp: missing closing ): `(`".into(),
                    ),
                    proposed: Vec::new(),
                },
            },
            SearchProblem::BadQuery {
                alert: Alert {
                    title: "Unable To Process Query".into(),
                    description: None,
                    proposed: vec![
                        "repo:^github\\.com/gorilla/mux$ (".into(),
                        "-file:_test x".into(),
                    ],
                },
            },
            SearchProblem::Incomplete {
                query: "x timeout:40s".into(),
                reason: "it ran for its whole timeout:40s".into(),
            },
            SearchProblem::Incomplete {
                query: "x timeout:1m".into(),
                reason: "it ran for its whole timeout:1m".into(),
            },
            SearchProblem::Incomplete {
                query: "x timeout:1m30s".into(),
                reason: "it ran for its whole timeout:1m30s".into(),
            },
            SearchProblem::Incomplete {
                query: "x".into(),
                reason: "1 repository timed out".into(),
            },
        ];
        let mut text = String::new();
        for problem in &all {
            let _ =
                writeln!(text, "[{} {:?}]", problem.code(), problem.outcome());
            text.push_str(&report(problem));
        }
        expect![[r#"
            [bad_query Invalid]
            error: the upstream can't run this query
            caused by: Error parsing regexp: missing closing ): `(`
            [bad_query Invalid]
            error: the upstream can't run this query
            hint: sego search 'repo:^github\.com/gorilla/mux$ ('
            hint: sego search '-file:_test x'
            [incomplete Unanswered]
            error: the search was cut short with no hits: it ran for its whole timeout:40s
            hint: sego search 'x timeout:80s'
            [incomplete Unanswered]
            error: the search was cut short with no hits: it ran for its whole timeout:1m
            hint: sego search 'x timeout:90s'
            [incomplete Unanswered]
            error: the search was cut short with no hits: it ran for its whole timeout:1m30s
            hint: sego search 'x timeout:1m30s'
            [incomplete Unanswered]
            error: the search was cut short with no hits: 1 repository timed out
            hint: sego search x
        "#]].assert_eq(&text);
    }
}
