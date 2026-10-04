use serde::Deserialize;
use serde::de::DeserializeOwned;

use super::{FetchError, Reply, first_line};
use crate::escape::sanitise;

/// The most of the body's first line a failure quotes.
const EXCERPT_BYTES: usize = 200;

/// An entry in a GraphQL response's `errors`. `message` is the upstream's
/// text, untrusted.
#[derive(Debug, Deserialize)]
pub struct GraphQlError {
    pub message: String,
    /// Field names and list indices, as strings and integers.
    pub path: Option<Vec<serde_json::Value>>,
}

/// A GraphQL response from a 200 reply. `data` that is `null` or `{}` is
/// `None`, and at least one of `data` and `errors` is present.
#[derive(Debug)]
pub struct Envelope {
    pub data: Option<serde_json::Value>,
    pub errors: Vec<GraphQlError>,
    pub trace: Option<String>,
}

#[derive(Deserialize)]
struct Wire {
    data: Option<serde_json::Value>,
    errors: Option<Vec<GraphQlError>>,
}

/// Classifies a reply: any status but 200 is `Status`; a blank body or one
/// with neither data nor errors is `Empty`; a body that isn't JSON is
/// `NotJson`; JSON that isn't a GraphQL response is `Field`.
pub fn decode(reply: Reply) -> Result<Envelope, FetchError> {
    let Reply { status, body, trace, location } = reply;
    if status != 200 {
        let excerpt = excerpt(&body);
        return Err(FetchError::Status { status, excerpt, location, trace });
    }
    if body.trim_ascii().is_empty() {
        return Err(FetchError::Empty { trace });
    }
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(&body) else {
        let excerpt = excerpt(&body);
        return Err(FetchError::NotJson { excerpt, trace });
    };
    let wire = match Wire::deserialize(value) {
        Ok(wire) => wire,
        Err(error) => {
            let cause = error.to_string();
            return Err(FetchError::Field { cause, trace });
        }
    };
    let data = wire.data.filter(|data| {
        !data.is_null() && data.as_object().is_none_or(|data| !data.is_empty())
    });
    let errors = wire.errors.unwrap_or_default();
    if data.is_none() && errors.is_empty() {
        return Err(FetchError::Empty { trace });
    }
    Ok(Envelope { data, errors, trace })
}

/// At most `EXCERPT_BYTES` of the body's first line, never cut inside a
/// character, then sanitised.
fn excerpt(body: &[u8]) -> String {
    let text = String::from_utf8_lossy(body);
    let line = first_line(&text).unwrap_or_default();
    sanitise(&line[..line.floor_char_boundary(EXCERPT_BYTES)])
}

impl Envelope {
    /// Removes and returns the first error whose path equals `path`. An
    /// integer segment equals no string.
    pub fn take_error(&mut self, path: &[&str]) -> Option<GraphQlError> {
        let at = |error: &GraphQlError| {
            error.path.as_deref().is_some_and(|segments| {
                segments.len() == path.len()
                    && segments
                        .iter()
                        .zip(path)
                        .all(|(segment, name)| segment.as_str() == Some(name))
            })
        };
        let i = self.errors.iter().position(at)?;
        Some(self.errors.remove(i))
    }

    /// The data, once the caller has taken the errors it recognises.
    /// Pathless errors are `Schema`, listing any located ones after them;
    /// located errors alone are `Server`; no data is `Empty`; data that
    /// doesn't deserialise is `Field`.
    pub fn into_data<T: DeserializeOwned>(self) -> Result<T, FetchError> {
        let Envelope { data, errors, trace } = self;
        let (pathless, located): (Vec<_>, Vec<_>) = errors
            .into_iter()
            .partition(|error| error.path.as_ref().is_none_or(Vec::is_empty));
        let messages = |errors: Vec<GraphQlError>| {
            errors.into_iter().map(|error| error.message).collect()
        };
        if !pathless.is_empty() {
            let mut errors: Vec<String> = messages(pathless);
            errors.extend(messages(located));
            return Err(FetchError::Schema { errors, trace });
        }
        if !located.is_empty() {
            return Err(FetchError::Server {
                errors: messages(located),
                trace,
            });
        }
        let Some(data) = data else {
            return Err(FetchError::Empty { trace });
        };
        serde_json::from_value(data).map_err(|error| FetchError::Field {
            cause: error.to_string(),
            trace,
        })
    }
}

