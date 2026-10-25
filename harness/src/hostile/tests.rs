use std::path::Path;

use serde_json::{Value, json};

use super::{
    check, content_corpus, corpus, corpus_starts_a_line, inject,
    partial_corpus, unsafe_character,
};
use crate::case::Case;
use crate::run::run_case;

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
        assert_eq!(
            injected(&json!({"oid": hostile}))["oid"],
            with_corpus(hostile)
        );
    }
}

#[test]
fn a_string_that_parses_as_a_number_is_kept() {
    for kept in ["1", "-2.5", "1e3", "007"] {
        let body = json!({"n": kept});
        assert_eq!(injected(&body), body, "{kept}");
    }
    for hostile in ["inf", "nan", "e", ""] {
        assert_eq!(
            injected(&json!({"n": hostile}))["n"],
            with_corpus(hostile),
            "{hostile}"
        );
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
    let body =
        json!({"path": ["data", "r0", 0], "entry": {"path": "src/x.go"}});
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
    assert_eq!(
        injected(&json!({"content": "a\n"}))["content"],
        format!("a{line}\n")
    );
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
    for text in
        ["a\u{1b}[0m", "a\r\nb", "a\rb", "a\u{202e}", "a\u{200b}", "a\u{0}"]
    {
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
    assert!(partial_corpus(&json!({"a": [whole]})).is_none());
    assert!(
        partial_corpus(&json!({"content": format!("a{}", content_corpus())}))
            .is_none()
    );
    assert!(partial_corpus(&json!({"a": "plain"})).is_none());
    let part = "aesc\u{1b}[0m".to_owned();
    assert_eq!(partial_corpus(&json!({"a": {"b": [&part]}})), Some(part));
    let corpus = corpus();
    let cut = &corpus[..corpus.len() - 4];
    assert!(partial_corpus(&json!({"a": cut})).is_some());
}

const FAILED_LIST: &str = "args: refs example.com/a/b
exit: 1
--- request Refs {\"first\":101,\"query\":null,\"repo0\":\"example.com/a/b\"}
--- query
query Refs($repo0: String!) { r0: repository(name: $repo0) { name } }
--- response RESPONSE
{\"data\": {\"r0\": null}, \"errors\": [{\"message\": \"boom\", \"path\": [\"r0\", \"tags\"]}]}
";

fn failed_list(response: &str) -> Case {
    let text = FAILED_LIST.replace("RESPONSE", response);
    Case::parse(&text, Path::new("refs/x.case"))
        .unwrap_or_else(|e| panic!("{e}"))
}

#[test]
fn a_case_served_a_response_is_run_again_and_passes() {
    let case = failed_list("200 application/json");
    let normal = run_case(&case).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(normal.exit, 1);
    assert_eq!(check(&case, &normal), Vec::<String>::new());
}

#[test]
fn an_exit_code_that_differs_from_the_normal_run_fails() {
    let case = failed_list("200 application/json");
    let mut normal = run_case(&case).unwrap_or_else(|e| panic!("{e}"));
    normal.exit = 0;
    let failures = check(&case, &normal);
    assert_eq!(failures.len(), 1, "{failures:?}");
    assert!(
        failures[0].ends_with(
            "hostile run: exit 1 with the corpus injected, 0 without"
        ),
        "{failures:?}"
    );
}

#[test]
fn a_case_served_no_response_is_not_run_again() {
    let case = failed_list("network error");
    let normal = run_case(&case).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(check(&case, &normal), Vec::<String>::new());
}
