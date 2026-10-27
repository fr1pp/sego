use std::collections::HashSet;

use serde_json::Value;

use super::case::{Body, Case, Exchange, Request};
use super::replay::Replay;
use super::{Outcome, run};

// The escape table is the oracle: it defines which characters are unsafe
// (ADR 0006), so the check reads sego's own table rather than a copy.
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
const KEPT: [&str; 6] = ["__typename", "title", "rawURL", "url", "kind", "value"];

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

/// Whether sego never prints `c` as itself.
pub fn is_unsafe(c: char) -> bool {
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
    let line = text
        .split('\n')
        .find(|line| line.starts_with(ESC_ESCAPED) || line.starts_with(AFTER_NEWLINE))?;
    Some(format!("{stream} has a line that starts with server text: {line}"))
}

/// A string of the JSON answer that carries part of the corpus and not all
/// of it.
pub fn corpus_fragment(answer: &Value) -> Option<String> {
    match answer {
        Value::String(s) => {
            let marked = MARKED.iter().any(|piece| s.contains(piece));
            let whole = s.contains(&corpus()) || s.contains(&content_corpus());
            (marked && !whole).then(|| s.clone())
        }
        Value::Array(items) => items.iter().find_map(corpus_fragment),
        Value::Object(map) => map.values().find_map(corpus_fragment),
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
            let Some(Request::Graphql { variables, .. }) = normal.sent.get(at).map(|s| &s.request)
            else {
                return exchange;
            };
            let Body::Text(text) = &exchange.body else { return exchange };
            let Ok(body) = serde_json::from_str(text) else { return exchange };
            exchange.body = Body::Text(inject(&body, variables).to_string());
            injected = true;
            exchange
        })
        .collect();
    if !injected {
        return Vec::new();
    }
    let hostile = match run(case, &Replay::serving(case, exchanges)) {
        Ok(hostile) => hostile,
        Err(e) => return vec![format!("{}: hostile run: {e}", case.path.display())],
    };
    let mut failures = Vec::new();
    if hostile.code != normal.code {
        failures.push(format!(
            "exit {} with the corpus injected, {} without",
            hostile.code, normal.code
        ));
    }
    for (stream, text) in [("stdout", &hostile.stdout), ("stderr", &hostile.stderr)] {
        failures.extend(unsafe_character(stream, text));
        failures.extend(corpus_starts_a_line(stream, text));
    }
    if case.args.iter().any(|arg| arg == "--json") && !hostile.stdout.is_empty() {
        match serde_json::from_str::<Value>(&hostile.stdout) {
            Ok(answer) => failures.extend(
                corpus_fragment(&answer)
                    .map(|s| format!("a JSON string holds part of the corpus: {s:?}")),
            ),
            Err(e) => failures.push(format!("stdout is not JSON: {e}")),
        }
    }
    failures
        .into_iter()
        .map(|failure| format!("{}: hostile run: {failure}", case.path.display()))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use serde_json::json;

    use super::*;

    fn injected(body: &Value) -> Value {
        inject(body, &Value::Null)
    }

    fn with_corpus(s: &str) -> String {
        format!("{s}{}", corpus())
    }

    #[test]
    fn a_string_value_gets_the_corpus_and_a_key_does_not() {
        let body = injected(&json!({"name": "mux", "nodes": ["a", {"b": "c"}]}));
        assert_eq!(
            body,
            json!({
                "name": with_corpus("mux"),
                "nodes": [with_corpus("a"), {"b": with_corpus("c")}],
            })
        );
    }

    #[test]
    fn numbers_booleans_and_null_are_left_alone() {
        let body = json!({"count": 3, "ok": true, "none": null});
        assert_eq!(injected(&body), body);
    }

    #[test]
    fn the_keys_the_answer_is_built_from_are_kept() {
        let body = json!({
            "__typename": "GitCommit",
            "title": "Rate limited",
            "rawURL": "/raw/x",
            "url": "/search?q=x",
            "kind": "file",
            "value": "archived:yes",
        });
        assert_eq!(injected(&body), body);
    }

    #[test]
    fn a_hex_string_of_7_to_40_characters_is_kept() {
        for kept in ["abcdef0", &"a".repeat(40), "98cb6bf42e08"] {
            let body = json!({"oid": kept});
            assert_eq!(injected(&body), body, "{kept}");
        }
        for hostile in ["abcdef", &"a".repeat(41), "abcdefg"] {
            assert_eq!(injected(&json!({"oid": hostile}))["oid"], with_corpus(hostile));
        }
    }

    #[test]
    fn a_string_that_parses_as_a_number_is_kept() {
        for kept in ["1", "-2.5", "1e3", "007"] {
            let body = json!({"n": kept});
            assert_eq!(injected(&body), body, "{kept}");
        }
        for hostile in ["inf", "nan", "e", ""] {
            assert_eq!(injected(&json!({"n": hostile}))["n"], with_corpus(hostile), "{hostile}");
        }
    }

    #[test]
    fn a_string_equal_to_one_in_the_variables_is_kept() {
        let variables = json!({"repo0": "example.com/a/b", "nested": ["main"]});
        let body = json!({"name": "example.com/a/b", "ref": "main", "x": "main2"});
        let body = inject(&body, &variables);
        assert_eq!(body["name"], "example.com/a/b");
        assert_eq!(body["ref"], "main");
        assert_eq!(body["x"], with_corpus("main2"));
    }

    #[test]
    fn every_element_of_a_path_array_is_kept_and_a_path_string_is_not() {
        let body = json!({"path": ["data", "r0", 0], "entry": {"path": "src/x.go"}});
        let body = injected(&body);
        assert_eq!(body["path"], json!(["data", "r0", 0]));
        assert_eq!(body["entry"]["path"], with_corpus("src/x.go"));
    }

    #[test]
    fn content_gets_the_corpus_on_every_line_before_any_carriage_return() {
        let line = content_corpus();
        let body = injected(&json!({"content": "a\r\nb\nc"}));
        assert_eq!(body["content"], format!("a{line}\r\nb{line}\nc{line}"));
        assert!(!line.contains('\0') && !line.contains('\n'));
    }

    #[test]
    fn empty_content_stays_empty_and_a_final_newline_adds_no_line() {
        let line = content_corpus();
        assert_eq!(injected(&json!({"content": ""}))["content"], "");
        assert_eq!(injected(&json!({"content": "a\n"}))["content"], format!("a{line}\n"));
    }

    #[test]
    fn a_message_gets_the_corpus_on_every_line_but_its_fences() {
        let line = content_corpus();
        let body = injected(&json!({"text": "```diff\r\nx\r\n+y\n```"}));
        assert_eq!(body["text"], format!("```diff\r\nx{line}\r\n+y{line}\n```"));
    }

    #[test]
    fn an_unsafe_character_other_than_a_newline_is_found() {
        assert!(unsafe_character("stdout", "a\nb\n").is_none());
        for text in ["a\u{1b}[0m", "a\r\nb", "a\rb", "a\u{202e}", "a\u{200b}", "a\u{0}"] {
            assert!(unsafe_character("stdout", text).is_some(), "{text:?}");
        }
    }

    #[test]
    fn a_line_starting_with_the_corpus_is_found() {
        let head = "esc\\u{1b}[0mrtl\\u{202e}xzw\\u{200b}x";
        let text = format!("sego: note: a\n    1 | body{head}\n{head}\n");
        let found = corpus_starts_a_line("stdout", &text);
        assert!(found.is_some_and(|f| f.ends_with(head)));
        let gutter = format!("    1 | {head}\nsego: note: {head}\nbody{head}\n");
        assert!(corpus_starts_a_line("stdout", &gutter).is_none());
    }

    #[test]
    fn the_corpus_after_a_newline_at_the_start_of_a_line_is_found() {
        let text = "    1 | newline\nlinecr\\u{d}nul\\u{0}x\n";
        assert!(corpus_starts_a_line("stderr", text).is_some());
    }

    #[test]
    fn a_json_string_with_part_of_the_corpus_is_found() {
        let whole = with_corpus("a");
        assert!(corpus_fragment(&json!({"a": [whole]})).is_none());
        assert!(corpus_fragment(&json!({"content": format!("a{}", content_corpus())})).is_none());
        assert!(corpus_fragment(&json!({"a": "plain"})).is_none());
        let part = "aesc\u{1b}[0m".to_owned();
        assert_eq!(corpus_fragment(&json!({"a": {"b": [&part]}})), Some(part));
        let corpus = corpus();
        let cut = &corpus[..corpus.len() - 4];
        assert!(corpus_fragment(&json!({"a": cut})).is_some());
    }

    const FAILED_LIST: &str = r#"
    args = ["refs", "example.com/a/b"]
    status.code = 1

    [[exchange]]
    operation = "Refs"
    variables = '{"first":101,"query":null,"repo0":"example.com/a/b"}'
    query = "query Refs($repo0: String!) { r0: repository(name: $repo0) { name } }"
    status = 200
    content-type = "application/json"
    response = '{"data": {"r0": null}, "errors": [{"message": "boom", "path": ["r0", "tags"]}]}'
    "#;

    fn failed_list() -> Case {
        Case::parse(FAILED_LIST, Path::new("refs/x.toml")).unwrap_or_else(|e| panic!("{e}"))
    }

    #[test]
    fn a_case_served_a_response_is_run_again_and_passes() {
        let case = failed_list();
        let normal = run(&case, &Replay::new(&case)).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(normal.code, 1);
        assert_eq!(check(&case, &normal), Vec::<String>::new());
    }

    #[test]
    fn an_exit_code_that_differs_from_the_normal_run_fails() {
        let case = failed_list();
        let mut normal = run(&case, &Replay::new(&case)).unwrap_or_else(|e| panic!("{e}"));
        normal.code = 0;
        let failures = check(&case, &normal);
        assert_eq!(failures.len(), 1, "{failures:?}");
        assert!(
            failures[0].ends_with("hostile run: exit 1 with the corpus injected, 0 without"),
            "{failures:?}"
        );
    }

    #[test]
    fn a_case_served_no_json_is_not_run_again() {
        let mut case = failed_list();
        case.exchanges[0].body = Body::Text("<html></html>".to_owned());
        let normal = run(&case, &Replay::new(&case)).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(check(&case, &normal), Vec::<String>::new());
    }
}
