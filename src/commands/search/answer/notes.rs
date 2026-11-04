use std::collections::HashMap;

use crate::error::Error;
use crate::output::Notes;

use super::text::tenths;
use super::{Alert, Answer};
use crate::sourcegraph::regexp_reason;

const FACTS: usize = 2;
const PROCESS_FAILURE: &str = "Unable To Process Query";
const REVISIONS: &str = "Some revisions could not be searched";
const NO_REPOSITORIES: &str = "No repositories found";

impl Answer {
    /// Writes the answer's notes, or the error it is: an alert that means
    /// the search failed, or a stopped search with nothing in it.
    pub fn judge(&self, notes: &mut Notes) -> Result<(), Error> {
        if let Some(alert) = &self.alert {
            if let Some(reason) = description(alert).and_then(regexp_reason) {
                return Err(super::super::regexp_error(reason, &self.query));
            }
            if alert.title == PROCESS_FAILURE {
                return Err(Error::upstream(description(alert).unwrap_or(&alert.title)));
            }
            if self.is_zero() {
                if alert.title == REVISIONS {
                    return Err(revisions(alert));
                }
                if alert.title == NO_REPOSITORIES {
                    return Err(proposals(
                        alert,
                        Error::not_found(&alert.title).fact(self.query.sent()),
                    ));
                }
            }
        }
        if self.is_zero() && self.stopped {
            return Err(Error::upstream(format!(
                "the search stopped at {} s with no results",
                tenths(self.elapsed_ms)
            ))
            .fact(self.query.sent()));
        }
        push_notes(self, notes);
        Ok(())
    }
}

fn description(alert: &Alert) -> Option<&str> {
    alert.description.as_deref().filter(|d| !d.trim().is_empty())
}

fn revisions(alert: &Alert) -> Error {
    let Some(text) = description(alert) else {
        return Error::not_found(&alert.title);
    };
    let missing: Vec<(&str, &str)> = text
        .lines()
        .filter_map(|line| line.trim().strip_prefix("* "))
        .filter_map(|rest| rest.split_once('@'))
        .collect();
    let lines: Vec<String> =
        missing.iter().map(|(repo, rev)| format!("revision {rev} not found in {repo}")).collect();
    let Some((first, rest)) = lines.split_first() else {
        return Error::not_found(text);
    };
    let error = Error::not_found(first);
    match rest {
        [] => error,
        [one] => error.fact(one),
        [one, two] => error.fact(one).fact(two),
        [one, ..] => {
            error.fact(one).fact(format!("and {} more revisions not found", lines.len() - FACTS))
        }
    }
}

fn proposals(alert: &Alert, mut error: Error) -> Error {
    for proposed in alert.proposed.iter().take(FACTS) {
        error = error.fact(format!("the server proposes: {}", proposed.query));
    }
    error
}

fn push_notes(searched: &Answer, notes: &mut Notes) {
    if let Some(alert) = &searched.alert {
        match description(alert) {
            Some(text) => notes.push(format!("{}: {text}", alert.title)),
            None => notes.push(&alert.title),
        }
        for proposed in &alert.proposed {
            notes.push(format!("the server proposes: {}", proposed.query));
        }
    }
    let mut order: Vec<&str> = Vec::new();
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for kind in &searched.unshown {
        let count = counts.entry(kind).or_insert(0);
        if *count == 0 {
            order.push(kind);
        }
        *count += 1;
    }
    for kind in order {
        match counts[kind] {
            1 => notes.push(format!("1 result of kind {kind} not shown")),
            n => notes.push(format!("{n} results of kind {kind} not shown")),
        }
    }
}
