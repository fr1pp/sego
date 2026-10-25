use super::string_literals;

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
