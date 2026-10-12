use std::collections::HashMap;

use crate::error::Error;
use crate::output::Notes;

use super::model::{AlertOut, Searched};
use super::query::tenths;

const FACTS: usize = 2;
const PROCESS_FAILURE: &str = "Unable To Process Query";
const REVISIONS: &str = "Some revisions could not be searched";
const NO_REPOSITORIES: &str = "No repositories found";

pub fn judge(searched: &Searched, notes: &mut Notes) -> Result<(), Error> {
    if let Some(alert) = &searched.alert {
        if alert.title == PROCESS_FAILURE {
            return Err(Error::upstream(
                description(alert).unwrap_or(&alert.title),
            ));
        }
        if searched.groups.is_empty() && searched.unshown.is_empty() {
            if alert.title == REVISIONS {
                return Err(revisions(alert));
            }
            if alert.title == NO_REPOSITORIES {
                return Err(proposals(alert, Error::missing(&alert.title)));
            }
        }
    }
    if searched.count == 0 && searched.stopped {
        return Err(Error::missing(format!(
            "the search stopped at {} s with no results",
            tenths(searched.elapsed_ms)
        )));
    }
    push_notes(searched, notes);
    Ok(())
}

fn description(alert: &AlertOut) -> Option<&str> {
    alert.description.as_deref().filter(|d| !d.trim().is_empty())
}

fn revisions(alert: &AlertOut) -> Error {
    let Some(text) = description(alert) else {
        return Error::missing(&alert.title);
    };
    let lines: Vec<String> = text
        .lines()
        .filter_map(|line| line.trim().strip_prefix("* "))
        .filter_map(|rest| rest.split_once('@'))
        .map(|(repo, rev)| format!("revision {rev} not found in {repo}"))
        .collect();
    let Some((first, rest)) = lines.split_first() else {
        return Error::missing(text);
    };
    let error = Error::missing(first);
    match rest {
        [] => error,
        [one] => error.fact(one),
        [one, two] => error.fact(one).fact(two),
        [one, ..] => error.fact(one).fact(format!(
            "and {} more revisions not found",
            lines.len() - FACTS
        )),
    }
}

fn proposals(alert: &AlertOut, mut error: Error) -> Error {
    for proposed in alert.proposed.iter().take(FACTS) {
        error = error.fact(format!("the server proposes: {}", proposed.query));
    }
    error
}

fn push_notes(searched: &Searched, notes: &mut Notes) {
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
