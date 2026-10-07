use std::time::Instant;

use serde_json::Value;

use crate::error::Error;
use crate::upstream::top_level;
use crate::upstream::{GRAPHQL_PATH, Request, Transport, Var, classify};

pub mod lint;
pub mod model;
pub mod outcome;
pub mod query;
pub mod response;

use query::Prepared;

pub const OPERATION: &str = "Search";

pub const SELECTION: &str = "\
search(query: $query, version: V3, patternType: keyword) {
  results {
    matchCount approximateResultCount limitHit repositoriesCount
    timedout { name } cloning { name } missing { name }
    alert { title description kind proposedQueries { description query } }
    elapsedMilliseconds
    dynamicFilters { value label count limitHit kind }
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

pub fn request(prepared: &Prepared) -> Request {
    let query = Var {
        name: "query",
        ty: "String!",
        value: Value::String(prepared.sent.clone()),
    };
    top_level(OPERATION, vec![query], SELECTION)
}

/// A search has one root, so a located error anywhere leaves no part of the
/// answer to trust.
pub fn fetch(
    prepared: &Prepared,
    transport: &dyn Transport,
    deadline: Instant,
) -> Result<response::Results, Error> {
    let failed = || Error::failed("the search");
    let reply =
        transport.post(GRAPHQL_PATH, &request(prepared).body(), deadline)?;
    let envelope = classify(reply)?;
    if !envelope.errors.is_empty() {
        return Err(failed());
    }
    let results =
        envelope.get(&["search", "results"]).ok_or_else(failed)?.clone();
    serde_json::from_value(results).map_err(|_| failed())
}
