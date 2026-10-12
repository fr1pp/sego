use std::collections::HashMap;

use crate::address::pinned;
use crate::error::Error;
use crate::output::Notes;
use crate::quote::shell_word;

use super::model::{AlertOut, Searched};
use super::query::tenths;
use super::response::UnindexedFile;

const FACTS: usize = 2;
const MIB: u64 = 1 << 20;
const KIB: u64 = 1 << 10;
const LISTED: usize = 3;
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
        if searched.is_zero() {
            if alert.title == REVISIONS {
                return Err(revisions(alert));
            }
            if alert.title == NO_REPOSITORIES {
                return Err(proposals(alert, Error::missing(&alert.title)));
            }
        }
    }
    if searched.is_zero() && searched.stopped {
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
    if let Some((first, rest)) = unindexed_note(searched) {
        notes.push_lines(first, rest);
    }
}

/// The note on the text files in scope that the search index doesn't hold,
/// for a zero or a complete answer; a lower bound or a stopped answer says
/// nothing about them. A path is relative only when the results and the
/// files are all in one repository, which the results already name.
pub fn unindexed_note(searched: &Searched) -> Option<(String, Vec<String>)> {
    let files = searched.unindexed.as_deref().filter(|f| !f.is_empty())?;
    if searched.stopped || searched.at_least {
        return None;
    }
    let (mut over, mut dense): (Vec<&UnindexedFile>, Vec<&UnindexedFile>) =
        files.iter().partition(|file| file.bytes > MIB);
    over.sort_by_key(|file| std::cmp::Reverse(file.bytes));
    dense.sort_by_key(|file| std::cmp::Reverse(file.bytes));
    let reasons: Vec<String> = [
        (over.len(), "over 1 MiB"),
        (dense.len(), "too many distinct trigrams"),
    ]
    .into_iter()
    .filter(|(n, _)| *n > 0)
    .map(|(n, reason)| format!("{n} {reason}"))
    .collect();
    let first = format!(
        "the contents of {} {} in scope aren't in the search index ({}):",
        files.len(),
        noun(files.len()),
        reasons.join(", ")
    );
    let relative = !searched.groups.is_empty()
        && searched
            .groups
            .iter()
            .all(|group| files.iter().all(|file| file.repo == group.repo()));
    let mut rest: Vec<String> = over
        .iter()
        .chain(&dense)
        .take(LISTED)
        .map(|file| {
            let path = if relative {
                shell_word(&file.path).into_owned()
            } else {
                pinned(&file.repo, &file.commit, Some(&file.path)).to_string()
            };
            format!("  {path}  {}", binary_size(file.bytes))
        })
        .collect();
    if files.len() > LISTED {
        rest.push(format!(
            "  … (+{} more {}; --json has the full list)",
            files.len() - LISTED,
            noun(files.len() - LISTED)
        ));
    }
    Some((first, rest))
}

fn noun(files: usize) -> &'static str {
    if files == 1 { "file" } else { "files" }
}

pub fn binary_size(bytes: u64) -> String {
    if bytes >= MIB {
        format!("{} MiB", tenths_of(bytes, MIB))
    } else if bytes >= KIB {
        format!("{} KiB", tenths_of(bytes, KIB))
    } else {
        format!("{bytes} B")
    }
}

/// `bytes / unit` to one decimal place, rounded half up.
fn tenths_of(bytes: u64, unit: u64) -> String {
    let tenths =
        (u128::from(bytes) * 10 + u128::from(unit) / 2) / u128::from(unit);
    format!("{}.{}", tenths / 10, tenths % 10)
}
