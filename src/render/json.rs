use std::io::{self, Write};

use serde::Serialize;
use serde_json::ser::{Formatter, Serializer};

use crate::diagnostic::{Diagnostic, Outcome};
use crate::escape::is_unsafe;

/// The schema version, independent of sego's version.
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Serialize)]
struct Envelope<'a, B> {
    version: u32,
    kind: &'a str,
    #[serde(flatten)]
    body: &'a B,
}

#[derive(Serialize)]
struct NonAnswer {
    code: &'static str,
    message: String,
    hints: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<serde_json::Value>,
}

/// The document for `body`, which must serialise as an object: `version`,
/// `kind`, then `body`'s fields, then a newline.
#[expect(
    clippy::expect_used,
    reason = "an object with string keys always serialises, as UTF-8"
)]
pub fn document(kind: &str, body: &impl Serialize) -> String {
    let envelope = Envelope { version: SCHEMA_VERSION, kind, body };
    let mut bytes = Vec::new();
    envelope
        .serialize(&mut Serializer::with_formatter(&mut bytes, Escaping))
        .expect("the body is an object");
    bytes.push(b'\n');
    String::from_utf8(bytes).expect("serde_json writes UTF-8")
}

/// What a partial answer leaves out, and the commands that continue or
/// narrow it.
#[derive(Serialize)]
pub struct Partial {
    pub reason: String,
    pub hints: Vec<String>,
}

/// The `unavailable` document for an *answered no*, `error` for the rest.
pub fn diagnostic(d: &dyn Diagnostic) -> String {
    let kind = match d.outcome() {
        Outcome::AnsweredNo => "unavailable",
        Outcome::Answered | Outcome::Invalid | Outcome::Unanswered => "error",
    };
    let mut message = d.to_string();
    for cause in d.causes() {
        message.push_str(": ");
        message.push_str(&cause);
    }
    let body = NonAnswer {
        code: d.code(),
        message,
        hints: d.hints(),
        detail: d.detail(),
    };
    document(kind, &body)
}

/// Compact output that also writes every unsafe character as `\u` escapes
/// of its UTF-16 units. `serde_json` has already escaped `"`, `\` and the C0
/// controls before a fragment gets here.
struct Escaping;

impl Formatter for Escaping {
    fn write_string_fragment<W: ?Sized + Write>(
        &mut self,
        writer: &mut W,
        fragment: &str,
    ) -> io::Result<()> {
        let mut start = 0;
        for (i, c) in fragment.char_indices() {
            if is_unsafe(c) {
                writer.write_all(&fragment.as_bytes()[start..i])?;
                for unit in c.encode_utf16(&mut [0; 2]) {
                    write!(writer, "\\u{unit:04x}")?;
                }
                start = i + c.len_utf8();
            }
        }
        writer.write_all(&fragment.as_bytes()[start..])
    }
}

#[cfg(test)]
mod tests {
    use std::fmt;

    use expect_test::expect;
    use serde_json::json;

    use super::*;
    use crate::diagnostic::Outcome;

    struct Sample {
        outcome: Outcome,
        message: &'static str,
        causes: Vec<String>,
        hints: Vec<String>,
        detail: Option<serde_json::Value>,
    }

    impl Sample {
        fn no() -> Self {
            Sample {
                outcome: Outcome::AnsweredNo,
                message: "repository github.com/gorilla/nope not found",
                causes: Vec::new(),
                hints: vec!["sego search 'type:repo nope'".into()],
                detail: None,
            }
        }

        fn failure() -> Self {
            Sample {
                outcome: Outcome::Unanswered,
                message: "the request to https://sourcegraph.com/.api/graphql \
                          failed",
                causes: vec!["connection refused".into()],
                hints: Vec::new(),
                detail: Some(json!({"status": 502})),
            }
        }
    }

    impl fmt::Display for Sample {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str(self.message)
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

        fn detail(&self) -> Option<serde_json::Value> {
            self.detail.clone()
        }
    }

    #[derive(Serialize)]
    struct Ordered {
        zeta: u32,
        alpha: &'static str,
    }

    #[test]
    fn documents_have_the_envelope_and_a_newline() {
        expect![[r#"
            {"version":1,"kind":"sample","text":"汉语 🚀"}
        "#]]
        .assert_eq(&document("sample", &json!({"text": "汉语 🚀"})));
    }

    #[test]
    fn body_fields_keep_their_order() {
        expect![[r#"
            {"version":1,"kind":"sample","zeta":1,"alpha":"a"}
        "#]]
        .assert_eq(&document("sample", &Ordered { zeta: 1, alpha: "a" }));
    }

    #[test]
    fn only_unsafe_characters_are_escaped() {
        assert_eq!(
            document("s", &json!({"t": "a\u{202e}b\u{200b}\u{1b}"})),
            "{\"version\":1,\"kind\":\"s\",\"t\":\"a\\u202eb\\u200b\\u001b\"}\n"
        );
    }

    #[test]
    fn other_unsafe_characters_are_escaped_too() {
        assert_eq!(
            document(
                "s",
                &json!({"t": "\u{7f}\u{85}\u{2028}\u{ad}\t\u{200d}é"})
            ),
            "{\"version\":1,\"kind\":\"s\",\
             \"t\":\"\\u007f\\u0085\\u2028\\u00ad\\t\u{200d}é\"}\n"
        );
    }

    #[test]
    fn astral_unsafe_characters_use_surrogate_pairs() {
        assert_eq!(
            document("s", &json!({"t": "x\u{e0001}y"})),
            "{\"version\":1,\"kind\":\"s\",\"t\":\"x\\udb40\\udc01y\"}\n"
        );
    }

    #[test]
    fn diagnostics_become_unavailable_or_error_documents() {
        expect![[r#"
            {"version":1,"kind":"unavailable","code":"sample","message":"repository github.com/gorilla/nope not found","hints":["sego search 'type:repo nope'"]}
        "#]].assert_eq(&diagnostic(&Sample::no()));
        expect![[r#"
            {"version":1,"kind":"error","code":"sample","message":"the request to https://sourcegraph.com/.api/graphql failed: connection refused","hints":[],"detail":{"status":502}}
        "#]].assert_eq(&diagnostic(&Sample::failure()));
    }

    #[test]
    fn diagnostic_messages_are_escaped_like_any_text() {
        let sample = Sample {
            outcome: Outcome::Invalid,
            message: "bad\u{202e}",
            causes: vec!["first".into(), "second\n".into()],
            hints: Vec::new(),
            detail: None,
        };
        assert_eq!(
            diagnostic(&sample),
            "{\"version\":1,\"kind\":\"error\",\"code\":\"sample\",\
             \"message\":\"bad\\u202e: first: second\\n\",\"hints\":[]}\n"
        );
    }
}
