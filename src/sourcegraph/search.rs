use serde::Deserialize;
use serde::de::{DeserializeOwned, Deserializer, Error as _};
use serde_json::{Map, Value, json};

use super::graphql::{Envelope, Request};
use crate::error::Error;

/// The word that starts the index's placeholder for a file it skipped.
pub const NOT_INDEXED: &str = "NOT-INDEXED";

const DOCUMENT: &str = "\
query Search($query: String!) {
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
}
";

/// What a search found, or why the server couldn't run it.
#[derive(Debug)]
pub enum Search {
    Results(Results),
    /// The server's regular expression engine couldn't compile the query,
    /// for this reason.
    BadRegexp {
        reason: String,
    },
}

pub fn request(query: &str) -> Request {
    let mut variables = Map::new();
    variables.insert("query".to_owned(), json!(query));
    Request::new("Search", DOCUMENT.to_owned(), variables)
}

/// A located error at or beneath `search` leaves no part of the answer to
/// trust.
pub fn read(envelope: &Envelope) -> Result<Search, Error> {
    let failed = || Error::failed("the search");
    if let Some(error) = envelope.error_beneath(&["search"]) {
        return error
            .regexp_reason()
            .map(|reason| Search::BadRegexp { reason: reason.to_owned() })
            .ok_or_else(failed);
    }
    let results = envelope.get(&["search", "results"]).ok_or_else(failed)?;
    Results::deserialize(results).map(Search::Results).map_err(|_| failed())
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

impl FileMatch {
    /// Why the index skipped this file, when the match is the index's
    /// placeholder for it: one chunk at line 0 that reads `NOT-INDEXED:
    /// REASON`, with no symbols.
    pub fn placeholder_reason(&self) -> Option<&str> {
        match self.chunk_matches.as_slice() {
            [chunk] if self.symbols.is_empty() && chunk.content_start.line == 0 => {
                let reason = chunk.content.strip_prefix(NOT_INDEXED)?.strip_prefix(": ")?;
                Some(reason.trim_end_matches('\n'))
            }
            _ => None,
        }
    }
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
    use serde_json::json;

    use super::*;

    fn placeholder_file(chunks: &Value, symbols: &Value) -> Value {
        json!({
            "__typename": "FileMatch",
            "repository": {"name": "r/a"},
            "file": {"path": "big.bin", "commit": {"oid": "c1"}},
            "revSpec": null,
            "chunkMatches": chunks,
            "symbols": symbols,
        })
    }

    fn placeholder_chunk(content: &str, line: u32) -> Value {
        json!({"content": content, "contentStart": {"line": line}, "ranges": []})
    }

    fn reason(value: &Value) -> Option<String> {
        let item: Item =
            serde_json::from_value(value.clone()).unwrap_or_else(|e| panic!("bad fixture: {e}"));
        match item {
            Item::FileMatch(file) => file.placeholder_reason().map(str::to_owned),
            _ => panic!("not a file match"),
        }
    }

    #[test]
    fn a_placeholder_is_one_not_indexed_line_at_line_zero_with_no_symbols() {
        let text = "NOT-INDEXED: exceeds the maximum size limit";
        let one = |chunks: Value| reason(&placeholder_file(&chunks, &json!([])));
        assert_eq!(
            one(json!([placeholder_chunk(text, 0)])).as_deref(),
            Some("exceeds the maximum size limit")
        );
        assert_eq!(one(json!([placeholder_chunk(text, 3)])), None);
        assert_eq!(one(json!([placeholder_chunk("not-indexed: x", 0)])), None);
        assert_eq!(one(json!([])), None);
        assert_eq!(one(json!([placeholder_chunk(text, 0), placeholder_chunk(text, 0)])), None);
        let symbol =
            json!({"name": "S", "kind": "FUNCTION", "containerName": null, "location": null});
        let with_symbol = placeholder_file(&json!([placeholder_chunk(text, 0)]), &json!([symbol]));
        assert_eq!(reason(&with_symbol), None);
    }
}
