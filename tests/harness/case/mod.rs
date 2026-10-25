use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::shell;

module_tests!();

/// A case file: its header, its expected answer and its exchanges. The file's
/// text is kept as it was read, so a rewrite touches only the parts it owns.
#[derive(Debug)]
pub struct Case {
    pub path: PathBuf,
    pub args: Vec<Vec<u8>>,
    pub env: Vec<(String, String)>,
    pub exit: i32,
    pub stdout_closed: bool,
    pub stdout: String,
    pub stderr: String,
    pub exchanges: Vec<Exchange>,
    document: Document,
}

/// A request sego sends and the response it gets. A response written alone
/// has no request.
#[derive(Clone, Debug, PartialEq)]
pub struct Exchange {
    pub request: Option<Request>,
    pub response: Response,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    Graphql { operation: String, variables: Value, query: String },
    Raw { method: String, path: String },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Response {
    pub served: Served,
    pub stamp: Option<Stamp>,
    /// The text under the response line; empty for a raw read, whose body is
    /// the case's `.body` file.
    pub body: String,
}

impl Response {
    fn marker(&self) -> String {
        let mut line = match &self.served {
            Served::Reply { status, content_type, headers } => {
                let mut line = format!("--- response {status}");
                if let Some(content_type) = content_type {
                    let _ = write!(line, " {content_type}");
                }
                for (name, value) in [
                    ("location", &headers.location),
                    ("retry-after", &headers.retry_after),
                    ("cf-mitigated", &headers.cf_mitigated),
                ] {
                    if let Some(value) = value {
                        let _ = write!(line, " {name}: {value}");
                    }
                }
                line
            }
            Served::Fault(Fault::Network) => {
                "--- response network error".to_owned()
            }
            Served::Fault(Fault::Redirect) => {
                "--- response redirect".to_owned()
            }
            Served::Fault(Fault::HtmlChallenge) => {
                "--- response html challenge".to_owned()
            }
        };
        if let Some(Stamp { date, endpoint, live_values }) = &self.stamp {
            let _ = write!(line, " recorded {date} {endpoint}");
            if *live_values {
                line.push_str(" live: values");
            }
        }
        line.push('\n');
        line
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Served {
    Reply { status: u16, content_type: Option<String>, headers: Headers },
    Fault(Fault),
}

/// The response headers besides the content type that sego reads.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Headers {
    pub location: Option<String>,
    pub retry_after: Option<String>,
    pub cf_mitigated: Option<String>,
}

/// A way the transport fails, written as the response line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    Network,
    Redirect,
    HtmlChallenge,
}

/// When and where a response was recorded, and whether `just live` compares
/// its values and not only its shape.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stamp {
    pub date: String,
    pub endpoint: String,
    pub live_values: bool,
}

pub fn normalise(query: &str) -> String {
    query.split_whitespace().collect::<Vec<_>>().join(" ")
}

impl Request {
    /// Whether `other` is the same request: the same operation and variables
    /// and the same query up to whitespace, or the same raw read.
    pub fn same_as(&self, other: &Request) -> bool {
        match (self, other) {
            (
                Request::Graphql { operation, variables, query },
                Request::Graphql {
                    operation: other_operation,
                    variables: other_variables,
                    query: other_query,
                },
            ) => {
                operation == other_operation
                    && variables == other_variables
                    && normalise(query) == normalise(other_query)
            }
            (
                Request::Raw { method, path },
                Request::Raw { method: other_method, path: other_path },
            ) => method == other_method && path == other_path,
            _ => false,
        }
    }

    /// The request as text for a failure message.
    pub fn describe(&self) -> String {
        match self {
            Request::Graphql { operation, variables, query } => {
                format!("{operation} {variables}\n{}", normalise(query))
            }
            Request::Raw { method, path } => format!("{method} {path}"),
        }
    }

