use sego::internal::error::{Error, report};

fn rendered(error: &Error) -> String {
    let mut out = Vec::new();
    assert!(report(error, &mut out).is_ok());
    String::from_utf8_lossy(&out).into_owned()
}

#[test]
fn exit_codes() {
    assert_eq!(Error::usage("x", "sego show").exit_code(), 2);
    assert_eq!(Error::missing("x").exit_code(), 1);
    assert_eq!(Error::upstream("x").exit_code(), 1);
    assert_eq!(Error::edge("x").exit_code(), 1);
    assert_eq!(Error::network("x").exit_code(), 1);
    assert_eq!(Error::deadline("sourcegraph.com").exit_code(), 1);
}

#[test]
fn report_formats_message_facts_and_synopsis() {
    let error = Error::usage(
        "show needs an address, like REPO[@REV]:PATH",
        "sego show [--json] REPO[@REV]:PATH [LINES]",
    );
    assert_eq!(
        rendered(&error),
        "sego: error: show needs an address, like REPO[@REV]:PATH\n\
         usage: sego show [--json] REPO[@REV]:PATH [LINES]\n"
    );
}

#[test]
fn multi_line_synopsis_aligns_under_the_first_command() {
    let error = Error::usage("bad", "sego show A\nsego show B");
    assert_eq!(
        rendered(&error),
        "sego: error: bad\nusage: sego show A\n       sego show B\n"
    );
}

#[test]
fn facts_are_indented_and_capped_at_two() {
    let error = Error::missing("gone").fact("one").fact("two").fact("three");
    assert_eq!(rendered(&error), "sego: error: gone\n  one\n  two\n");
}

#[test]
fn server_text_stays_on_one_line() {
    let error = Error::upstream("bad\nthing\u{202e}").fact("a\r\nb");
    assert_eq!(rendered(&error), "sego: error: bad thing\\u{202e}\n  a b\n");
}

#[test]
fn deadline_message() {
    assert_eq!(
        Error::deadline("sourcegraph.com").to_string(),
        "no answer from sourcegraph.com within 90 s"
    );
}

#[test]
fn failed_names_the_part_the_server_did_not_answer() {
    let error = Error::failed("the file content");
    assert_eq!(error.kind(), sego::internal::error::Kind::Upstream);
    assert_eq!(
        rendered(&error),
        "sego: error: Sourcegraph failed to answer the file content\n"
    );
}
