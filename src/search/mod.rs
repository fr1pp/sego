use std::time::Instant;

use serde_json::Value;

use crate::error::{Error, Kind};
use crate::output::Notes;
use crate::upstream::top_level;
use crate::upstream::{Envelope, Request, Transport, Var, exchange};

pub mod derive;
pub mod lint;
pub mod model;
pub mod outcome;
pub mod plan;
pub mod query;
pub mod response;
pub mod text;

use derive::in_time;
use model::Searched;
use query::{Prepared, prepare};
use response::{Listing, Results, UnindexedFile};

pub const OPERATION: &str = "Search";

pub const SELECTION: &str = "\
search(query: $query, version: V3, patternType: keyword) {
  results {
    matchCount approximateResultCount limitHit
    timedout { name }
    alert { title description kind proposedQueries { description query } }
    elapsedMilliseconds
    dynamicFilters { value count kind }
    results {
      __typename
      ... on FileMatch {
        repository { name }
        file { path commit { oid } }
        revSpec { ... on GitRevSpecExpr { expr } }
        chunkMatches { content contentStart { line }
          ranges { start { line character } end { line character } } }
        symbols { name kind containerName location { range { start { line } } } }
      }
      ... on CommitSearchResult {
        commit { oid subject author { date person { displayName } } repository { name } }
        matches { body { text } highlights { line character length } }
      }
      ... on Repository { name }
    }
  }
}";

const UNINDEXED_SELECTION: &str = "\
unindexed: search(query: $unindexed, version: V3, patternType: keyword) {
  results {
    limitHit elapsedMilliseconds timedout { name } alert { title }
    results { ... on FileMatch { repository { name } file { path byteSize commit { oid } } } }
  }
}";

pub fn request(prepared: &Prepared) -> Request {
    let string = |name, value: &str| Var {
        name,
        ty: "String!",
        value: Value::String(value.to_owned()),
    };
    let mut vars = vec![string("query", &prepared.query)];
    let mut selection = SELECTION.to_owned();
    if let Some(unindexed) = &prepared.unindexed {
        vars.push(string("unindexed", unindexed));
        selection.push('\n');
        selection.push_str(UNINDEXED_SELECTION);
    }
    top_level(OPERATION, vars, &selection)
}

pub struct Fetched {
    pub results: Results,
    /// `None` when the side alias wasn't sent or its answer can't be trusted.
    pub unindexed: Option<Vec<UnindexedFile>>,
}

/// A located error at or beneath `search` leaves no part of the answer to
/// trust. The `unindexed` alias can't fail the search: any problem with it
/// leaves its fact unknown.
pub fn fetch(
    prepared: &Prepared,
    transport: &dyn Transport,
    deadline: Instant,
) -> Result<Fetched, Error> {
    let failed = || Error::failed("the search");
    let envelope =
        exchange(transport, &request(prepared), deadline).map_err(pointed)?;
    if let Some(error) = envelope.error_beneath(&["search"]) {
        return Err(
            regexp_error(&error.message, prepared).unwrap_or_else(failed)
        );
    }
    let results =
        envelope.get(&["search", "results"]).ok_or_else(failed)?.clone();
    let results = serde_json::from_value(results).map_err(|_| failed())?;
    Ok(Fetched { results, unindexed: listed(&envelope, prepared) })
}

fn pointed(error: Error) -> Error {
    if error.kind() == Kind::Deadline {
        error.fact("repo:, lang: or file: narrows what the search reads")
    } else {
        error
    }
}

const REASON_CHARS: usize = 200;

/// The error for a search whose regular expression the server's Go regexp
/// couldn't compile, as a structural hole the query syntax lets through;
/// `None` for any other message.
fn regexp_error(message: &str, prepared: &Prepared) -> Option<Error> {
    let mut rest = message;
    while let Some(after) = rest.strip_prefix("rpc error: code = ") {
        rest = after.split_once(" desc = ")?.1;
    }
    let reason =
        rest.strip_prefix("error parsing regexp: ")?.lines().next()?.trim();
    let reason = if reason.chars().count() > REASON_CHARS {
        format!("{}…", reason.chars().take(REASON_CHARS).collect::<String>())
    } else {
        reason.to_owned()
    };
    let error = Error::upstream(format!(
        "the search's regular expression doesn't compile: {reason}"
    ));
    Some(if prepared.pattern_type == "structural" {
        error.fact(
            "structural search compiles each :[name~regex] hole as an RE2 regular expression",
        )
    } else {
        error
    })
}

fn listed(
    envelope: &Envelope,
    prepared: &Prepared,
) -> Option<Vec<UnindexedFile>> {
    prepared.unindexed.as_ref()?;
    if envelope.error_beneath(&["unindexed"]).is_some() {
        return None;
    }
    let value = envelope.get(&["unindexed", "results"])?.clone();
    let listing: Listing = serde_json::from_value(value).ok()?;
    let clean = listing.alert.is_none()
        && !listing.limit_hit
        && listing.timedout.is_empty()
        && in_time(listing.elapsed_milliseconds, prepared.deadline_ms);
    clean.then(|| listing.files())
}

pub fn run(
    query: &str,
    transport: &dyn Transport,
    deadline: Instant,
    notes: &mut Notes,
) -> Result<Searched, Error> {
    let prepared = prepare(query);
    for note in &prepared.notes {
        notes.push(note);
    }
    let fetched = fetch(&prepared, transport, deadline)?;
    let mut searched = Searched::new(fetched.results, &prepared);
    searched.unindexed = fetched.unindexed;
    outcome::judge(&searched, notes)?;
    Ok(searched)
}
