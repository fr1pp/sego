use std::io;
use std::time::Instant;

use serde::Serialize;
use serde_json::{Value, json};

use crate::address::{Address, End, LineRange, pinned, rev_field};
use crate::error::Error;
use crate::escape::strip_cr;
use crate::output::text::{code_line, gutter_width, header};
use crate::output::{Answer, Marker, Notes};
use crate::submodule::{self, Submodule};
use crate::upstream::{
    Candidate, GRAPHQL_PATH, Located, Plan, RawError, Transport, Var, View,
    build, classify, locate, raw,
};

const DEFAULT_WINDOW: u32 = 500;
const OPEN_END_SPAN: i64 = 500_000;
const CONTENT_LIMIT: u64 = 1_572_864;
const LIMIT_MESSAGE: &str = "file exceeded size limit";

const COMMIT_TEMPLATE: &str = "\
meta: blob(path: $path) { byteSize totalLines binary rawURL lfs { byteSize } }
      body: blob(path: $path) { content(startLine: $start, endLine: $end, textOnly: true) }
      dir: tree(path: $path) { entries { path } }
      parent: tree(path: $parent) { entries { path submodule { url commit } } }";

#[derive(Serialize)]
pub struct Line {
    pub line: u32,
    pub text: String,
}

#[derive(Serialize)]
pub struct Shown {
    pub repo: String,
    pub rev: Option<String>,
    pub commit: String,
    pub path: String,
    pub total_lines: u32,
    pub byte_size: u64,
    pub binary: bool,
    pub crlf: bool,
    pub lfs_size: Option<u64>,
    pub submodule: Option<Submodule>,
    pub start: Option<u32>,
    pub end: Option<u32>,
    pub lines: Vec<Line>,
    pub via_raw: bool,
    #[serde(skip)]
    fields: Vec<String>,
}

impl Answer for Shown {
    fn write_json(&self, out: &mut dyn io::Write) -> io::Result<()> {
        crate::output::json::write(self, out)
    }

    fn text(&self) -> String {
        let address = pinned(&self.repo, &self.commit, Some(&self.path));
        let mut out = header(&address, &self.fields);
        let width = gutter_width(self.end.unwrap_or(0));
        for line in &self.lines {
            out.push_str(&code_line(
                line.line,
                width,
                Marker::Match,
                &line.text,
                None,
            ));
        }
        out
    }
}

/// What the caller asked for, as the server is asked and as it is reported.
struct Window {
    start: u32,
    end: Option<u32>,
    shown: Option<LineRange>,
}

impl Window {
    fn new(lines: Option<&LineRange>) -> Window {
        match lines {
            None => {
                Window { start: 1, end: Some(DEFAULT_WINDOW), shown: None }
            }
            Some(range) => Window {
                start: range.start,
                end: match range.end {
                    End::Line(end) => Some(end),
                    End::Open => None,
                },
                shown: Some(*range),
            },
        }
    }

    fn sent_end(&self) -> i64 {
        self.end.map_or_else(
            || {
                (i64::from(self.start) + OPEN_END_SPAN)
                    .min(i64::from(i32::MAX))
            },
            i64::from,
        )
    }
}

fn plan(window: &Window) -> Plan {
    let (start, end) = (i64::from(window.start), window.sent_end());
    Plan {
        operation: "ShowFile",
        commit_template: COMMIT_TEMPLATE,
        vars: Box::new(move |candidate: &Candidate| {
            let path = candidate.path.clone().unwrap_or_default();
            let parent =
                path.rsplit_once('/').map_or("", |(parent, _)| parent);
            vec![
                Var { name: "parent", ty: "String!", value: json!(parent) },
                Var { name: "path", ty: "String!", value: json!(path) },
                Var { name: "start", ty: "Int!", value: json!(start) },
                Var { name: "end", ty: "Int!", value: json!(end) },
            ]
        }),
    }
}

