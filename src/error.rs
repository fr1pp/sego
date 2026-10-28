use std::fmt::{self, Write as _};
use std::io;

use crate::escape::one_line;

const MAX_FACTS: usize = 2;
const USAGE_PREFIX: &str = "usage: ";

/// Why an invocation stopped: its answer was an error, or writing failed.
pub enum Failure {
    Answer(Error),
    Write(io::Error),
}

impl Failure {
    /// The failure with `synopsis` when it is a usage error without one.
    #[must_use]
    pub fn with_synopsis(self, synopsis: &'static str) -> Failure {
        match self {
            Failure::Answer(error) => Failure::Answer(error.with_synopsis(synopsis)),
            Failure::Write(error) => Failure::Write(error),
        }
    }
}

impl From<Error> for Failure {
    fn from(error: Error) -> Failure {
        Failure::Answer(error)
    }
}

impl From<io::Error> for Failure {
    fn from(error: io::Error) -> Failure {
        Failure::Write(error)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Usage,
    NotFound,
    Upstream,
    Edge,
    Network,
    Deadline,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    kind: Kind,
    message: String,
    facts: Vec<String>,
    synopsis: Option<&'static str>,
}

impl Error {
    fn new(kind: Kind, message: impl Into<String>) -> Error {
        Error { kind, message: message.into(), facts: Vec::new(), synopsis: None }
    }

    pub fn usage(message: impl Into<String>, synopsis: &'static str) -> Error {
        Error { synopsis: Some(synopsis), ..Error::new(Kind::Usage, message) }
    }

    /// A usage error found from the server's response, which takes its
    /// command's synopsis with `with_synopsis`.
    pub fn usage_from_response(message: impl Into<String>) -> Error {
        Error::new(Kind::Usage, message)
    }

    /// The error with `synopsis` when it is a usage error without one.
    #[must_use]
    pub fn with_synopsis(mut self, synopsis: &'static str) -> Error {
        if self.kind == Kind::Usage && self.synopsis.is_none() {
            self.synopsis = Some(synopsis);
        }
        self
    }

    pub fn not_found(message: impl Into<String>) -> Error {
        Error::new(Kind::NotFound, message)
    }

    pub fn upstream(message: impl Into<String>) -> Error {
        Error::new(Kind::Upstream, message)
    }

    pub fn failed(part: &str) -> Error {
        Error::upstream(format!("Sourcegraph failed to answer {part}"))
    }

    pub fn edge(message: impl Into<String>) -> Error {
        Error::new(Kind::Edge, message)
    }

    pub fn network(message: impl Into<String>) -> Error {
        Error::new(Kind::Network, message)
    }

    pub fn deadline(host: &str) -> Error {
        Error::new(Kind::Deadline, format!("no answer from {host} within 90 s"))
    }

    #[must_use]
    pub fn fact(mut self, fact: impl Into<String>) -> Error {
        if self.facts.len() < MAX_FACTS {
            self.facts.push(fact.into());
        }
        self
    }

    #[must_use]
    pub fn facts(self, facts: impl IntoIterator<Item = String>) -> Error {
        facts.into_iter().fold(self, Error::fact)
    }

    pub fn kind(&self) -> Kind {
        self.kind
    }

    pub fn exit_code(&self) -> u8 {
        match self.kind {
            Kind::Usage => 2,
            Kind::NotFound => 1,
            Kind::Upstream | Kind::Edge | Kind::Network | Kind::Deadline => 3,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

/// A synopsis as a usage error and a help text print it: `usage: ` before
/// the first line, matching indentation before each later one.
pub fn usage_block(synopsis: &str) -> String {
    let indent = " ".repeat(USAGE_PREFIX.len());
    let lines: Vec<&str> = synopsis.lines().collect();
    format!("{USAGE_PREFIX}{}", lines.join(&format!("\n{indent}")))
}

/// What sego writes to stderr for `error`: the message, its facts indented,
/// then the synopsis of a usage error.
pub fn report(error: &Error) -> String {
    let mut text = format!("sego: error: {}\n", one_line(&error.message));
    for fact in &error.facts {
        let _ = writeln!(text, "  {}", one_line(fact));
    }
    if let Some(synopsis) = error.synopsis {
        let _ = writeln!(text, "{}", usage_block(synopsis));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_usage_error_found_from_a_response_takes_its_commands_synopsis() {
        let error = Error::usage_from_response("gorilla/mux exists as a and b");
        assert_eq!(error.exit_code(), 2);
        assert_eq!(report(&error), "sego: error: gorilla/mux exists as a and b\n");
        assert_eq!(
            report(&error.with_synopsis("sego show X")),
            "sego: error: gorilla/mux exists as a and b\nusage: sego show X\n"
        );
        let kept = Error::usage("x", "sego ls Y").with_synopsis("sego show X");
        assert_eq!(report(&kept), "sego: error: x\nusage: sego ls Y\n");
        assert_eq!(report(&Error::not_found("x").with_synopsis("sego show X")), "sego: error: x\n");
    }

    #[test]
    fn a_usage_error_exits_2_not_found_exits_1_and_a_failure_exits_3() {
        assert_eq!(Error::usage("x", "sego show").exit_code(), 2);
        assert_eq!(Error::not_found("x").exit_code(), 1);
        assert_eq!(Error::upstream("x").exit_code(), 3);
        assert_eq!(Error::edge("x").exit_code(), 3);
        assert_eq!(Error::network("x").exit_code(), 3);
        assert_eq!(Error::deadline("sourcegraph.com").exit_code(), 3);
    }

    #[test]
    fn an_error_keeps_two_facts_and_drops_the_rest() {
        let error = Error::not_found("gone").fact("one").fact("two").fact("three");
        assert_eq!(report(&error), "sego: error: gone\n  one\n  two\n");
    }
}
