use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;
use toml_edit::{ArrayOfTables, DocumentMut, Item, Table, TableLike};

use super::hostile::is_unsafe;

/// A case file: what a command line should print, and the exchanges with the
/// server that answer it. The document is kept as it was read, so a rewrite
/// touches only the values it owns.
#[derive(Clone, Debug)]
pub struct Case {
    pub path: PathBuf,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub code: i32,
    pub stdout: Stream,
    pub stderr: Stream,
    pub exchanges: Vec<Exchange>,
    document: DocumentMut,
}

/// What a case expects on stdout or stderr: the text, which is empty when the
/// key is absent, or a pipe whose reader has gone.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Stream {
    Text(String),
    Closed,
}

/// A request sego sends and the response it gets. A response written by hand
/// may leave the request for bless to fill in.
#[derive(Clone, Debug, PartialEq)]
pub struct Exchange {
    pub request: Option<Request>,
    pub status: u16,
    pub content_type: Option<String>,
    pub recorded: Option<Recorded>,
    pub body: Body,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    Graphql { operation: String, variables: Value, query: String },
    Raw { path: String },
}

/// A response's body: text served as written, or JSON served compact; or a
/// file beside the case, for a raw read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Body {
    Text(String),
    File(String),
}

/// When and where a response was recorded, and whether `just live` compares
/// its values as well as its shape.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Recorded {
    pub date: String,
    pub endpoint: String,
    pub live_values: bool,
}

impl Request {
    /// Whether `other` is the same request: the same operation and variables
    /// and the same query up to whitespace, or the same raw read.
    pub fn same_as(&self, other: &Request) -> bool {
        match (self, other) {
            (
                Request::Graphql { operation, variables, query },
                Request::Graphql { operation: o, variables: v, query: q },
            ) => operation == o && variables == v && normalise(query) == normalise(q),
            (Request::Raw { path }, Request::Raw { path: p }) => path == p,
            _ => false,
        }
    }

    /// The request as text for a failure message.
    pub fn describe(&self) -> String {
        match self {
            Request::Graphql { operation, variables, query } => {
                format!("{operation} {variables}\n{}", normalise(query))
            }
            Request::Raw { path } => format!("GET {path}"),
        }
    }
}