pub fn run(
    address: &Address,
    lines: Option<&LineRange>,
    transport: &dyn Transport,
    deadline: Instant,
    notes: &mut Notes,
) -> Result<Shown, Error> {
    let window = Window::new(lines);
    let request = build(&plan(&window), address);
    let reply = transport.post(GRAPHQL_PATH, &request.body(), deadline)?;
    let envelope = classify(reply)?;
    let located = locate(address, &envelope)?;
    let view = View::new(&envelope, &located.at);
    interpret(&view, &located, &window, transport, deadline, notes)
}

fn limit_exceeded(view: &View<'_>, tail: &[&str]) -> bool {
    view.error(tail).is_some_and(|error| error.message.contains(LIMIT_MESSAGE))
}

fn not_found(located: &Located, path: &str) -> Error {
    Error::missing(format!(
        "{path} not found in {}",
        pinned(&located.repo, &located.commit, None)
    ))
}

fn submodule_of(
    view: &View<'_>,
    located: &Located,
    path: &str,
) -> Option<Submodule> {
    let entries = view.get(&["parent", "entries"])?.as_array()?;
    submodule::find(entries, path, &located.repo)
}

/// The file's `meta`, once the path is known to be a readable file or a
/// submodule's parent. Everything else the response can mean is an error.
fn file_meta<'a>(
    view: &'a View<'_>,
    located: &Located,
    path: &str,
) -> Result<&'a Value, Error> {
    let meta = view.get(&["meta"]);
    let body = view.get(&["body"]);
    let dir = view.get(&["dir"]);
    let (meta_failed, body_failed) =
        (view.errored(&["meta"]), view.errored(&["body"]));
    if meta_failed
        && body_failed
        && let Some(dir) = dir
    {
        let count =
            dir.get("entries").and_then(Value::as_array).map_or(0, Vec::len);
        let unit = if count == 1 { "entry" } else { "entries" };
        let name = if path.is_empty() { "./" } else { path };
        return Err(Error::missing(format!(
            "{name} is a directory at {} ({count} {unit})",
            pinned(&located.repo, &located.commit, None)
        )));
    }
    let silent = !(meta_failed || body_failed || view.errored(&["dir"]));
    if silent && meta.is_none() && body.is_none() && dir.is_none() {
        return Err(not_found(located, path));
    }
    let submodule_open =
        meta_failed || body_failed || meta.is_none() || body.is_none();
    if submodule_open && view.errored(&["parent"]) {
        return Err(Error::failed("the directory listing"));
    }
    meta.ok_or_else(|| Error::failed("the file's details"))
}

struct Details {
    byte_size: u64,
    binary: bool,
    reported_lines: u32,
    lfs_size: Option<u64>,
}

fn details_of(view: &View<'_>, meta: &Value) -> Result<Details, Error> {
    let details = || Error::failed("the file's details");
    if view.errored(&["meta", "lfs"])
        && !limit_exceeded(view, &["meta", "lfs"])
    {
        return Err(details());
    }
    let lfs_size = match view.get(&["meta", "lfs"]) {
        Some(lfs) => Some(
            lfs.get("byteSize")
                .and_then(Value::as_str)
                .and_then(|s| s.parse::<u64>().ok())
                .ok_or_else(details)?,
        ),
        None => None,
    };
    Ok(Details {
        byte_size: meta
            .get("byteSize")
            .and_then(Value::as_u64)
            .ok_or_else(details)?,
        binary: meta
            .get("binary")
            .and_then(Value::as_bool)
            .ok_or_else(details)?,
        reported_lines: meta
            .get("totalLines")
            .and_then(Value::as_u64)
            .and_then(|n| u32::try_from(n).ok())
            .ok_or_else(details)?,
        lfs_size,
    })
}