    fn sections(&self) -> Vec<Section> {
        let mut sections =
            vec![Section { marker: self.marker(), body: String::new() }];
        if let Request::Graphql { query, .. } = self {
            let mut body = query.clone();
            if !body.ends_with('\n') {
                body.push('\n');
            }
            sections.push(Section { marker: "--- query\n".to_owned(), body });
        }
        sections
    }

    fn marker(&self) -> String {
        match self {
            Request::Graphql { operation, variables, .. } => {
                if variables.is_null() {
                    format!("--- request {operation}\n")
                } else {
                    format!("--- request {operation} {variables}\n")
                }
            }
            Request::Raw { method, path } => {
                format!("--- request {method} {path}\n")
            }
        }
    }
}

#[derive(Clone, Debug)]
struct Document {
    header: String,
    sections: Vec<Section>,
}

/// A section's marker line, with its line ending, and the lines under it.
#[derive(Clone, Debug)]
struct Section {
    marker: String,
    body: String,
}

enum Marker<'a> {
    Stdout,
    Stderr,
    Query,
    Request(&'a str),
    Response(&'a str),
}

/// Whether `line` starts a section of a case file.
pub fn is_marker(line: &str) -> bool {
    marker(line).is_some()
}

fn marker(line: &str) -> Option<Marker<'_>> {
    let line = line.trim_end_matches(['\r', '\n']);
    match line {
        "--- stdout" => return Some(Marker::Stdout),
        "--- stderr" => return Some(Marker::Stderr),
        "--- query" => return Some(Marker::Query),
        "--- request" => return Some(Marker::Request("")),
        "--- response" => return Some(Marker::Response("")),
        _ => {}
    }
    if let Some(rest) = line.strip_prefix("--- request ") {
        Some(Marker::Request(rest))
    } else {
        line.strip_prefix("--- response ").map(Marker::Response)
    }
}

impl Document {
    fn parse(text: &str) -> Document {
        let mut header = String::new();
        let mut sections: Vec<Section> = Vec::new();
        for line in text.split_inclusive('\n') {
            if marker(line).is_some() {
                sections.push(Section {
                    marker: line.to_owned(),
                    body: String::new(),
                });
            } else if let Some(last) = sections.last_mut() {
                last.body.push_str(line);
            } else {
                header.push_str(line);
            }
        }
        Document { header, sections }
    }

    fn render(&self) -> String {
        let mut text = self.header.clone();
        for section in &self.sections {
            text.push_str(&section.marker);
            text.push_str(&section.body);
        }
        text
    }

    fn set_exit(&mut self, exit: i32) {
        let mut header = String::new();
        for line in self.header.split_inclusive('\n') {
            if line.starts_with("exit:") {
                let eol = &line[line.trim_end_matches(['\r', '\n']).len()..];
                let _ = write!(header, "exit: {exit}{eol}");
            } else {
                header.push_str(line);
            }
        }
        self.header = header;
    }

    fn set_stream(&mut self, name: &str, text: &str) {
        let found = self.sections.iter().position(|section| {
            match marker(&section.marker) {
                Some(Marker::Stdout) => name == "stdout",
                Some(Marker::Stderr) => name == "stderr",
                _ => false,
            }
        });
        match (found, text.is_empty()) {
            (Some(i), true) => {
                self.sections.remove(i);
            }
            (Some(i), false) => text.clone_into(&mut self.sections[i].body),
            (None, true) => {}
            (None, false) => {
                let after_stdout = name == "stderr"
                    && self.sections.first().is_some_and(|section| {
                        matches!(marker(&section.marker), Some(Marker::Stdout))
                    });
                self.sections.insert(
                    usize::from(after_stdout),
                    Section {
                        marker: format!("--- {name}\n"),
                        body: text.to_owned(),
                    },
                );
            }
        }
    }

