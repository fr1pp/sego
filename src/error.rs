use std::fmt;
use std::io::{self, Write};

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
        Error {
            kind,
            message: message.into(),
            facts: Vec::new(),
            synopsis: None,
        }
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

    pub fn deadline() -> Error {
        Error::new(
            Kind::Deadline,
            "no answer from sourcegraph.com within 90 s",
        )
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

pub fn report(error: &Error, out: &mut dyn Write) -> io::Result<()> {
    writeln!(out, "sego: error: {}", one_line(&error.message))?;
    for fact in &error.facts {
        writeln!(out, "  {}", one_line(fact))?;
    }
    if let Some(synopsis) = error.synopsis {
        writeln!(out, "{}", usage_block(synopsis))?;
    }
    Ok(())
}
