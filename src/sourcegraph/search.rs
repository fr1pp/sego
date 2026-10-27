use serde::de::{DeserializeOwned, Deserializer, Error as _, IgnoredAny};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use super::graphql::{Envelope, Request};
use crate::error::Error;

/// A search, and when `$listed` the side query that lists the files the
/// index skipped.
const DOCUMENT: &str = "\
query Search($query: String!, $unindexed: String!, $listed: Boolean!) {
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
  }
  unindexed: search(query: $unindexed, version: V3, patternType: keyword) @include(if: $listed) {
    results {
      limitHit elapsedMilliseconds timedout { name } alert { title }
      results { ... on FileMatch { repository { name } file { path byteSize commit { oid } } } }
    }
  }
}
";

/// What a search found, or why the server couldn't run it.
#[derive(Debug)]
#[expect(clippy::large_enum_variant, reason = "an invocation reads one")]
pub enum Search {
    Results {
        results: Results,
        /// The side query's listing; `None` when it wasn't sent, or failed or
        /// can't be read.
        unindexed: Option<Listing>,
    },
    /// The server's regular expression engine couldn't compile the query,
    /// for this reason.
    BadRegexp { reason: String },
}

/// `query`, and the side query `unindexed` that lists the files the index
/// skipped, when given. A `String!` is sent even where its part is skipped.
pub fn request(query: &str, unindexed: Option<&str>) -> Request {
    let mut variables = Map::new();
    variables.insert("query".to_owned(), json!(query));
    variables.insert("unindexed".to_owned(), json!(unindexed.unwrap_or_default()));
    variables.insert("listed".to_owned(), json!(unindexed.is_some()));
    Request::new("Search", DOCUMENT.to_owned(), variables)
}

/// A located error at or beneath `search` leaves no part of the answer to
/// trust. The `unindexed` alias, read only when `listed` sent it, can't fail
/// the search: any problem with it leaves its listing unknown.
pub fn read(envelope: &Envelope, listed: bool) -> Result<Search, Error> {
    let failed = || Error::failed("the search");
    if let Some(error) = envelope.error_beneath(&["search"]) {
        return regexp_reason(&error.message)
            .map(|reason| Search::BadRegexp { reason: reason.to_owned() })
            .ok_or_else(failed);
    }
    let results = envelope.get(&["search", "results"]).ok_or_else(failed)?;
    let results = Results::deserialize(results).map_err(|_| failed())?;
    let unindexed = if !listed || envelope.error_beneath(&["unindexed"]).is_some() {
        None
    } else {
        envelope.get(&["unindexed", "results"]).and_then(|v| Listing::deserialize(v).ok())
    };
    Ok(Search::Results { results, unindexed })
}

/// The first line of the reason in a Go regexp error, which may come
/// wrapped in gRPC errors; `None` for any other message.
fn regexp_reason(message: &str) -> Option<&str> {
    let mut rest = message;
    while let Some(after) = rest.strip_prefix("rpc error: code = ") {
        rest = after.split_once(" desc = ")?.1;
    }
    Some(rest.strip_prefix("error parsing regexp: ")?.lines().next()?.trim())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Results {
    pub match_count: u64,
    pub approximate_result_count: String,
    pub limit_hit: bool,
    pub timedout: Vec<Named>,
    pub alert: Option<Alert>,
    pub elapsed_milliseconds: u64,
    pub dynamic_filters: Vec<DynamicFilter>,
    #[serde(rename = "results")]
    pub items: Vec<Item>,
}

#[derive(Debug, Serialize)]
pub struct UnindexedFile {
    pub repo: String,
    pub commit: String,
    pub path: String,
    pub bytes: u64,
}

/// The `unindexed` alias's results.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Listing {
    pub limit_hit: bool,
    pub elapsed_milliseconds: u64,
    pub timedout: Vec<Named>,
    pub alert: Option<IgnoredAny>,
    /// A selection naming only `FileMatch` leaves `{}` for any other kind.
    pub results: Vec<Value>,
}

