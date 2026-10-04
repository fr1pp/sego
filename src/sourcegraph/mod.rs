pub mod blob;
pub mod envelope;
pub mod query;
pub mod search;
#[cfg(test)]
pub mod testing;
pub mod transport;
pub mod tree;
pub mod vocabulary;

use std::fmt;
use std::time::Duration;

use crate::diagnostic::{Diagnostic, Outcome};

/// The longest one request may take, from connecting to the end of the
/// body.
pub const GLOBAL_TIMEOUT: Duration = Duration::from_secs(95);

/// A GraphQL document and its variables.
pub struct Plan {
    pub query: &'static str,
    pub variables: serde_json::Value,
}

impl Plan {
    /// The request body.
    pub fn request(&self) -> serde_json::Value {
        serde_json::json!({"query": self.query, "variables": self.variables})
    }
}

/// An HTTP response, whatever its status.
#[derive(Debug)]
pub struct Reply {
    pub status: u16,
    pub body: Vec<u8>,
    /// The `x-trace` header.
    pub trace: Option<String>,
    /// The `Location` header.
    pub location: Option<String>,
}

pub type Fetch<'a> = &'a dyn Fn(&Plan) -> Result<Reply, FetchError>;

/// Why a request has no answer. `excerpt`s and `errors` are the upstream's
/// text, untrusted.
#[derive(Debug)]
pub enum FetchError {
    Transport {
        url: String,
        cause: String,
    },
    Status {
        status: u16,
        excerpt: String,
        location: Option<String>,
        trace: Option<String>,
    },
    NotJson {
        excerpt: String,
        trace: Option<String>,
    },
    Empty {
        trace: Option<String>,
    },
    Field {
        cause: String,
        trace: Option<String>,
    },
    Server {
        errors: Vec<String>,
        trace: Option<String>,
    },
    Schema {
        errors: Vec<String>,
        trace: Option<String>,
    },
}

/// The first line of `text` with anything in it, trimmed; `None` if there
/// is none.
fn first_line(text: &str) -> Option<&str> {
    text.split(['\r', '\n']).map(str::trim).find(|line| !line.is_empty())
}

impl fmt::Display for FetchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FetchError::Transport { url, .. } => {
                write!(f, "the request to {url} failed")
            }
            FetchError::Status { status, excerpt, location, .. } => {
                write!(f, "the upstream answered HTTP {status}")?;
                if (300..400).contains(status) {
                    match location {
                        Some(location) => {
                            write!(f, " with Location {location}")?;
                        }
                        None => f.write_str(" with no Location")?,
                    }
                }
                match first_line(excerpt) {
                    Some(line) => write!(f, ": {line}"),
                    None => Ok(()),
                }
            }
            FetchError::NotJson { excerpt, .. } => {
                f.write_str("the upstream's answer isn't JSON")?;
                match first_line(excerpt) {
                    Some(line) => write!(f, ": {line}"),
                    None => Ok(()),
                }
            }
            FetchError::Empty { .. } => {
                f.write_str("the upstream's answer is empty")
            }
            FetchError::Field { .. } => f.write_str(
                "the upstream's answer has missing or unknown fields",
            ),
            FetchError::Server { errors, .. } => match errors.len() {
                1 => f.write_str("the upstream answered with an error"),
                _ => f.write_str("the upstream answered with errors"),
            },
            FetchError::Schema { .. } => {
                f.write_str("the upstream rejected sego's query")
            }
        }
    }
}

impl FetchError {
    fn trace(&self) -> Option<&str> {
        match self {
            FetchError::Transport { .. } => None,
            FetchError::Status { trace, .. }
            | FetchError::NotJson { trace, .. }
            | FetchError::Empty { trace }
            | FetchError::Field { trace, .. }
            | FetchError::Server { trace, .. }
            | FetchError::Schema { trace, .. } => trace.as_deref(),
        }
    }
}