    /// The indexes of each exchange's request, query and response sections.
    fn exchange_spans(&self) -> Vec<[Option<usize>; 3]> {
        let mut spans = Vec::new();
        let mut open = [None; 3];
        for (i, section) in self.sections.iter().enumerate() {
            match marker(&section.marker) {
                Some(Marker::Request(_)) => open[0] = Some(i),
                Some(Marker::Query) => open[1] = Some(i),
                Some(Marker::Response(_)) => {
                    open[2] = Some(i);
                    spans.push(std::mem::take(&mut open));
                }
                _ => {}
            }
        }
        spans
    }

    fn set_request(&mut self, exchange: usize, request: &Request) {
        let [request_at, query_at, response_at] =
            self.exchange_spans()[exchange];
        let first = request_at.or(query_at).or(response_at).unwrap_or(0);
        for at in [query_at, request_at].into_iter().flatten() {
            self.sections.remove(at);
        }
        self.sections.splice(first..first, request.sections());
    }

    /// Writes `exchange` as exchange number `at`: in place of the one there,
    /// or after the last when `at` is one past it.
    fn set_exchange(
        &mut self,
        at: usize,
        exchange: &Exchange,
    ) -> Result<(), String> {
        let mut sections: Vec<Section> =
            exchange.request.iter().flat_map(Request::sections).collect();
        sections.push(Section {
            marker: exchange.response.marker(),
            body: exchange.response.body.clone(),
        });
        let spans = self.exchange_spans();
        if let Some(span) = spans.get(at) {
            let first = span.iter().flatten().min().copied().unwrap_or(0);
            let last = span[2].unwrap_or(first);
            self.sections.splice(first..=last, sections);
        } else if at == spans.len() {
            if !self.render().is_empty() && !self.render().ends_with('\n') {
                return Err("the file does not end in a newline".to_owned());
            }
            self.sections.extend(sections);
        } else {
            return Err(format!("exchange {} is not next", at + 1));
        }
        Ok(())
    }
}

impl Case {
    pub fn parse(text: &str, path: &Path) -> Result<Case, String> {
        let document = Document::parse(text);
        let mut args = None;
        let mut env = Vec::new();
        let mut exit = None;
        let mut stdout_closed = false;
        for line in document.header.lines() {
            if line.trim().is_empty() || line.starts_with('#') {
                continue;
            }
            let (key, value) = line
                .split_once(':')
                .map(|(key, value)| (key, value.trim()))
                .ok_or_else(|| format!("not a key: value line: {line}"))?;
            match key {
                "args" => args = Some(shell::split(value)?),
                "env" => {
                    let (name, value) = value
                        .split_once('=')
                        .ok_or_else(|| format!("env needs K=V: {line}"))?;
                    env.push((name.to_owned(), value.to_owned()));
                }
                "exit" => {
                    exit = Some(
                        value
                            .parse::<i32>()
                            .map_err(|_| format!("bad exit: {value}"))?,
                    );
                }
                "stdout" if value == "closed" => stdout_closed = true,
                "stdout" => return Err(format!("stdout: {value}")),
                _ => return Err(format!("unknown key: {key}")),
            }
        }
        let (stdout, stderr, exchanges) = sections(&document)?;
        Ok(Case {
            path: path.to_path_buf(),
            args: args.ok_or("no args: line")?,
            env,
            exit: exit.ok_or("no exit: line")?,
            stdout_closed,
            stdout,
            stderr,
            exchanges,
            document,
        })
    }

    pub fn load(path: &Path) -> Result<Case, String> {
        let text = fs::read_to_string(path)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        Case::parse(&text, path)
            .map_err(|e| format!("{}: {e}", path.display()))
    }

    /// The file's text, byte for byte as it was read or last blessed.
    pub fn to_text(&self) -> String {
        self.document.render()
    }

    /// Where the bodies of this case's raw reads are kept.
    pub fn body_path(&self) -> PathBuf {
        self.path.with_extension("body")
    }

