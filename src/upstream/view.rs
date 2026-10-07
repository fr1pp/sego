use serde_json::Value;

use super::envelope::{Envelope, GqlError};

/// An envelope read relative to the commit's alias path, such as
/// `["r0", "c0"]`.
pub struct View<'a> {
    envelope: &'a Envelope,
    at: &'a [String],
}

impl<'a> View<'a> {
    pub fn new(envelope: &'a Envelope, at: &'a [String]) -> View<'a> {
        View { envelope, at }
    }

    fn path<'p>(&'p self, tail: &[&'p str]) -> Vec<&'p str> {
        let mut path: Vec<&str> = self.at.iter().map(String::as_str).collect();
        path.extend_from_slice(tail);
        path
    }

    /// See `Envelope::get`.
    pub fn get(&self, tail: &[&str]) -> Option<&Value> {
        self.envelope.get(&self.path(tail))
    }

    /// See `Envelope::error_at`.
    pub fn error(&self, tail: &[&str]) -> Option<&GqlError> {
        self.envelope.error_at(&self.path(tail))
    }

    pub fn errored(&self, tail: &[&str]) -> bool {
        self.error(tail).is_some()
    }

    /// Whether an error sits at `tail` or anywhere below it.
    pub fn errored_beneath(&self, tail: &[&str]) -> bool {
        self.envelope.error_beneath(&self.path(tail)).is_some()
    }
}
