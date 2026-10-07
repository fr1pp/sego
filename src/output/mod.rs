use std::io::{self, Write};

use crate::escape;

pub mod json;
pub mod text;
pub mod vocab;

/// What a command found, ready to be written as JSON or as text.
pub trait Answer {
    fn write_json(&self, out: &mut dyn Write) -> io::Result<()>;
    fn text(&self) -> String;
}

#[derive(Debug, Default)]
pub struct Notes(Vec<String>);

impl Notes {
    pub fn new() -> Notes {
        Notes::default()
    }

    pub fn push(&mut self, note: impl Into<String>) {
        self.0.push(note.into());
    }

    pub fn write(&self, err: &mut dyn Write) -> io::Result<()> {
        for note in &self.0 {
            writeln!(err, "sego: note: {}", escape::one_line(note))?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Marker {
    Match,
    Context,
}