    /// Rewrites the exit code, stdout (unless it is closed) and stderr, and
    /// the request and query of each exchange without a recording stamp, to
    /// what a run produced. A recorded exchange whose request changed is an
    /// error and nothing is rewritten.
    pub fn bless(
        &mut self,
        exit: i32,
        stdout: Option<&str>,
        stderr: &str,
        sent: &[Request],
    ) -> Result<(), String> {
        for text in stdout.into_iter().chain([stderr]) {
            if !text.is_empty() && !text.ends_with('\n') {
                return Err(format!(
                    "{}: output does not end in a newline, which a case \
                     cannot hold",
                    self.path.display()
                ));
            }
        }
        let changed: Vec<usize> = sent
            .iter()
            .zip(&self.exchanges)
            .enumerate()
            .filter(|(_, (sent, exchange))| {
                !exchange.request.as_ref().is_some_and(|r| r.same_as(sent))
            })
            .map(|(i, _)| i)
            .collect();
        if let Some(&i) = changed
            .iter()
            .find(|&&i| self.exchanges[i].response.stamp.is_some())
        {
            return Err(format!(
                "{}: the request of recorded exchange {} changed; record it \
                 again with `just record`",
                self.path.display(),
                i + 1
            ));
        }
        let mut document = self.document.clone();
        document.set_exit(exit);
        if let Some(stdout) = stdout {
            document.set_stream("stdout", stdout);
        }
        document.set_stream("stderr", stderr);
        for i in changed {
            document.set_request(i, &sent[i]);
        }
        *self = Case::parse(&document.render(), &self.path)?;
        Ok(())
    }

    /// Writes `exchange` as exchange number `at` (counting from 0): in place
    /// of the one there, or after the last when `at` is one past it. The rest
    /// of the file is kept as it was.
    pub fn set_exchange(
        &mut self,
        at: usize,
        exchange: &Exchange,
    ) -> Result<(), String> {
        let mut document = self.document.clone();
        document.set_exchange(at, exchange)?;
        *self = Case::parse(&document.render(), &self.path)?;
        Ok(())
    }
}

type Parts = (String, String, Vec<Exchange>);

fn sections(document: &Document) -> Result<Parts, String> {
    let (mut stdout, mut stderr) = (None, None);
    let mut exchanges = Vec::new();
    let mut open: Option<Request> = None;
    let mut query_seen = false;
    for section in &document.sections {
        let Some(found) = marker(&section.marker) else { continue };
        match found {
            Marker::Stdout | Marker::Stderr => {
                let slot = if matches!(found, Marker::Stdout) {
                    &mut stdout
                } else {
                    &mut stderr
                };
                if !exchanges.is_empty() || open.is_some() {
                    return Err("stdout and stderr come before the exchanges"
                        .to_owned());
                }
                if slot.replace(section.body.clone()).is_some() {
                    return Err(format!(
                        "two {} sections",
                        section.marker.trim_end()
                    ));
                }
            }
            Marker::Request(rest) => {
                if open.is_some() {
                    return Err("a request without a response".to_owned());
                }
                open = Some(request(rest)?);
                query_seen = false;
            }
            Marker::Query => {
                match &mut open {
                    Some(Request::Graphql { query, .. }) if !query_seen => {
                        query.clone_from(&section.body);
                    }
                    Some(Request::Graphql { .. }) => {
                        return Err("two query sections".to_owned());
                    }
                    Some(Request::Raw { .. }) => {
                        return Err("a raw read has no query".to_owned());
                    }
                    None => {
                        return Err("a query without a request".to_owned());
                    }
                }
                query_seen = true;
            }
            Marker::Response(rest) => {
                exchanges.push(Exchange {
                    request: open.take(),
                    response: response(rest, &section.body)?,
                });
            }
        }
    }
    if open.is_some() {
        return Err("a request without a response".to_owned());
    }
    Ok((stdout.unwrap_or_default(), stderr.unwrap_or_default(), exchanges))
}

