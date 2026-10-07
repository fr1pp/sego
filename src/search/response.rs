use serde::de::{Deserializer, Error as _};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::Value;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Results {
    pub match_count: u64,
    pub approximate_result_count: String,
    pub limit_hit: bool,
    pub repositories_count: u64,
    pub timedout: Vec<Named>,
    pub cloning: Vec<Named>,
    pub missing: Vec<Named>,
    pub alert: Option<Alert>,
    pub elapsed_milliseconds: u64,
    pub dynamic_filters: Vec<DynamicFilter>,
    pub results: Vec<Item>,
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
    pub label: String,
    pub count: u64,
    pub limit_hit: bool,
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

fn rest<T: DeserializeOwned, E: serde::de::Error>(
    value: Value,
) -> Result<T, E> {
    serde_json::from_value(value).map_err(E::custom)
}

impl<'de> Deserialize<'de> for Item {
    fn deserialize<D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Item, D::Error> {
        let value = Value::deserialize(deserializer)?;
        let typename = value
            .get("__typename")
            .and_then(Value::as_str)
            .ok_or_else(|| D::Error::missing_field("__typename"))?
            .to_owned();
        Ok(match typename.as_str() {
            "FileMatch" => Item::FileMatch(rest(value)?),
            "CommitSearchResult" => Item::CommitSearchResult(rest(value)?),
            "Repository" => Item::Repository(rest(value)?),
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