impl Diagnostic for FetchError {
    fn code(&self) -> &'static str {
        match self {
            FetchError::Transport { .. } => "transport",
            FetchError::Status { .. }
            | FetchError::NotJson { .. }
            | FetchError::Empty { .. }
            | FetchError::Field { .. } => "bad_response",
            FetchError::Server { .. } => "server_error",
            FetchError::Schema { .. } => "schema_mismatch",
        }
    }

    fn outcome(&self) -> Outcome {
        Outcome::Unanswered
    }

    fn causes(&self) -> Vec<String> {
        match self {
            FetchError::Transport { cause, .. }
            | FetchError::Field { cause, .. } => vec![cause.clone()],
            FetchError::Server { errors, .. }
            | FetchError::Schema { errors, .. } => errors.clone(),
            FetchError::Status { .. }
            | FetchError::NotJson { .. }
            | FetchError::Empty { .. } => Vec::new(),
        }
    }

    fn detail(&self) -> Option<serde_json::Value> {
        self.trace().map(|trace| serde_json::json!({"trace": trace}))
    }
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use expect_test::expect;

    use super::*;
    use crate::diagnostic::report;

    #[test]
    fn plans_become_query_and_variables() {
        let query = "query Q($r: String!) { repository(name: $r) { id } }";
        let variables = serde_json::json!({"r": "github.com/gorilla/mux"});
        let plan = Plan { query, variables: variables.clone() };
        assert_eq!(
            plan.request(),
            serde_json::json!({"query": query, "variables": variables})
        );
    }

    #[test]
    fn status_errors_name_the_status_body_and_location() {
        let moved = FetchError::Status {
            status: 301,
            excerpt: "<html>\r\n<head><title>301 Moved Permanently</title>"
                .into(),
            location: Some("https://sourcegraph.com/.api/graphql".into()),
            trace: None,
        };
        let missing = FetchError::Status {
            status: 404,
            excerpt: "\nno route\nsecond line".into(),
            location: None,
            trace: None,
        };
        let bare_redirect = FetchError::Status {
            status: 302,
            excerpt: String::new(),
            location: None,
            trace: None,
        };
        let unsupported = FetchError::Status {
            status: 415,
            excerpt: String::new(),
            location: None,
            trace: None,
        };
        let messages = [moved, missing, bare_redirect, unsupported]
            .map(|error| error.to_string() + "\n")
            .concat();
        expect![[r#"
            the upstream answered HTTP 301 with Location https://sourcegraph.com/.api/graphql: <html>
            the upstream answered HTTP 404: no route
            the upstream answered HTTP 302 with no Location
            the upstream answered HTTP 415
        "#]].assert_eq(&messages);
    }

    #[test]
    fn fetch_errors_are_unanswered_with_their_trace() {
        let trace = || Some("abc123".to_owned());
        let errors = [
            FetchError::Transport {
                url: "https://sourcegraph.com/.api/graphql".into(),
                cause: "Connection refused (os error 111)".into(),
            },
            FetchError::Status {
                status: 500,
                excerpt: "internal\nerror".into(),
                location: None,
                trace: trace(),
            },
            FetchError::NotJson { excerpt: "<html>".into(), trace: trace() },
            FetchError::Empty { trace: trace() },
            FetchError::Field {
                cause: "missing field `repository`".into(),
                trace: trace(),
            },
            FetchError::Server {
                errors: vec!["boom".into(), "bang".into()],
                trace: trace(),
            },
            FetchError::Schema {
                errors: vec!["Cannot query field \"nope\"".into()],
                trace: None,
            },
        ];
        let mut text = String::new();
        for error in &errors {
            assert_eq!(error.outcome(), Outcome::Unanswered);
            let _ = writeln!(text, "[{}]", error.code());
            text.push_str(&report(error));
            if let Some(detail) = error.detail() {
                let _ = writeln!(text, "detail: {detail}");
            }
        }
        expect![[r#"
            [transport]
            error: the request to https://sourcegraph.com/.api/graphql failed
            caused by: Connection refused (os error 111)
            [bad_response]
            error: the upstream answered HTTP 500: internal
            detail: {"trace":"abc123"}
            [bad_response]
            error: the upstream's answer isn't JSON: <html>
            detail: {"trace":"abc123"}
            [bad_response]
            error: the upstream's answer is empty
            detail: {"trace":"abc123"}
            [bad_response]
            error: the upstream's answer has missing or unknown fields
            caused by: missing field `repository`
            detail: {"trace":"abc123"}
            [server_error]
            error: the upstream answered with errors
            caused by: boom
            caused by: bang
            detail: {"trace":"abc123"}
            [schema_mismatch]
            error: the upstream rejected sego's query
            caused by: Cannot query field "nope"
        "#]]
        .assert_eq(&text);
    }
}