fn request(rest: &str) -> Result<Request, String> {
    let (first, tail) = rest.split_once(' ').unwrap_or((rest, ""));
    let tail = tail.trim();
    if first.is_empty() {
        return Err("a request needs an operation or a method".to_owned());
    }
    if tail.starts_with('/') {
        return Ok(Request::Raw {
            method: first.to_owned(),
            path: tail.to_owned(),
        });
    }
    let variables = if tail.is_empty() {
        Value::Null
    } else {
        serde_json::from_str(tail)
            .map_err(|e| format!("variables are not JSON: {e}"))?
    };
    Ok(Request::Graphql {
        operation: first.to_owned(),
        variables,
        query: String::new(),
    })
}

fn response(rest: &str, body: &str) -> Result<Response, String> {
    let (line, stamp) = match rest.split_once(" recorded ") {
        Some((line, stamp)) => {
            let fields: Vec<&str> = stamp.split_whitespace().collect();
            let (date, endpoint, live_values) = match fields[..] {
                [date, endpoint] => (date, endpoint, false),
                [date, endpoint, "live:", "values"] => (date, endpoint, true),
                _ => {
                    return Err(format!(
                        "recorded needs a date and an endpoint, then \
                         optionally `live: values`: {stamp}"
                    ));
                }
            };
            let stamp = Stamp {
                date: date.to_owned(),
                endpoint: endpoint.to_owned(),
                live_values,
            };
            (line, Some(stamp))
        }
        None => (rest, None),
    };
    if stamp.is_none() && line.split_whitespace().any(|word| word == "live:") {
        return Err(format!("live: needs a recorded response: {line}"));
    }
    let (line, headers) = split_headers(line)?;
    let fault = match line {
        "network error" => Some(Fault::Network),
        "redirect" => Some(Fault::Redirect),
        "html challenge" => Some(Fault::HtmlChallenge),
        _ => None,
    };
    let served = if let Some(fault) = fault {
        if stamp.is_some() {
            return Err(format!("a fault is not recorded: {line}"));
        }
        if headers != Headers::default() {
            return Err(format!("a fault has no headers: {rest}"));
        }
        Served::Fault(fault)
    } else {
        let (status, content_type) =
            line.split_once(' ').unwrap_or((line, ""));
        let status = status
            .parse::<u16>()
            .map_err(|_| format!("bad response line: {rest}"))?;
        let content_type = content_type.trim();
        if stamp.is_some() && headers != Headers::default() {
            return Err(format!("headers are not recorded: {rest}"));
        }
        Served::Reply {
            status,
            content_type: (!content_type.is_empty())
                .then(|| content_type.to_owned()),
            headers,
        }
    };
    Ok(Response { served, stamp, body: body.to_owned() })
}

/// The status and content type at the start of a response line, and the
/// `name: value` headers after them.
fn split_headers(line: &str) -> Result<(&str, Headers), String> {
    let is_header = |word: &str| {
        matches!(word, "location:" | "retry-after:" | "cf-mitigated:")
    };
    let mut offset = 0;
    let mut start = None;
    for word in line.split(' ') {
        if is_header(word) {
            start = Some(offset);
            break;
        }
        offset += word.len() + 1;
    }
    let Some(start) = start else {
        return Ok((line.trim(), Headers::default()));
    };
    let mut headers = Headers::default();
    let mut words = line[start..].split_whitespace();
    while let Some(name) = words.next() {
        let slot = match name {
            "location:" => &mut headers.location,
            "retry-after:" => &mut headers.retry_after,
            "cf-mitigated:" => &mut headers.cf_mitigated,
            _ => return Err(format!("unexpected after the headers: {name}")),
        };
        let value = words
            .next()
            .filter(|value| !is_header(value))
            .ok_or_else(|| format!("{name} needs a value: {line}"))?;
        if slot.replace(value.to_owned()).is_some() {
            return Err(format!("two {name} headers: {line}"));
        }
    }
    Ok((line[..start].trim(), headers))
}
