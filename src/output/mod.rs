use std::io::{self, Write};

use crate::escape;

pub mod json;
pub mod text;
pub mod vocab;

const DATE_LEN: usize = 10;

/// The day of a timestamp: its first ten characters, `YYYY-MM-DD`.
pub fn date(timestamp: &str) -> String {
    timestamp.chars().take(DATE_LEN).collect()
}

/// What a command found, ready to be written as JSON or as text.
pub trait Answer {
    fn write_json(&self, out: &mut dyn Write) -> io::Result<()>;
    fn text(&self) -> String;
}

#[derive(Debug, Default)]
pub struct Notes(Vec<Note>);

#[derive(Debug)]
struct Note {
    first: String,
    rest: Vec<String>,
}

impl Notes {
    pub fn new() -> Notes {
        Notes::default()
    }

    pub fn push(&mut self, note: impl Into<String>) {
        self.push_lines(note, Vec::new());
    }

    /// A note whose `rest` lines print under the first, each as given.
    pub fn push_lines(&mut self, first: impl Into<String>, rest: Vec<String>) {
        self.0.push(Note { first: first.into(), rest });
    }

    pub fn write(&self, err: &mut dyn Write) -> io::Result<()> {
        for note in &self.0 {
            writeln!(err, "sego: note: {}", escape::one_line(&note.first))?;
            for line in &note.rest {
                writeln!(err, "{}", escape::one_line(line))?;
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Marker {
    Match,
    Context,
}
