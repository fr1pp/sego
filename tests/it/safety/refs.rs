use std::ffi::OsString;
use std::process::ExitCode;

use serde_json::{Value, json};

use super::{
    COMMIT, ESCAPED, ESCAPED_MESSAGE, REPO, Ran, assert_clean_error,
    assert_not_served, assert_safe, hostile, hostile_name, json, requested,
    sego,
};

fn refs(text: Option<&str>, json: bool) -> Ran {
    let mut args: Vec<OsString> = vec!["refs".into()];
    if json {
        args.push("--json".into());
    }
    args.push(REPO.into());
    if let Some(text) = text {
        args.push(text.into());
    }
    sego(args)
}

const REF_QUERIES: [Option<&str>; 6] = [
    None,
    Some("HOSTILE"),
    Some("broken"),
    Some("nobranches"),
    Some("absent"),
    Some("refused"),
];

fn refs_query(query: Option<&str>) -> Option<String> {
    query.map(requested)
}

/// Header, `branch` rows, then `tag` rows. `ESCAPED` stands for each hostile
/// name; the hostile commit and date are written out.
fn refs_text(counts: &str) -> String {
    let short = "esc\\u{1b}[0mrtl\\u{202e}x";
    let date = "esc\\u{1b}[0mrtl";
    format!(
        "{REPO}{ESCAPED}  {counts}\n\
         branch  0123456789ab  2026-03-01  {ESCAPED}  (default)\n\
         branch  {short}  {date}  b-{ESCAPED}\n\
         tag     0123456789ab  {date}  t-{ESCAPED}\n\
         tag     0123456789ab  2026-02-01  {ESCAPED}\n\
         tag     -             -           u-{ESCAPED}\n"
    )
}

/// Every stdout line of `refs` is the header or a `branch` or `tag` row, in
/// that order.
fn assert_refs_layout(out: &str, branches: usize, tags: usize) {
    assert!(out.ends_with('\n'), "{out:?}");
    let lines: Vec<&str> =
        out.strip_suffix('\n').unwrap_or(out).split('\n').collect();
    assert!(lines[0].starts_with(REPO), "{:?}", lines[0]);
    assert_eq!(lines.len(), 1 + branches + tags, "{out:?}");
    for (i, line) in lines.iter().enumerate().skip(1) {
        let kind = if i <= branches { "branch  " } else { "tag     " };
        let rest = line.strip_prefix(kind);
        assert!(
            rest.is_some_and(|rest| !rest.is_empty() && !rest.starts_with(' ')),
            "line {i}: {line:?}"
        );
    }
}

#[test]
fn no_unsafe_character_reaches_any_refs_output() {
    for query in REF_QUERIES {
        for json in [false, true] {
            let ran = refs(refs_query(query).as_deref(), json);
            let what = format!("refs {query:?} json={json}");
            assert_safe(&format!("stdout of {what}"), &ran.out);
            assert_safe(&format!("stderr of {what}"), &ran.err);
        }
    }
}

#[test]
fn refs_text_has_only_the_layout_lines() {
    let ran = refs(None, false);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", ran.err);
    assert!(ran.err.is_empty(), "{:?}", ran.err);
    assert_refs_layout(&ran.out, 2, 3);
    assert_eq!(ran.out, refs_text("2 branches  3 tags"));

    let ran = refs(Some(&hostile()), false);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", ran.err);
    assert!(ran.err.is_empty(), "{:?}", ran.err);
    assert_refs_layout(&ran.out, 2, 3);
    assert_eq!(
        ran.out,
        refs_text(&format!(
            "2 of 9 branches match \"{ESCAPED}\"  \
             3 of 12 tags match \"{ESCAPED}\""
        ))
    );
}

#[test]
fn refs_json_values_equal_the_hostile_originals() {
    for query in [None, Some(hostile())] {
        let ran = refs(query.as_deref(), true);
        assert_eq!(ran.code, ExitCode::SUCCESS, "{}", ran.err);
        assert!(ran.err.is_empty(), "{:?}", ran.err);
        let value = json(&ran);
        let h = hostile();
        assert_eq!(value["repo"], hostile_name());
        assert_eq!(value["query"], json!(query));
        assert_eq!(value["default_branch"], h);
        let (branches_all, tags_all) = match query {
            Some(_) => (json!(9), json!(12)),
            None => (Value::Null, Value::Null),
        };
        assert_eq!(value["branches_all"], branches_all);
        assert_eq!(value["tags_all"], tags_all);
        let kinds: Vec<&Value> =
            value["refs"].as_array().expect("refs").iter().collect();
        let field = |name: &str| -> Vec<Value> {
            kinds.iter().map(|r| r[name].clone()).collect()
        };
        assert_eq!(
            field("kind"),
            ["branch", "branch", "tag", "tag", "tag"].map(Value::from)
        );
        assert_eq!(
            field("name"),
            [
                h.clone(),
                format!("b-{h}"),
                format!("t-{h}"),
                h.clone(),
                format!("u-{h}")
            ]
            .map(Value::from)
        );
        assert_eq!(
            field("commit"),
            [
                json!(COMMIT),
                json!(h),
                json!(COMMIT),
                json!(COMMIT),
                Value::Null
            ]
        );
        let date: String = h.chars().take(10).collect();
        assert_eq!(
            field("date"),
            [
                json!("2026-03-01"),
                json!(date),
                json!(date),
                json!("2026-02-01"),
                Value::Null
            ]
        );
    }
}

#[test]
fn refs_error_paths_fail_with_a_clean_message() {
    let rejected = format!(
        "sego: error: Sourcegraph rejected sego's request: \
         {ESCAPED_MESSAGE}\n"
    );
    let cases = [
        ("broken", "sego: error: Sourcegraph failed to answer the tag list\n"),
        (
            "nobranches",
            "sego: error: Sourcegraph failed to answer the branch list\n",
        ),
        ("refused", rejected.as_str()),
    ];
    for (query, message) in cases {
        for as_json in [false, true] {
            let ran = refs(Some(query), as_json);
            let what = format!("refs {query} json={as_json}");
            assert_clean_error(&what, &ran, 1);
            assert_eq!(ran.err, message, "{what}");
        }
    }
    for as_json in [false, true] {
        assert_not_served("refs absent", &refs(Some("absent"), as_json));
    }
}
