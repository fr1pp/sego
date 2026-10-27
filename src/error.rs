use std::fmt::{self, Write as _};

use crate::escape::one_line;

const MAX_FACTS: usize = 2;
const USAGE_PREFIX: &str = "usage: ";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Usage,
    Missing,
    Upstream,
    Edge,
    Network,
    Deadline,
}

#[derive(Debug)]
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

    pub fn missing(message: impl Into<String>) -> Error {
        Error::new(Kind::Missing, message)
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

    pub fn kind(&self) -> Kind {
        self.kind
    }

    pub fn exit_code(&self) -> u8 {
        match self.kind {
            Kind::Usage => 2,
            _ => 1,
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
    fn a_usage_error_exits_2_and_every_other_error_exits_1() {
        assert_eq!(Error::usage("x", "sego show").exit_code(), 2);
        assert_eq!(Error::missing("x").exit_code(), 1);
        assert_eq!(Error::upstream("x").exit_code(), 1);
        assert_eq!(Error::edge("x").exit_code(), 1);
        assert_eq!(Error::network("x").exit_code(), 1);
        assert_eq!(Error::deadline("sourcegraph.com").exit_code(), 1);
    }

    #[test]
    fn an_error_keeps_two_facts_and_drops_the_rest() {
        let error = Error::missing("gone").fact("one").fact("two").fact("three");
        assert_eq!(report(&error), "sego: error: gone\n  one\n  two\n");
    }
}