#[derive(Debug, Deserialize)]
struct Listed {
    repository: Named,
    file: ListedFile,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListedFile {
    path: String,
    byte_size: u64,
    commit: Oid,
}

impl Listing {
    /// The file matches, in order; anything else is skipped.
    pub fn files(self) -> Vec<UnindexedFile> {
        self.results
            .into_iter()
            .filter_map(|item| serde_json::from_value::<Listed>(item).ok())
            .map(|listed| UnindexedFile {
                repo: listed.repository.name,
                commit: listed.file.commit.oid,
                path: listed.file.path,
                bytes: listed.file.byte_size,
            })
            .collect()
    }
}

#[derive(Debug, Deserialize)]
pub struct Named {
    pub name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Alert {
    pub title: String,
    pub description: Option<String>,
    pub kind: Option<String>,
    pub proposed_queries: Option<Vec<Proposed>>,
}

#[derive(Debug, Deserialize)]
pub struct Proposed {
    pub description: Option<String>,
    pub query: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DynamicFilter {
    pub value: String,
    pub count: u64,
    pub kind: String,
}

#[derive(Debug)]
pub enum Item {
    FileMatch(FileMatch),
    CommitSearchResult(CommitResult),
    Repository(Named),
    /// A `__typename` this selection doesn't name.
    Other(String),
}

fn from_value<T: DeserializeOwned, E: serde::de::Error>(value: Value) -> Result<T, E> {
    serde_json::from_value(value).map_err(E::custom)
}

impl<'de> Deserialize<'de> for Item {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Item, D::Error> {
        let value = Value::deserialize(deserializer)?;
        let typename = value
            .get("__typename")
            .and_then(Value::as_str)
            .ok_or_else(|| D::Error::missing_field("__typename"))?
            .to_owned();
        Ok(match typename.as_str() {
            "FileMatch" => Item::FileMatch(from_value(value)?),
            "CommitSearchResult" => Item::CommitSearchResult(from_value(value)?),
            "Repository" => Item::Repository(from_value(value)?),
            _ => Item::Other(typename),
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileMatch {
    pub repository: Named,
    pub file: File,
    pub rev_spec: Option<RevSpec>,
    pub chunk_matches: Vec<ChunkMatch>,
    pub symbols: Vec<Symbol>,
}

#[derive(Debug, Deserialize)]
pub struct File {
    pub path: String,
    pub commit: Oid,
}

#[derive(Debug, Deserialize)]
pub struct Oid {
    pub oid: String,
}

#[derive(Debug, Deserialize)]
pub struct RevSpec {
    pub expr: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChunkMatch {
    pub content: String,
    pub content_start: Line,
    pub ranges: Vec<Range>,
}

#[derive(Debug, Deserialize)]
pub struct Line {
    pub line: u32,
}

#[derive(Debug, Deserialize)]
pub struct Range {
    pub start: Position,
    pub end: Position,
}

#[derive(Debug, Deserialize)]
pub struct Position {
    pub line: u32,
    pub character: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Symbol {
    pub name: String,
    pub kind: String,
    pub container_name: Option<String>,
    pub location: Option<Location>,
}

#[derive(Debug, Deserialize)]
pub struct Location {
    pub range: Option<SymbolRange>,
}

#[derive(Debug, Deserialize)]
pub struct SymbolRange {
    pub start: Option<Line>,
}

#[derive(Debug, Deserialize)]
pub struct CommitResult {
    pub commit: Commit,
    pub matches: Vec<Match>,
}

#[derive(Debug, Deserialize)]
pub struct Commit {
    pub oid: String,
    pub subject: String,
    pub author: Author,
    pub repository: Named,
}

#[derive(Debug, Deserialize)]
pub struct Author {
    pub date: String,
    pub person: Option<Person>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Person {
    pub display_name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Match {
    pub body: Text,
    pub highlights: Vec<Highlight>,
}

#[derive(Debug, Deserialize)]
pub struct Text {
    pub text: String,
}

#[derive(Debug, Deserialize)]
pub struct Highlight {
    pub line: u32,
    pub character: u32,
    pub length: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn results() -> serde_json::Value {
        serde_json::json!({
            "matchCount": 0, "approximateResultCount": "0", "limitHit": false, "timedout": [],
            "alert": null, "elapsedMilliseconds": 5, "dynamicFilters": [], "results": []
        })
    }

    fn listing() -> serde_json::Value {
        serde_json::json!({
            "limitHit": false, "elapsedMilliseconds": 5, "timedout": [], "alert": null,
            "results": []
        })
    }

    fn searched(body: &serde_json::Value, listed: bool) -> Result<Search, String> {
        read(&crate::sourcegraph::parsed(body), listed).map_err(|e| crate::error::report(&e))
    }

    #[test]
    fn the_side_listing_is_read_only_when_it_was_sent_and_answered() {
        let body = serde_json::json!({"data": {
            "search": {"results": results()},
            "unindexed": {"results": listing()},
        }});
        let listed = |body: &serde_json::Value, sent: bool| match searched(body, sent) {
            Ok(Search::Results { unindexed, .. }) => unindexed.is_some(),
            other => panic!("{other:?}"),
        };
        assert!(listed(&body, true));
        assert!(!listed(&body, false));
        let mut failed = body.clone();
        failed["errors"] = serde_json::json!([{"message": "x", "path": ["unindexed", "results"]}]);
        assert!(!listed(&failed, true));
    }

    #[test]
    fn a_regexp_the_server_cant_compile_is_its_reason_and_any_other_error_fails_the_search() {
        let failing = |message: &str| {
            serde_json::json!({"data": {"search": null},
                "errors": [{"message": message, "path": ["search"]}]})
        };
        let wrapped = "rpc error: code = Unknown desc = error parsing regexp: missing closing ): \
                       `(a`\nmore";
        match searched(&failing(wrapped), false) {
            Ok(Search::BadRegexp { reason }) => assert_eq!(reason, "missing closing ): `(a`"),
            other => panic!("{other:?}"),
        }
        assert_eq!(
            searched(&failing("something else"), false).err().as_deref(),
            Some("sego: error: Sourcegraph failed to answer the search\n")
        );
    }
}