fn interpret(
    view: &View<'_>,
    located: &Located,
    window: &Window,
    transport: &dyn Transport,
    deadline: Instant,
    notes: &mut Notes,
) -> Result<Shown, Error> {
    let path = located.path.clone().unwrap_or_default();
    let mut shown = Shown {
        repo: located.repo.clone(),
        rev: located.rev.clone(),
        commit: located.commit.clone(),
        path: path.clone(),
        total_lines: 0,
        byte_size: 0,
        binary: false,
        crlf: false,
        lfs_size: None,
        submodule: None,
        start: None,
        end: None,
        lines: Vec::new(),
        via_raw: false,
        fields: Vec::new(),
    };
    let rev = rev_field(located.rev.as_ref());

    if let Some(submodule) = submodule_of(view, located, &path) {
        shown.fields.push(format!("submodule  {}", submodule.shown()));
        shown.fields.extend(rev);
        shown.submodule = Some(submodule);
        return Ok(shown);
    }

    let meta = file_meta(view, located, &path)?;
    let details = details_of(view, meta)?;
    shown.byte_size = details.byte_size;
    shown.binary = details.binary;
    shown.lfs_size = details.lfs_size;
    if shown.binary {
        shown.fields.push(format!("binary  {} bytes", shown.byte_size));
        shown.fields.extend(rev);
        return Ok(shown);
    }
    let total = if shown.byte_size == 0 { 0 } else { details.reported_lines };
    shown.total_lines = total;
    if let Some(size) = shown.lfs_size {
        shown.fields.push(format!("LFS pointer  object {size} bytes"));
    }

    let first = window.start;
    let last = window.end.unwrap_or(total).min(total);
    if first <= total {
        let content = if limit_exceeded(view, &["body", "content"]) {
            let raw = meta
                .get("rawURL")
                .and_then(Value::as_str)
                .filter(|raw| raw.starts_with('/') && !raw.starts_with("//"))
                .ok_or_else(|| Error::failed("the file's details"))?;
            notes.push(format!(
                "{path} is {} bytes, over GraphQL's {CONTENT_LIMIT}-byte \
                 content limit; read through the raw route (a second \
                 request)",
                shown.byte_size
            ));
            shown.via_raw = true;
            let whole = match raw::read(transport, raw, deadline) {
                Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
                Err(RawError::NotFound) => {
                    return Err(not_found(located, &path));
                }
                Err(RawError::Failed(error)) => return Err(error),
            };
            lines_from(&whole, first)
        } else {
            view.get(&["body", "content"])
                .and_then(Value::as_str)
                .ok_or_else(|| Error::failed("the file content"))?
                .to_owned()
        };
        let wanted = (last - first + 1) as usize;
        let lines = split_lines(&content).into_iter().take(wanted);
        for (number, text) in (first..).zip(lines) {
            let (text, cr) = strip_cr(text);
            shown.crlf |= cr;
            shown.lines.push(Line { line: number, text: text.to_owned() });
        }
        if shown.lines.len() != wanted {
            return Err(Error::failed("the file content"));
        }
        shown.start = Some(first);
        shown.end = Some(last);
    }
    describe(&mut shown, window, notes);
    if shown.crlf {
        shown.fields.push("CRLF".to_owned());
    }
    shown.fields.extend(rev);
    Ok(shown)
}

/// The header's range field and the notes about a range that was cut.
fn describe(shown: &mut Shown, window: &Window, notes: &mut Notes) {
    let total = shown.total_lines;
    if let (Some(start), Some(end)) = (shown.start, shown.end) {
        let range = if start == end {
            start.to_string()
        } else {
            format!("{start}-{end}")
        };
        shown.fields.push(format!("lines {range} of {total}"));
        if let Some(asked) = &window.shown
            && let End::Line(asked_end) = asked.end
            && asked_end > end
        {
            notes.push(format!(
                "lines {} clamped to {range}; the file has {total} lines",
                asked.display()
            ));
        }
        return;
    }
    if total == 0 {
        shown.fields.push("lines 0 of 0".to_owned());
    }
    if let Some(asked) = window.shown.as_ref().map(LineRange::display) {
        let message = format!("no lines {asked}; the file has {total}");
        if total > 0 {
            shown.fields.push(message.clone());
        }
        notes.push(message);
    }
}

/// The text of `whole` from line `first` on, as the server's `content`
/// would give it.
fn lines_from(whole: &str, first: u32) -> String {
    let skip = (first - 1) as usize;
    whole.split_inclusive('\n').skip(skip).collect()
}

fn split_lines(content: &str) -> Vec<&str> {
    let mut lines: Vec<&str> = content.split('\n').collect();
    if lines.last() == Some(&"") {
        lines.pop();
    }
    lines
}
