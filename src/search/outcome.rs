use std::collections::HashMap;

use crate::error::Error;
use crate::output::Notes;

use super::query::{Prepared, seconds};
use super::response::{Alert, Item, Named, Results};

const LISTED: usize = 5;
const FACTS: usize = 2;
const PROCESS_FAILURE: &str = "Unable To Process Query";
const REVISIONS: &str = "Some revisions could not be searched";
const NO_REPOSITORIES: &str = "No repositories found";

pub fn judge(
    results: &Results,
    prepared: &Prepared,
    notes: &mut Notes,
) -> Result<(), Error> {
    let stopped = !results.timedout.is_empty()
        || results.elapsed_milliseconds >= prepared.deadline_ms;
    if let Some(alert) = &results.alert {
        if alert.title == PROCESS_FAILURE {
            return Err(Error::upstream(
                description(alert).unwrap_or(&alert.title),
            ));
        }
        if results.results.is_empty() {
            if alert.title == REVISIONS {
                return Err(revisions(alert));
            }
            if alert.title == NO_REPOSITORIES {
                return Err(proposals(alert, Error::missing(&alert.title)));
            }
        }
    }
    if results.results.is_empty() && stopped {
        let error = Error::missing(format!(
            "the search stopped at its {} s deadline with no results",
            seconds(prepared.deadline_ms)
        ));
        return Err(if results.timedout.is_empty() {
            error
        } else {
            error.fact(format!("timed out: {}", names(&results.timedout)))
        });
    }
    push_notes(results, prepared, stopped, notes);
    Ok(())
}

pub fn names(list: &[Named]) -> String {
    let shown: Vec<&str> =
        list.iter().take(LISTED).map(|n| n.name.as_str()).collect();
    match list.len().checked_sub(LISTED).filter(|&more| more > 0) {
        Some(more) => format!("{}, and {more} more", shown.join(", ")),
        None => shown.join(", "),
    }
}

fn description(alert: &Alert) -> Option<&str> {
    alert.description.as_deref().filter(|d| !d.trim().is_empty())
}

fn revisions(alert: &Alert) -> Error {
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

fn proposals(alert: &Alert, mut error: Error) -> Error {
    for proposed in alert.proposed_queries.iter().flatten().take(FACTS) {
        error = error.fact(format!("the server proposes: {}", proposed.query));
    }
    error
}

fn push_notes(
    results: &Results,
    prepared: &Prepared,
    stopped: bool,
    notes: &mut Notes,
) {
    if let Some(alert) = &results.alert {
        match description(alert) {
            Some(text) => notes.push(format!("{}: {text}", alert.title)),
            None => notes.push(&alert.title),
        }
        for proposed in alert.proposed_queries.iter().flatten() {
            notes.push(format!("the server proposes: {}", proposed.query));
        }
    }
    for (label, list) in [
        ("timed out", &results.timedout),
        ("cloning", &results.cloning),
        ("missing", &results.missing),
    ] {
        if !list.is_empty() {
            notes.push(format!("{label}: {}", names(list)));
        }
    }
    if stopped && !results.results.is_empty() {
        notes.push(format!(
            "the search stopped at its {} s deadline",
            seconds(prepared.deadline_ms)
        ));
    }
    let mut order: Vec<&str> = Vec::new();
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for item in &results.results {
        if let Item::Other(kind) = item {
            let count = counts.entry(kind).or_insert(0);
            if *count == 0 {
                order.push(kind);
            }
            *count += 1;
        }
    }
    for kind in order {
        match counts[kind] {
            1 => notes.push(format!("1 result of kind {kind} not shown")),
            n => notes.push(format!("{n} results of kind {kind} not shown")),
        }
    }
}