#[cfg(test)]
mod tests {
    use expect_test::expect;
    use serde::Deserialize;

    use super::*;
    use crate::diagnostic::{Diagnostic, report};

    fn reply(status: u16, body: &[u8]) -> Reply {
        Reply { status, body: body.to_vec(), trace: None, location: None }
    }

    fn data(body: &[u8]) -> Result<serde_json::Value, FetchError> {
        decode(reply(200, body)).and_then(Envelope::into_data)
    }

    #[test]
    fn a_status_other_than_200_is_bad_response() {
        let body = b"<html>\r\n<head><title>502 Bad Gateway</title>\r\n";
        let error = decode(reply(502, body)).unwrap_err();
        assert!(matches!(error, FetchError::Status { status: 502, .. }));
        assert_eq!(error.code(), "bad_response");
        expect![[r#"
            error: the upstream answered HTTP 502: <html>
        "#]]
        .assert_eq(&report(&error));
    }

    #[test]
    fn a_redirect_names_its_location() {
        let mut moved = reply(301, b"");
        moved.location = Some("https://sourcegraph.com/.api/graphql".into());
        let error = decode(moved).unwrap_err();
        expect![[r#"
            error: the upstream answered HTTP 301 with Location https://sourcegraph.com/.api/graphql
        "#]]
        .assert_eq(&report(&error));
    }

    #[test]
    fn excerpts_are_200_bytes_of_the_first_line_sanitised() {
        let line = "é".repeat(150);
        let body = format!("\n  {line}\nsecond line");
        let FetchError::Status { excerpt, .. } =
            decode(reply(404, body.as_bytes())).unwrap_err()
        else {
            panic!("not a status error");
        };
        assert_eq!(excerpt, "é".repeat(100));

        let FetchError::NotJson { excerpt, .. } =
            decode(reply(200, b"\x1b[31mred\xff\nmore")).unwrap_err()
        else {
            panic!("not a not-JSON error");
        };
        assert_eq!(excerpt, "\\x1b[31mred\u{fffd}");
    }

    #[test]
    fn a_truncated_body_is_not_json() {
        let error = decode(reply(200, br#"{"data":{"repo"#)).unwrap_err();
        assert!(matches!(error, FetchError::NotJson { .. }));
        assert_eq!(error.code(), "bad_response");
        expect![[r#"
            error: the upstream's answer isn't JSON: {"data":{"repo
        "#]]
        .assert_eq(&report(&error));
    }

    #[test]
    fn neither_data_nor_errors_is_empty() {
        for body in [&b"{}"[..], b"", b"{\"data\":null,\"errors\":[]}"] {
            let error = decode(reply(200, body)).unwrap_err();
            assert!(matches!(error, FetchError::Empty { .. }), "{error:?}");
            assert_eq!(error.code(), "bad_response");
        }
    }

    #[test]
    fn empty_data_counts_as_no_data() {
        let error = data(br#"{"data":{}}"#).unwrap_err();
        assert!(matches!(error, FetchError::Empty { .. }), "{error:?}");

        let mut envelope = decode(reply(
            200,
            br#"{"data":{},"errors":[{"message":"x","path":["a"]}]}"#,
        ))
        .unwrap();
        assert!(envelope.take_error(&["a"]).is_some());
        let error = envelope.into_data::<serde_json::Value>().unwrap_err();
        assert!(matches!(error, FetchError::Empty { .. }), "{error:?}");
    }

    #[test]
    fn a_body_that_is_not_an_envelope_is_a_field_error() {
        for body in [&b"[1]"[..], br#"{"errors":[{"path":["a"]}]}"#] {
            let error = decode(reply(200, body)).unwrap_err();
            assert!(matches!(error, FetchError::Field { .. }), "{error:?}");
        }
    }

    #[test]
    fn pathless_errors_are_schema_mismatch() {
        let error = data(
            br#"{"data":{},"errors":[
                {"message":"Cannot query field","locations":[]}
            ]}"#,
        )
        .unwrap_err();
        assert_eq!(error.code(), "schema_mismatch");
        expect![[r#"
            error: the upstream rejected sego's query
            caused by: Cannot query field
        "#]]
        .assert_eq(&report(&error));
    }

    #[test]
    fn schema_mismatch_lists_located_errors_after_pathless_ones() {
        let error = data(
            br#"{"data":{"repository":null},"errors":[
                {"message":"boom","path":["repository"]},
                {"message":"Cannot query field"}
            ]}"#,
        )
        .unwrap_err();
        assert_eq!(error.code(), "schema_mismatch");
        expect![[r#"
            error: the upstream rejected sego's query
            caused by: Cannot query field
            caused by: boom
        "#]]
        .assert_eq(&report(&error));
    }

    #[test]
    fn errors_at_a_path_are_server_errors() {
        let error = data(
            br#"{"data":{"repository":{"x":null}},"errors":[
                {"message":"boom","path":["repository","x"]}
            ]}"#,
        )
        .unwrap_err();
        assert_eq!(error.code(), "server_error");
        expect![[r#"
            error: the upstream answered with an error
            caused by: boom
        "#]]
        .assert_eq(&report(&error));
    }

    #[test]
    fn take_error_removes_a_recognised_error() {
        let mut envelope = decode(reply(
            200,
            br#"{"data":{"repository":{"x":null,"y":[]}},"errors":[
                {"message":"other","path":["repository","y",0]},
                {"message":"boom","path":["repository","x"]}
            ]}"#,
        ))
        .unwrap();
        assert!(envelope.take_error(&["repository"]).is_none());
        assert!(envelope.take_error(&["repository", "y", "0"]).is_none());
        let taken = envelope.take_error(&["repository", "x"]).unwrap();
        assert_eq!(taken.message, "boom");
        assert!(envelope.take_error(&["repository", "x"]).is_none());
        assert_eq!(envelope.errors.len(), 1);
        assert_eq!(envelope.errors[0].message, "other");
    }

    #[test]
    fn data_without_errors_deserialises() {
        #[derive(Debug, Deserialize)]
        struct Data {
            repository: Option<Repository>,
        }
        #[derive(Debug, Deserialize)]
        struct Repository {
            name: String,
        }
        let body = br#"{"data":{"repository":{"name":"github.com/a/b"}}}"#;
        let found: Data =
            decode(reply(200, body)).unwrap().into_data().unwrap();
        assert_eq!(found.repository.unwrap().name, "github.com/a/b");

        let body = br#"{"data":{"repository":{"title":"b"}}}"#;
        let error =
            decode(reply(200, body)).unwrap().into_data::<Data>().unwrap_err();
        expect![[r#"
            error: the upstream's answer has missing or unknown fields
            caused by: missing field `name`
        "#]]
        .assert_eq(&report(&error));
    }

    #[test]
    fn the_trace_reaches_the_failure() {
        let traced = |status, body: &[u8]| Reply {
            trace: Some("t".into()),
            ..reply(status, body)
        };
        let failures = [
            decode(traced(500, b"oops")).unwrap_err(),
            decode(traced(200, b"<html>")).unwrap_err(),
            decode(traced(200, b"{}")).unwrap_err(),
            decode(traced(200, b"[]")).unwrap_err(),
            decode(traced(200, br#"{"data":{}}"#))
                .and_then(Envelope::into_data::<serde_json::Value>)
                .unwrap_err(),
            decode(traced(200, br#"{"errors":[{"message":"m"}]}"#))
                .and_then(Envelope::into_data::<serde_json::Value>)
                .unwrap_err(),
            decode(traced(200, br#"{"errors":[{"message":"m","path":[]}]}"#))
                .and_then(Envelope::into_data::<serde_json::Value>)
                .unwrap_err(),
            decode(traced(200, br#"{"data":{"a":1}}"#))
                .and_then(Envelope::into_data::<String>)
                .unwrap_err(),
        ];
        for error in &failures {
            assert_eq!(
                error.detail(),
                Some(serde_json::json!({"trace": "t"})),
                "{error:?}"
            );
        }
    }
}
