//! The recordings: what sourcegraph.com answered, one generated file per
//! distinct request, named for a hash of the request. A test holds nothing of
//! them: sego sends a request, and the store answers it.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use ring::digest::{SHA256, digest};
use serde_json::{Map, Value, json};

use sego::Response;

use super::replay::response_of;

#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    Graphql { operation: String, variables: Value, query: String },
    Raw { path: String },
}

impl Request {
    /// The request as text for a failure message.
    pub fn describe(&self) -> String {
        match self {
            Request::Graphql { operation, variables, query } => {
                format!("{operation} {variables}\n{}", normalise(query))
            }
            Request::Raw { path } => format!("GET {path}"),
        }
    }

    /// The name of its recording: the operation, then 16 hex digits of the
    /// SHA-256 of the request, its query's whitespace normalised.
    pub fn name(&self) -> String {
        let (label, text) = match self {
            Request::Graphql { operation, variables, query } => {
                (operation.as_str(), format!("{operation}\0{variables}\0{}", normalise(query)))
            }
            Request::Raw { path } => ("GET", format!("GET\0{path}")),
        };
        let hash = digest(&SHA256, text.as_bytes());
        let mut name = format!("{label}-");
        for byte in &hash.as_ref()[..8] {
            let _ = write!(name, "{byte:02x}");
        }
        name
    }
}

fn normalise(query: &str) -> String {
    query.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// What the server answers, whether recorded or set in a test.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerResponse {
    pub status: u16,
    pub content_type: Option<String>,
    pub body: Vec<u8>,
}

impl ServerResponse {
    pub fn new(status: u16, content_type: &str, body: impl Into<Vec<u8>>) -> ServerResponse {
        ServerResponse { status, content_type: Some(content_type.to_owned()), body: body.into() }
    }

    pub fn served(&self) -> Response {
        response_of(self.status, self.content_type.as_deref(), self.body.clone())
    }
}

