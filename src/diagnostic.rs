use std::fmt::{Display, Write as _};

use crate::escape::sanitise;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Answered,
    AnsweredNo,
    Invalid,
    Unanswered,
}

impl Outcome {
    pub fn exit_code(self) -> u8 {
        match self {
            Outcome::Answered => 0,
            Outcome::AnsweredNo => 1,
            Outcome::Invalid => 2,
            Outcome::Unanswered => 3,
        }
    }
}

/// Anything that isn't an answer. `Display` writes the message.
pub trait Diagnostic: Display {
    fn code(&self) -> &'static str;
    fn outcome(&self) -> Outcome;
    fn causes(&self) -> Vec<String> {
        Vec::new()
    }
    fn hints(&self) -> Vec<String> {
        Vec::new()
    }
    fn detail(&self) -> Option<serde_json::Value> {
        None
    }
}

/// The stderr text: an *answered no* as a plain sentence, anything else as
/// `error:` with its `caused by:` lines; then the `hint:` lines. Message and
/// causes are sanitised so each stays on one line; hints are already shell
/// words.
pub fn report(diagnostic: &dyn Diagnostic) -> String {
    let message = sanitise(&diagnostic.to_string());
    let mut text = String::new();
    if diagnostic.outcome() == Outcome::AnsweredNo {
        let _ = writeln!(text, "{message}");
    } else {
        let _ = writeln!(text, "error: {message}");
        for cause in diagnostic.causes() {
            let _ = writeln!(text, "caused by: {}", sanitise(&cause));
        }
    }
    for hint in diagnostic.hints() {
        let _ = writeln!(text, "hint: {hint}");
    }
    text
}

#[cfg(test)]
mod tests {
    use std::fmt;

    use expect_test::expect;

    use super::*;

    struct Sample {
        outcome: Outcome,
        message: String,
        causes: Vec<String>,
        hints: Vec<String>,
    }

    impl Sample {
        fn no() -> Self {
            Sample {
                outcome: Outcome::AnsweredNo,
                message: "repository github.com/gorilla/nope not found".into(),
                causes: Vec::new(),
                hints: vec!["sego search 'type:repo nope'".into()],
            }
        }

        fn failure() -> Self {
            Sample {
                outcome: Outcome::Unanswered,
                message: "the request to \
                          https://sourcegraph.com/.api/graphql failed"
                    .into(),
                causes: vec!["connection refused".into()],
                hints: vec!["sego read github.com/x/y mux.go".into()],
            }
        }
    }

    impl fmt::Display for Sample {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str(&self.message)
        }
    }

    impl Diagnostic for Sample {
        fn code(&self) -> &'static str {
            "sample"
        }

        fn outcome(&self) -> Outcome {
            self.outcome
        }

        fn causes(&self) -> Vec<String> {
            self.causes.clone()
        }

        fn hints(&self) -> Vec<String> {
            self.hints.clone()
        }
    }

    #[test]
    fn exit_codes() {
        let codes = [
            Outcome::Answered,
            Outcome::AnsweredNo,
            Outcome::Invalid,
            Outcome::Unanswered,
        ]
        .map(Outcome::exit_code);
        assert_eq!(codes, [0, 1, 2, 3]);
    }

    #[test]
    fn answered_no_is_a_plain_sentence() {
        expect![[r#"
            repository github.com/gorilla/nope not found
            hint: sego search 'type:repo nope'
        "#]]
        .assert_eq(&report(&Sample::no()));
    }

    #[test]
    fn errors_carry_causes_and_hints() {
        expect![[r#"
            error: the request to https://sourcegraph.com/.api/graphql failed
            caused by: connection refused
            hint: sego read github.com/x/y mux.go
        "#]]
        .assert_eq(&report(&Sample::failure()));
    }

    #[test]
    fn messages_and_causes_stay_on_one_line() {
        let sample = Sample {
            outcome: Outcome::Invalid,
            message: "bad\nthing\u{202e}".into(),
            causes: vec!["first\r\nsecond".into()],
            hints: Vec::new(),
        };
        expect![[r#"
            error: bad\x0athing\u{202e}
            caused by: first\x0d\x0asecond
        "#]]
        .assert_eq(&report(&sample));
    }

    #[test]
    fn the_defaults_add_nothing() {
        struct Bare;

        impl fmt::Display for Bare {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("bare")
            }
        }

        impl Diagnostic for Bare {
            fn code(&self) -> &'static str {
                "bare"
            }

            fn outcome(&self) -> Outcome {
                Outcome::Unanswered
            }
        }

        assert_eq!((Bare.code(), Bare.detail()), ("bare", None));
        assert_eq!(report(&Bare), "error: bare\n");
    }
}
