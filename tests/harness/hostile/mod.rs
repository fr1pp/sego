use std::collections::HashSet;

use serde_json::Value;

use crate::case::{Case, Exchange, Request, Served};
use crate::replay::Replay;
use crate::run::{self, Outcome};

module_tests!();

#[path = "../../../src/escape/table.rs"]
mod table;

const ESC: &str = "esc\u{1b}[0m";
const ESC_ESCAPED: &str = "esc\\u{1b}[0m";
const RTL: &str = "rtl\u{202e}x";
const ZERO_WIDTH: &str = "zw\u{200b}x";
const NEWLINE: &str = "new\nline";
const CR: &str = "cr\r";
const NUL: &str = "nul\u{0}x";
const SEPARATOR: &str = "ls\u{2028}x";
const DEL: &str = "del\u{7f}x";

/// The items that carry a character no answer may show as itself.
const MARKED: [&str; 6] = [ESC, RTL, ZERO_WIDTH, NUL, SEPARATOR, DEL];

/// What follows the newline of the corpus.
const AFTER_NEWLINE: &str = "linecr";

/// Appended to the strings of a response.
fn corpus() -> String {
    [ESC, RTL, ZERO_WIDTH, NEWLINE, CR, NUL, SEPARATOR, DEL].concat()
}

/// Appended to every line of file content, which has no NUL and no newline.
fn content_corpus() -> String {
    [ESC, RTL, ZERO_WIDTH, CR, SEPARATOR, DEL].concat()
}

/// Keys whose string values stay as they are.
const KEPT: [&str; 6] =
    ["__typename", "title", "rawURL", "url", "kind", "value"];

/// `body` with the corpus appended to its string values, except those the
/// answer is built from: keys, `KEPT`, hex strings of 7 to 40 characters,
/// numbers, strings in the request's `variables`, and GraphQL error paths.
pub fn inject(body: &Value, variables: &Value) -> Value {
    let mut asked = HashSet::new();
    strings(variables, &mut asked);
    let mut body = body.clone();
    walk(&mut body, None, &asked);
    body
}

fn strings(value: &Value, into: &mut HashSet<String>) {
    match value {
        Value::String(s) => {
            into.insert(s.clone());
        }
        Value::Array(items) => {
            for item in items {
                strings(item, into);
            }
        }
        Value::Object(map) => {
            for item in map.values() {
                strings(item, into);
            }
        }
        _ => {}
    }
}

fn walk(value: &mut Value, key: Option<&str>, asked: &HashSet<String>) {
    match value {
        Value::Object(map) => {
            for (key, item) in map.iter_mut() {
                walk(item, Some(key), asked);
            }
        }
        Value::Array(items) => {
            if key != Some("path") {
                for item in items {
                    walk(item, None, asked);
                }
            }
        }
        Value::String(s) => {
            if key.is_some_and(|key| KEPT.contains(&key)) {
                return;
            }
            if key == Some("content") {
                *s = in_content(s);
            } else if key == Some("text") {
                *s = in_message(s);
            } else if !left_alone(s, asked) {
                s.push_str(&corpus());
            }
        }
        _ => {}
    }
}

fn left_alone(s: &str, asked: &HashSet<String>) -> bool {
    asked.contains(s) || is_hex(s) || is_number(s)
}

fn is_hex(s: &str) -> bool {
    (7..=40).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_hexdigit())
}

fn is_number(s: &str) -> bool {
    s.bytes().any(|b| b.is_ascii_digit()) && s.parse::<f64>().is_ok()
}

/// `content` with the content corpus at the end of every line, before any
/// `\r`, including an unterminated last line.
fn in_content(content: &str) -> String {
    in_lines(content, |_| false)
}

/// A commit message or diff, fenced by lines of backticks, as `in_content`
/// except for the fences, which sego drops and tells a diff by.
fn in_message(text: &str) -> String {
    in_lines(text, |line| line.starts_with("```"))
}

