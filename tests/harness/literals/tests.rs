use std::fs;
use std::path::Path;

use super::{rust_files, string_literals};

/// Every note, error and fact sego writes comes from string literals in
/// src/, so no literal ends with a period, before any trailing newlines. The
/// exceptions are code tokens and the cache tag's file text.
#[test]
fn no_text_in_the_source_ends_with_a_period() {
    const CODE: &[&str] =
        &[".", "..", "+-.", "_-.", "www.", "has.", "contains."];
    let mut files = Vec::new();
    rust_files(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut files);
    assert!(files.len() > 10, "{files:?}");
    let mut found = Vec::new();
    for file in &files {
        let source = fs::read_to_string(file)
            .unwrap_or_else(|e| panic!("{}: {e}", file.display()));
        for literal in string_literals(&source) {
            let text = literal.trim_end_matches('\n');
            if text.ends_with('.')
                && !text.ends_with("...")
                && !CODE.contains(&text)
                && !text.starts_with("Signature: ")
            {
                found.push(format!("{}: {literal:?}", file.display()));
            }
        }
    }
    assert!(found.is_empty(), "{}", found.join("\n"));
}

#[test]
fn string_literals_are_found_in_source() {
    let source = r##"
        // "a comment."
        /* "a block comment." */
        let c = '"'; let e = '\''; let u = '\u{22}'; fn f<'a>(x: &'a str) {}
        let s = "one \"two\". \
                 three.";
        let r = r#"raw "quoted"."#;
        let b = br"bytes.";
        let n = b"\n";
    "##;
    assert_eq!(
        string_literals(source),
        ["one \"two\". three.", "raw \"quoted\".", "bytes.", "\n"]
    );
}