fn normalise(query: &str) -> String {
    query.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The keys a case's top level and its exchanges hold, in the order they are
/// written.
const TOP_KEYS: [&str; 5] = ["args", "env", "status", "stdout", "stderr"];
const EXCHANGE_KEYS: [&str; 9] = [
    "operation",
    "variables",
    "query",
    "get",
    "status",
    "content-type",
    "recorded",
    "response",
    "body",
];

impl Case {
    pub fn load(path: &Path) -> Result<Case, String> {
        let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Case::parse(&text, path).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn parse(text: &str, path: &Path) -> Result<Case, String> {
        let document: DocumentMut = text.parse().map_err(|e| format!("{e}"))?;
        let root = document.as_table();
        for (key, _) in root {
            if !TOP_KEYS.contains(&key) && key != "exchange" {
                return Err(format!("unknown key {key}"));
            }
        }
        let args = root
            .get("args")
            .and_then(Item::as_array)
            .ok_or("args must be an array of strings")?
            .iter()
            .map(|arg| arg.as_str().map(str::to_owned).ok_or("args must be an array of strings"))
            .collect::<Result<_, _>>()?;
        let mut env = Vec::new();
        if let Some(table) = root.get("env") {
            let table = table.as_table_like().ok_or("env must be a table of strings")?;
            for (name, value) in table.iter() {
                let value = value.as_str().ok_or("env must be a table of strings")?;
                env.push((name.to_owned(), value.to_owned()));
            }
        }
        let code = root
            .get("status")
            .and_then(Item::as_table_like)
            .and_then(|status| status.get("code"))
            .and_then(Item::as_integer)
            .and_then(|code| i32::try_from(code).ok())
            .ok_or("status.code must be an exit code")?;
        let mut exchanges = Vec::new();
        if let Some(item) = root.get("exchange") {
            let tables = item.as_array_of_tables().ok_or("exchange must be [[exchange]] tables")?;
            for (i, table) in tables.iter().enumerate() {
                exchanges.push(exchange(table).map_err(|e| format!("exchange {}: {e}", i + 1))?);
            }
        }
        Ok(Case {
            path: path.to_path_buf(),
            args,
            env,
            code,
            stdout: stream(root, "stdout")?,
            stderr: stream(root, "stderr")?,
            exchanges,
            document,
        })
    }

    pub fn to_text(&self) -> String {
        self.document.to_string()
    }

    /// The directory a case's body files are named relative to.
    pub fn dir(&self) -> &Path {
        self.path.parent().unwrap_or(Path::new(""))
    }

    /// Rewrites the exit code, each stream that isn't closed, and the request
    /// of each unrecorded exchange whose request changed, to what a run
    /// produced. A recorded exchange whose request changed is an error, and
    /// nothing is rewritten.
    pub fn bless(
        &mut self,
        code: i32,
        stdout: &str,
        stderr: &str,
        sent: &[Request],
    ) -> Result<(), String> {
        let changed: Vec<usize> = sent
            .iter()
            .zip(&self.exchanges)
            .enumerate()
            .filter(|(_, (sent, exchange))| {
                !exchange.request.as_ref().is_some_and(|written| written.same_as(sent))
            })
            .map(|(i, _)| i)
            .collect();
        if let Some(&i) = changed.iter().find(|&&i| self.exchanges[i].recorded.is_some()) {
            return Err(format!(
                "{}: the request of recorded exchange {} changed; record it again with `just \
                 record`",
                self.path.display(),
                i + 1
            ));
        }
        let mut document = self.document.clone();
        let root = document.as_table_mut();
        root["status"]["code"] = toml_edit::value(i64::from(code));
        for (name, stream, text) in
            [("stdout", &self.stdout, stdout), ("stderr", &self.stderr, stderr)]
        {
            if *stream == Stream::Closed {
                continue;
            }
            if text.is_empty() {
                root.remove(name);
            } else {
                root[name] = Item::Value(string(text));
            }
        }
        order(root, &TOP_KEYS);
        let tables = root.get_mut("exchange").and_then(Item::as_array_of_tables_mut);
        if let Some(tables) = tables {
            for i in changed {
                if let Some(table) = tables.get_mut(i) {
                    set_request(table, &sent[i]);
                }
            }
        }
        *self = Case::parse(&document.to_string(), &self.path)?;
        Ok(())
    }

    /// Writes `exchange` as exchange number `at`, counting from 0: in place of
    /// the one there, keeping the comments above it, or after the last when
    /// `at` is one past it.
    pub fn set_exchange(&mut self, at: usize, exchange: &Exchange) -> Result<(), String> {
        let mut document = self.document.clone();
        let root = document.as_table_mut();
        if !root.contains_key("exchange") {
            root.insert("exchange", Item::ArrayOfTables(ArrayOfTables::new()));
        }
        let tables = root
            .get_mut("exchange")
            .and_then(Item::as_array_of_tables_mut)
            .ok_or("exchange must be [[exchange]] tables")?;
        let mut table = exchange_table(exchange);
        if let Some(old) = tables.get_mut(at) {
            *table.decor_mut() = old.decor().clone();
            *old = table;
        } else if at == tables.len() {
            tables.push(table);
        } else {
            return Err(format!("exchange {} is not next", at + 1));
        }
        *self = Case::parse(&document.to_string(), &self.path)?;
        Ok(())
    }
}

fn stream(root: &Table, name: &str) -> Result<Stream, String> {
    match root.get(name) {
        None => Ok(Stream::Text(String::new())),
        Some(item) => {
            if let Some(text) = item.as_str() {
                return Ok(Stream::Text(text.to_owned()));
            }
            let closed = item.as_table_like().and_then(|t| t.get("closed")).and_then(Item::as_bool);
            if closed == Some(true) {
                Ok(Stream::Closed)
            } else {
                Err(format!("{name} must be a string or closed = true"))
            }
        }
    }
}

fn text(table: &Table, key: &str) -> Result<Option<String>, String> {
    table
        .get(key)
        .map(|item| item.as_str().map(str::to_owned).ok_or(format!("{key} must be a string")))
        .transpose()
}

fn exchange(table: &Table) -> Result<Exchange, String> {
    for (key, _) in table {
        if !EXCHANGE_KEYS.contains(&key) {
            return Err(format!("unknown key {key}"));
        }
    }
    let operation = text(table, "operation")?;
    let get = text(table, "get")?;
    let request = match (operation, get) {
        (Some(_), Some(_)) => return Err("operation and get are both set".to_owned()),
        (Some(operation), None) => {
            let variables = match text(table, "variables")? {
                Some(json) => serde_json::from_str(&json)
                    .map_err(|e| format!("variables are not JSON: {e}"))?,
                None => Value::Null,
            };
            let query = text(table, "query")?.unwrap_or_default();
            Some(Request::Graphql { operation, variables, query })
        }
        (None, Some(path)) => Some(Request::Raw { path }),
        (None, None) => None,
    };
    let status = table
        .get("status")
        .and_then(Item::as_integer)
        .and_then(|status| u16::try_from(status).ok())
        .ok_or("status must be an HTTP status")?;
    let recorded = match table.get("recorded") {
        None => None,
        Some(item) => Some(recorded(item.as_table_like().ok_or("recorded must be a table")?)?),
    };
    let body = match (text(table, "response")?, text(table, "body")?) {
        (Some(_), Some(_)) => return Err("response and body are both set".to_owned()),
        (Some(text), None) => Body::Text(text),
        (None, Some(file)) => Body::File(file),
        (None, None) => Body::Text(String::new()),
    };
    Ok(Exchange { request, status, content_type: text(table, "content-type")?, recorded, body })
}

fn recorded(table: &dyn TableLike) -> Result<Recorded, String> {
    let date = table
        .get("date")
        .and_then(Item::as_datetime)
        .and_then(|datetime| datetime.date)
        .ok_or("recorded.date must be a date")?;
    let endpoint =
        table.get("endpoint").and_then(Item::as_str).ok_or("recorded.endpoint must be a string")?;
    let live_values = match table.get("live").map(Item::as_str) {
        None => false,
        Some(Some("values")) => true,
        Some(_) => return Err("recorded.live must be \"values\"".to_owned()),
    };
    Ok(Recorded { date: date.to_string(), endpoint: endpoint.to_owned(), live_values })
}

fn set_request(table: &mut Table, request: &Request) {
    for key in ["operation", "variables", "query", "get"] {
        table.remove(key);
    }
    match request {
        Request::Graphql { operation, variables, query } => {
            table["operation"] = Item::Value(string(operation));
            if !variables.is_null() {
                table["variables"] = Item::Value(string(&variables.to_string()));
            }
            table["query"] = Item::Value(string(query));
        }
        Request::Raw { path } => table["get"] = Item::Value(string(path)),
    }
    order(table, &EXCHANGE_KEYS);
}

fn exchange_table(exchange: &Exchange) -> Table {
    let mut table = Table::new();
    if let Some(request) = &exchange.request {
        set_request(&mut table, request);
    }
    table["status"] = toml_edit::value(i64::from(exchange.status));
    if let Some(content_type) = &exchange.content_type {
        table["content-type"] = Item::Value(string(content_type));
    }
    if let Some(recorded) = &exchange.recorded {
        let mut stamp = toml_edit::InlineTable::new();
        let date = recorded.date.parse::<toml_edit::Datetime>();
        match date {
            Ok(date) => stamp.insert("date", date.into()),
            Err(_) => stamp.insert("date", recorded.date.as_str().into()),
        };
        stamp.insert("endpoint", recorded.endpoint.as_str().into());
        if recorded.live_values {
            stamp.insert("live", "values".into());
        }
        table["recorded"] = toml_edit::value(stamp);
    }
    match &exchange.body {
        Body::Text(text) => table["response"] = Item::Value(string(text)),
        Body::File(file) => table["body"] = Item::Value(string(file)),
    }
    table
}

/// Puts the table's keys in the order of `keys`, leaving the rest after them.
fn order(table: &mut Table, keys: &[&str]) {
    let rank = |key: &str| keys.iter().position(|k| *k == key).unwrap_or(keys.len());
    table.sort_values_by(|a, _, b, _| rank(a.get()).cmp(&rank(b.get())));
}

/// `text` as a TOML string written to be read: text that ends a line is a
/// multi-line literal string where it can be one, so stdout and JSON read as
/// they print, and otherwise
/// a basic string that escapes every character sego never prints as itself.
fn string(text: &str) -> toml_edit::Value {
    let plain = |c: char| c == '\n' || c == '\t' || !(is_unsafe(c) || c.is_control());
    let literal = text.chars().all(plain) && !text.contains("'''") && !text.ends_with('\'');
    let repr = if literal && text.ends_with('\n') {
        format!("'''\n{text}'''")
    } else if literal && text.contains(['\\', '"']) && !text.contains('\'') {
        format!("'{text}'")
    } else {
        basic(text)
    };
    repr.parse().unwrap_or_else(|_| toml_edit::Value::from(text))
}

/// `text` as a basic string, multi-line if it ends a line.
fn basic(text: &str) -> String {
    let multi_line = text.ends_with('\n');
    let quotes = if multi_line { "\"\"\"" } else { "\"" };
    let mut out = String::from(quotes);
    if multi_line {
        out.push('\n');
    }
    let mut previous = None;
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' if multi_line && previous != Some('"') => out.push(c),
            '"' => out.push_str("\\\""),
            '\n' if multi_line => out.push(c),
            '\n' => out.push_str("\\n"),
            '\t' => out.push(c),
            '\r' => out.push_str("\\r"),
            c if is_unsafe(c) || c.is_control() => {
                let _ = if u32::from(c) <= 0xFFFF {
                    write!(out, "\\u{:04X}", u32::from(c))
                } else {
                    write!(out, "\\U{:08X}", u32::from(c))
                };
            }
            c => out.push(c),
        }
        previous = Some(c);
    }
    out.push_str(quotes);
    out
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn parse(text: &str) -> Case {
        Case::parse(text, Path::new("refs/x.toml")).unwrap_or_else(|e| panic!("{e}\n{text}"))
    }

    const EXCHANGE: &str = r#"
[[exchange]]
operation = "Refs"
variables = '{"first":101,"repo0":"example.com/a/b"}'
query = '''
query Refs($repo0: String!) {
  r0: repository(name: $repo0) { name }
}
'''
status = 200
content-type = "application/json"
recorded = { date = 2026-10-07, endpoint = "https://sourcegraph.com", live = "values" }
response = '''
{"data": {"r0": {"name": "example.com/a/b"}}}
'''
"#;

    fn refs(repo: &str, query: &str) -> Request {
        Request::Graphql {
            operation: "Refs".to_owned(),
            variables: json!({"first": 101, "repo0": repo}),
            query: query.to_owned(),
        }
    }

    #[test]
    fn a_case_file_parses() {
        let case = parse(
            "# a comment\nargs = [\"show\", \"a b\"]\nenv = { SEGO_ENDPOINT = \"http://x\" }\n\
             status.code = 2\nstdout.closed = true\nstderr = '''\nerr\n'''\n",
        );
        assert_eq!(case.args, ["show", "a b"]);
        assert_eq!(case.env, [("SEGO_ENDPOINT".to_owned(), "http://x".to_owned())]);
        assert_eq!(case.code, 2);
        assert_eq!(case.stdout, Stream::Closed);
        assert_eq!(case.stderr, Stream::Text("err\n".to_owned()));
        assert_eq!(case.exchanges, []);
    }

    #[test]
    fn an_exchange_holds_its_request_response_and_recording() {
        let case = parse(&format!("args = [\"x\"]\nstatus.code = 0\n{EXCHANGE}"));
        let [exchange] = &case.exchanges[..] else { panic!("{:?}", case.exchanges) };
        assert!(exchange.request.as_ref().is_some_and(|r| r.same_as(&refs(
            "example.com/a/b",
            "query Refs($repo0: String!) { r0: repository(name: $repo0) { name } }"
        ))));
        assert_eq!(exchange.status, 200);
        assert_eq!(exchange.content_type.as_deref(), Some("application/json"));
        let recorded = Recorded {
            date: "2026-10-07".to_owned(),
            endpoint: "https://sourcegraph.com".to_owned(),
            live_values: true,
        };
        assert_eq!(exchange.recorded, Some(recorded));
        assert!(matches!(&exchange.body, Body::Text(text) if text.starts_with("{\"data\"")));
    }

    #[test]
    fn a_raw_read_names_its_body_file_and_a_response_may_stand_alone() {
        let case = parse(
            "args = [\"x\"]\nstatus.code = 0\n\n[[exchange]]\nget = \"/r/x\"\nstatus = 404\n\
             body = \"x.body\"\n\n[[exchange]]\nstatus = 200\nresponse = \"{}\"\n",
        );
        assert_eq!(case.exchanges[0].request, Some(Request::Raw { path: "/r/x".to_owned() }));
        assert_eq!(case.exchanges[0].body, Body::File("x.body".to_owned()));
        assert_eq!(case.dir().join("x.body"), Path::new("refs/x.body"));
        assert_eq!(case.exchanges[1].request, None);
    }

    #[test]
    fn a_case_with_an_unknown_key_or_a_missing_field_is_refused() {
        let refused = |text: &str| Case::parse(text, Path::new("x.toml")).is_err();
        assert!(refused("args = [\"x\"]\nstatus.code = 0\nexit = 1\n"));
        assert!(refused("status.code = 0\n"));
        assert!(refused("args = [\"x\"]\n"));
        assert!(refused("args = [\"x\"]\nstatus.code = 0\nstdout.closed = false\n"));
        assert!(refused("args = [\"x\"]\nstatus.code = 0\n\n[[exchange]]\nresponse = \"\"\n"));
        assert!(refused(
            "args = [\"x\"]\nstatus.code = 0\n\n[[exchange]]\nstatus = 200\nrecorded = { date \
             = 2026-10-07, endpoint = \"e\", live = \"shape\" }\n"
        ));
    }

    #[test]
    fn bless_rewrites_the_exit_code_and_streams_and_keeps_the_rest() {
        let text = "# keep\nargs = [\"x\"]\nstatus.code = 0\nstdout = '''\nold\n'''\n";
        let mut case = parse(text);
        case.bless(1, "", "sego: error: e\n", &[]).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            case.to_text(),
            "# keep\nargs = [\"x\"]\nstatus.code = 1\nstderr = '''\nsego: error: e\n'''\n"
        );
        case.bless(1, "out\n", "sego: error: e\n", &[]).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            case.to_text(),
            "# keep\nargs = [\"x\"]\nstatus.code = 1\nstdout = '''\nout\n'''\nstderr = \
             '''\nsego: error: e\n'''\n"
        );
    }

    #[test]
    fn bless_leaves_a_closed_stream_alone() {
        let text = "args = [\"x\"]\nstatus.code = 0\nstdout.closed = true\n";
        let mut case = parse(text);
        case.bless(0, "", "", &[]).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(case.to_text(), text);
    }

    #[test]
    fn bless_writes_the_request_of_an_unrecorded_exchange_in_order() {
        let mut case = parse(
            "args = [\"x\"]\nstatus.code = 0\n\n# hand-written\n[[exchange]]\nstatus = 200\n\
             response = \"{}\"\n",
        );
        let sent = refs("example.com/c/d", "query Refs { r0 }\n");
        case.bless(0, "", "", std::slice::from_ref(&sent)).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(case.exchanges[0].request, Some(sent));
        let text = case.to_text();
        assert!(text.contains("# hand-written\n[[exchange]]\noperation = \"Refs\"\n"), "{text}");
        let raw = Request::Raw { path: "/r".to_owned() };
        case.bless(0, "", "", std::slice::from_ref(&raw)).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(case.exchanges[0].request, Some(raw));
        assert!(!case.to_text().contains("operation"));
    }

    #[test]
    fn bless_refuses_a_recorded_exchange_whose_request_changed() {
        let text = format!("args = [\"x\"]\nstatus.code = 0\n{EXCHANGE}");
        let mut case = parse(&text);
        let error = case
            .bless(0, "", "", &[refs("example.com/c/d", "query")])
            .err()
            .unwrap_or_else(|| panic!("blessed"));
        assert!(error.contains("just record"), "{error}");
        assert_eq!(case.to_text(), text);
        let same = refs(
            "example.com/a/b",
            "query Refs($repo0: String!) {\n  r0: repository(name: $repo0) { name }\n}\n",
        );
        case.bless(0, "", "", &[same]).unwrap_or_else(|e| panic!("{e}"));
    }

    #[test]
    fn an_exchange_is_set_in_place_or_after_the_last() {
        let mut case = parse(&format!("args = [\"x\"]\nstatus.code = 0\n\n# kept\n{EXCHANGE}"));
        let exchange = Exchange {
            request: Some(Request::Raw { path: "/r".to_owned() }),
            status: 200,
            content_type: Some("text/plain".to_owned()),
            recorded: None,
            body: Body::File("x.body".to_owned()),
        };
        case.set_exchange(1, &exchange).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(case.exchanges.len(), 2);
        case.set_exchange(0, &exchange).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(case.exchanges, [exchange.clone(), exchange.clone()]);
        assert!(
            case.to_text().contains("# kept\n\n[[exchange]]\nget = \"/r\""),
            "{}",
            case.to_text()
        );
        assert!(case.set_exchange(3, &exchange).is_err());
    }

    #[test]
    fn text_is_written_to_read_back_exactly() {
        for text in [
            "plain",
            "a line\n",
            "no final newline\nhere",
            "it's \"quoted\" \\ and '''\n",
            "tab\tand\u{1b}escape\u{202e}\r\n",
            "ends in a quote'\n",
            "\"\"\"\n",
            "",
        ] {
            let mut case = parse("args = [\"x\"]\nstatus.code = 0\n");
            case.bless(0, text, "", &[]).unwrap_or_else(|e| panic!("{e}"));
            let read = parse(&case.to_text());
            let expected = Stream::Text(text.to_owned());
            assert_eq!(read.stdout, expected, "{}", case.to_text());
            assert!(!case.to_text().contains(['\u{1b}', '\u{202e}', '\r']), "{}", case.to_text());
        }
    }
}