fn in_lines(text: &str, kept: impl Fn(&str) -> bool) -> String {
    let corpus = content_corpus();
    let mut out = String::with_capacity(text.len() + corpus.len());
    for line in text.split_inclusive('\n') {
        let (line, ending) = match line.strip_suffix('\n') {
            Some(line) => (line, "\n"),
            None => (line, ""),
        };
        let (line, cr) = match line.strip_suffix('\r') {
            Some(line) => (line, "\r"),
            None => (line, ""),
        };
        out.push_str(line);
        if !kept(line) {
            out.push_str(&corpus);
        }
        out.push_str(cr);
        out.push_str(ending);
    }
    out
}

fn is_unsafe(c: char) -> bool {
    table::UNSAFE.iter().any(|&(first, last)| (first..=last).contains(&c))
}

/// A line of `text` with a character that sego must have escaped. A newline
/// is layout.
pub fn unsafe_character(stream: &str, text: &str) -> Option<String> {
    let line = text.split('\n').find(|line| line.chars().any(is_unsafe))?;
    Some(format!("{stream} holds an unsafe character: {line:?}"))
}

/// A line of `text` that starts with the corpus: injected text standing alone
/// at column 0, or the corpus after a newline that was not escaped.
pub fn corpus_starts_a_line(stream: &str, text: &str) -> Option<String> {
    let line = text.split('\n').find(|line| {
        line.starts_with(ESC_ESCAPED) || line.starts_with(AFTER_NEWLINE)
    })?;
    Some(format!("{stream} has a line that starts with server text: {line}"))
}

/// A string of the JSON answer that carries part of the corpus and not all
/// of it.
pub fn partial_corpus(answer: &Value) -> Option<String> {
    match answer {
        Value::String(s) => {
            let marked = MARKED.iter().any(|piece| s.contains(piece));
            let whole = s.contains(&corpus()) || s.contains(&content_corpus());
            (marked && !whole).then(|| s.clone())
        }
        Value::Array(items) => items.iter().find_map(partial_corpus),
        Value::Object(map) => map.values().find_map(partial_corpus),
        _ => None,
    }
}

/// The second run of a case: the hostile corpus in its responses, held to
/// what ADR 0006 requires. Empty when the case was served no GraphQL
/// response.
pub fn check(case: &Case, normal: &Outcome) -> Vec<String> {
    let mut injected = false;
    let exchanges: Vec<Exchange> = case
        .exchanges
        .iter()
        .enumerate()
        .map(|(at, exchange)| {
            let mut exchange = exchange.clone();
            let Some(Request::Graphql { variables, .. }) = normal.sent.get(at)
            else {
                return exchange;
            };
            if !matches!(exchange.response.served, Served::Reply { .. }) {
                return exchange;
            }
            let Ok(body) = serde_json::from_str(&exchange.response.body)
            else {
                return exchange;
            };
            exchange.response.body = inject(&body, variables).to_string();
            injected = true;
            exchange
        })
        .collect();
    if !injected {
        return Vec::new();
    }
    let hostile = match run::run_with(case, &Replay::serving(case, exchanges))
    {
        Ok(hostile) => hostile,
        Err(e) => {
            return vec![format!("{}: hostile run: {e}", case.path.display())];
        }
    };
    let mut failures = Vec::new();
    if hostile.exit != normal.exit {
        failures.push(format!(
            "exit {} with the corpus injected, {} without",
            hostile.exit, normal.exit
        ));
    }
    for (stream, text) in
        [("stdout", &hostile.stdout), ("stderr", &hostile.stderr)]
    {
        failures.extend(unsafe_character(stream, text));
        failures.extend(corpus_starts_a_line(stream, text));
    }
    if case.args.iter().any(|arg| arg == b"--json")
        && !hostile.stdout.is_empty()
    {
        match serde_json::from_str::<Value>(&hostile.stdout) {
            Ok(answer) => failures.extend(partial_corpus(&answer).map(|s| {
                format!("a JSON string holds part of the corpus: {s:?}")
            })),
            Err(e) => failures.push(format!("stdout is not JSON: {e}")),
        }
    }
    failures
        .into_iter()
        .map(|failure| {
            format!("{}: hostile run: {failure}", case.path.display())
        })
        .collect()
}