impl From<Value> for ServerResponse {
    fn from(body: Value) -> ServerResponse {
        ServerResponse::new(200, "application/json", body.to_string())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Recording {
    pub request: Request,
    pub response: ServerResponse,
    pub date: String,
}

/// The directory of recordings.
#[derive(Clone, Debug)]
pub struct Store {
    dir: PathBuf,
    /// Recording forwards a request even when it has a recording.
    pub again: bool,
}

impl Default for Store {
    fn default() -> Store {
        Store::at(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/recordings"))
    }
}

impl Store {
    pub fn at(dir: &Path) -> Store {
        Store { dir: dir.to_path_buf(), again: false }
    }

    pub fn again(self) -> Store {
        Store { again: true, ..self }
    }

    fn file(&self, name: &str) -> PathBuf {
        self.dir.join(format!("{name}.json"))
    }

    /// The name of every recording.
    pub fn names(&self) -> Result<Vec<String>, String> {
        let entries =
            fs::read_dir(&self.dir).map_err(|e| format!("{}: {e}", self.dir.display()))?;
        let mut names = Vec::new();
        for entry in entries {
            let path = entry.map_err(|e| e.to_string())?.path();
            if let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) {
                names.push(stem.to_owned());
            }
        }
        names.sort();
        Ok(names)
    }

    pub fn load(&self, name: &str) -> Result<Recording, String> {
        let path = self.file(name);
        let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let file: Value =
            serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        let (request, response) = (&file["request"], &file["response"]);
        let text_of = |value: &Value| value.as_str().unwrap_or_default().to_owned();
        let body = match &response["body"] {
            Value::String(text) => text.clone().into_bytes(),
            body => body.to_string().into_bytes(),
        };
        let status = response["status"].as_u64().and_then(|status| u16::try_from(status).ok());
        Ok(Recording {
            request: match request["get"].as_str() {
                Some(get) => Request::Raw { path: get.to_owned() },
                None => Request::Graphql {
                    operation: text_of(&request["operation"]),
                    variables: request["variables"].clone(),
                    query: text_of(&request["query"]),
                },
            },
            response: ServerResponse {
                status: status.ok_or(format!("{}: no status", path.display()))?,
                content_type: response["content-type"].as_str().map(str::to_owned),
                body,
            },
            date: text_of(&file["date"]),
        })
    }

    /// The recording of `request`, or why sego may not send it.
    pub fn find(&self, request: &Request) -> Result<ServerResponse, String> {
        let name = request.name();
        if !self.file(&name).is_file() {
            return Err(format!(
                "no recording answers this request; run `just record`\n{}",
                request.describe()
            ));
        }
        Ok(self.load(&name)?.response)
    }

    /// Writes what the server answered to `request`: a GraphQL response that
    /// is JSON with sorted keys and without what varies, and any other as text,
    /// which a body that isn't UTF-8 can't be.
    pub fn write(
        &self,
        request: &Request,
        response: &Response,
        date: &str,
    ) -> Result<String, String> {
        let name = request.name();
        let sent = match request {
            Request::Graphql { operation, variables, query } => {
                json!({"operation": operation, "variables": variables, "query": query})
            }
            Request::Raw { path } => json!({"get": path}),
        };
        let json = matches!(request, Request::Graphql { .. })
            .then(|| serde_json::from_slice::<Value>(&response.body).ok())
            .flatten();
        let body = match json {
            Some(mut body) => {
                strip(&mut body);
                body
            }
            None => json!(std::str::from_utf8(&response.body).map_err(|e| {
                format!("{}: can't record a body that isn't UTF-8: {e}", request.describe())
            })?),
        };
        let mut answered = Map::new();
        answered.insert("status".to_owned(), json!(response.status));
        if let Some(content_type) = &response.content_type {
            answered.insert("content-type".to_owned(), json!(content_type));
        }
        answered.insert("body".to_owned(), body);
        let file = json!({"date": date, "request": sent, "response": answered});
        let mut text = serde_json::to_string_pretty(&file).map_err(|e| e.to_string())?;
        text.push('\n');
        let path = self.file(&name);
        fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(name)
    }

    pub fn remove(&self, name: &str) -> Result<(), String> {
        let path = self.file(name);
        fs::remove_file(&path).map_err(|e| format!("{}: {e}", path.display()))
    }
}

/// `response` without what varies between answers. An error's own `extensions`
/// stay: sego reads their `code`.
fn strip(response: &mut Value) {
    if let Value::Object(map) = response {
        map.remove("extensions");
    }
    strip_request_ids(response);
}

fn strip_request_ids(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.remove("requestId");
            map.values_mut().for_each(strip_request_ids);
        }
        Value::Array(items) => items.iter_mut().for_each(strip_request_ids),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    const JSON: Option<&str> = Some("application/json");

    fn refs(repo: &str, query: &str) -> Request {
        Request::Graphql {
            operation: "Refs".to_owned(),
            variables: json!({"repo0": repo}),
            query: query.to_owned(),
        }
    }

    fn store() -> (TempDir, Store) {
        let dir = TempDir::new().unwrap_or_else(|e| panic!("{e}"));
        let store = Store::at(dir.path());
        (dir, store)
    }

    #[test]
    fn a_name_follows_the_request_up_to_the_whitespace_of_its_query() {
        let name = refs("a/b", "query Refs { x }").name();
        assert!(name.starts_with("Refs-"), "{name}");
        assert_eq!(name, refs("a/b", "query Refs {\n  x\n}\n").name());
        assert_ne!(name, refs("c/d", "query Refs { x }").name());
        assert_ne!(name, refs("a/b", "query Refs { y }").name());
        let raw = Request::Raw { path: "/r".to_owned() }.name();
        assert!(raw.starts_with("GET-"), "{raw}");
    }

    #[test]
    fn a_json_response_is_written_into_the_recording_without_what_varies() {
        let (_dir, store) = store();
        let request = refs("a/b", "query");
        let body =
            r#"{"data":{"z":1,"a":[{"requestId":"r","k":2}]},"extensions":{},"requestId":"q"}"#;
        let name = store
            .write(&request, &response_of(200, JSON, body), "2026-10-07")
            .unwrap_or_else(|e| panic!("{e}"));
        let recording = store.load(&name).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(recording.request, request);
        assert_eq!(recording.date, "2026-10-07");
        assert_eq!(recording.response.status, 200);
        assert_eq!(recording.response.content_type.as_deref(), JSON);
        assert_eq!(recording.response.body, br#"{"data":{"a":[{"k":2}],"z":1}}"#);
    }

    #[test]
    fn an_errors_extensions_are_kept_since_sego_reads_them() {
        let (_dir, store) = store();
        let request = refs("a/b", "query");
        let body = r#"{"errors":[{"extensions":{"code":"E"},"message":"m"}],"extensions":{"t":1}}"#;
        let name = store
            .write(&request, &response_of(200, JSON, body), "d")
            .unwrap_or_else(|e| panic!("{e}"));
        let recording = store.load(&name).unwrap_or_else(|e| panic!("{e}"));
        let body = recording.response.body;
        assert_eq!(body, br#"{"errors":[{"extensions":{"code":"E"},"message":"m"}]}"#);
    }

    #[test]
    fn any_other_body_is_written_as_text() {
        let (_dir, store) = store();
        let raw = Request::Raw { path: "/r".to_owned() };
        let plain = Some("text/plain");
        for (request, body) in [(raw, "line\r\n\0bytes\n"), (refs("a/b", "query"), "Bad Request")] {
            store
                .write(&request, &response_of(400, plain, body), "d")
                .unwrap_or_else(|e| panic!("{e}"));
            let found = store.find(&request).unwrap_or_else(|e| panic!("{e}"));
            assert_eq!(found.body, body.as_bytes());
        }
    }

    #[test]
    fn a_body_that_isnt_utf_8_is_refused_and_nothing_is_written() {
        let (dir, store) = store();
        let raw = Request::Raw { path: "/r".to_owned() };
        let refused = store.write(&raw, &response_of(200, None, b"\xff\xfe".to_vec()), "d");
        assert!(refused.is_err_and(|e| e.contains("isn't UTF-8")));
        assert_eq!(fs::read_dir(dir.path()).map(Iterator::count).ok(), Some(0));
    }

    #[test]
    fn a_request_with_no_recording_says_to_record_it() {
        let (_dir, store) = store();
        let error = store.find(&refs("a/b", "query")).err().unwrap_or_default();
        assert!(error.starts_with("no recording answers this request; run `just record`\nRefs"));
    }
}
