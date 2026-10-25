use std::fmt::Write as _;

use crate::case::{Case, Request};
use crate::run::Outcome;

/// The checks every case gets: at most one GraphQL request and one raw read,
/// no request on a usage error, and no note or error line ending in a period.
pub fn every_case(outcome: &Outcome) -> Vec<String> {
    let mut failures = Vec::new();
    let graphql = outcome
        .sent
        .iter()
        .filter(|request| matches!(request, Request::Graphql { .. }))
        .count();
    let raw = outcome.sent.len() - graphql;
    if graphql > 1 || raw > 1 {
        failures.push(format!(
            "sego sent {graphql} GraphQL requests and {raw} raw reads; an \
             invocation sends at most one of each"
        ));
    }
    if outcome.exit == 2 && !outcome.sent.is_empty() {
        failures.push(format!(
            "{} request(s) reached the server on a usage error",
            outcome.sent.len()
        ));
    }
    failures.extend(outcome.stderr.lines().filter_map(ends_in_a_period));
    failures.extend(outcome.problems.iter().cloned());
    failures
}

/// A note, an error or a fact line ends without a period, as in rustc, cargo
/// and git. A message that starts in uppercase is Sourcegraph's own text,
/// quoted as sent.
fn ends_in_a_period(line: &str) -> Option<String> {
    let text = line
        .strip_prefix("sego: note: ")
        .or_else(|| line.strip_prefix("sego: error: "))
        .unwrap_or(line.trim_start());
    if text.starts_with(|c: char| c.is_ascii_uppercase()) {
        return None;
    }
    (text.ends_with('.') && !text.ends_with("..."))
        .then(|| format!("a note or error line ends with a period: {line}"))
}

/// A request with no exchange left, and exchanges that went unused.
pub fn exchanges_used(case: &Case, outcome: &Outcome) -> Vec<String> {
    let mut failures = Vec::new();
    for (i, request) in outcome.sent.iter().enumerate() {
        if i >= case.exchanges.len() {
            failures.push(format!(
                "{}: sego sent request {} with no exchange left to answer \
                 it\n{}",
                case.path.display(),
                i + 1,
                request.describe()
            ));
        }
    }
    let unused = case.exchanges.len().saturating_sub(outcome.sent.len());
    if unused > 0 {
        failures.push(format!(
            "{}: {unused} of {} exchanges went unused; sego sent {} request(s)",
            case.path.display(),
            case.exchanges.len(),
            outcome.sent.len()
        ));
    }
    failures
}

/// Each request sent against the exchange written for it, both shown when
/// they differ.
pub fn requests_match(case: &Case, outcome: &Outcome) -> Vec<String> {
    let mut failures = Vec::new();
    for (i, (sent, exchange)) in
        outcome.sent.iter().zip(&case.exchanges).enumerate()
    {
        let written = exchange.request.as_ref();
        if written.is_some_and(|written| written.same_as(sent)) {
            continue;
        }
        let mut message = format!(
            "{}: request {} differs from the exchange written for it\n--- \
             written\n{}\n--- sent\n{}",
            case.path.display(),
            i + 1,
            written.map_or_else(|| "(none)".to_owned(), Request::describe),
            sent.describe()
        );
        if exchange.response.stamp.is_some() {
            let _ = write!(
                message,
                "\nits response is recorded; record it again with `just \
                 record`"
            );
        }
        failures.push(message);
    }
    failures
}

module_tests!();
