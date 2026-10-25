use super::{Error, report};

fn rendered(error: &Error) -> String {
    let mut out = Vec::new();
    assert!(report(error, &mut out).is_ok());
    String::from_utf8_lossy(&out).into_owned()
}

#[test]
fn a_usage_error_exits_2_and_every_other_error_exits_1() {
    assert_eq!(Error::usage("x", "sego show").exit_code(), 2);
    assert_eq!(Error::missing("x").exit_code(), 1);
    assert_eq!(Error::upstream("x").exit_code(), 1);
    assert_eq!(Error::edge("x").exit_code(), 1);
    assert_eq!(Error::network("x").exit_code(), 1);
    assert_eq!(Error::deadline("sourcegraph.com").exit_code(), 1);
}

#[test]
fn an_error_keeps_two_facts_and_drops_the_rest() {
    let error = Error::missing("gone").fact("one").fact("two").fact("three");
    assert_eq!(rendered(&error), "sego: error: gone\n  one\n  two\n");
}
