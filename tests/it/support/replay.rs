use std::fmt::Write as _;

use serde_json::Value;

use sego::internal::upstream::GRAPHQL_PATH;
use sego::{Reply, TransportError};

use super::fixture::{Fault, Fixture};

const CHALLENGE_LEN: usize = 5 * 1024;

enum Query {
    Graphql { operation: String, variables: Value, query: String },
    Raw { method: String, path: String },
}

impl Query {
    fn parse(method: &str, path: &str, body: &[u8]) -> Result<Query, String> {
        if method == "GET" {
            return Ok(Query::Raw {
                method: method.to_owned(),
                path: path.to_owned(),
            });
        }
        if method != "POST" || path != GRAPHQL_PATH {
            return Err(format!("no fixture for {method} {path}"));
        }
        let sent: Value = serde_json::from_slice(body)
            .map_err(|e| format!("the request body is not JSON: {e}"))?;
        let text = |key: &str| {
            sent.get(key)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        Ok(Query::Graphql {
            operation: text("operationName"),
            variables: sent.get("variables").cloned().unwrap_or(Value::Null),
            query: text("query"),
        })
    }

    fn matches(&self, fixture: &Fixture) -> bool {
        match self {
            Query::Graphql { operation, variables, .. } => {
                fixture.operation.as_ref() == Some(operation)
                    && fixture.variables.as_ref().unwrap_or(&Value::Null)
                        == variables
            }
            Query::Raw { method, path } => {
                fixture.request.as_ref().is_some_and(|request| {
                    request.method == *method && request.path == *path
                })
            }
        }
    }
}

fn plain(status: u16, text: String) -> Reply {
    Reply {
        status,
        content_type: Some("text/plain".to_owned()),
        location: None,
        retry_after: None,
        cf_mitigated: None,
        body: text.into_bytes(),
    }
}

fn normalise(query: &str) -> String {
    query.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn header(fixture: &Fixture, name: &str) -> Option<String> {
    fixture
        .headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.clone())
}

fn from_fixture(fixture: &Fixture, body: Vec<u8>) -> Reply {
    Reply {
        status: fixture.status,
        content_type: header(fixture, "content-type"),
        location: header(fixture, "location"),
        retry_after: header(fixture, "retry-after"),
        cf_mitigated: header(fixture, "cf-mitigated"),
        body,
    }
}

fn serve(fixture: &Fixture, query: &Query) -> Result<Reply, TransportError> {
    if let (Query::Graphql { query: sent, .. }, Some(stored)) =
        (query, &fixture.query)
        && normalise(stored) != normalise(sent)
    {
        return Ok(plain(
            500,
            format!(
                "stale fixture {}: query changed\n--- fixture\n{}\n--- \
                 sent\n{}",
                fixture.file.display(),
                normalise(stored),
                normalise(sent)
            ),
        ));
    }
    match fixture.fault {
        Some(Fault::Network) => {
            Err(TransportError::Network("connection closed"))
        }
        Some(Fault::HtmlChallenge) => {
            let mut body = b"<html><body>".to_vec();
            body.resize(CHALLENGE_LEN - b"</body></html>".len(), b'.');
            body.extend_from_slice(b"</body></html>");
            Ok(Reply {
                status: 429,
                content_type: Some("text/html".to_owned()),
                location: None,
                retry_after: None,
                cf_mitigated: Some("challenge".to_owned()),
                body,
            })
        }
        Some(Fault::Redirect) => {
            Ok(Reply { status: 302, ..from_fixture(fixture, Vec::new()) })
        }
        Some(Fault::Status) => Ok(from_fixture(fixture, Vec::new())),
        None => Ok(from_fixture(fixture, fixture.body())),
    }
}

fn missing(fixtures: &[Fixture], query: &Query) -> Reply {
    let (what, candidates): (String, Vec<&Fixture>) = match query {
        Query::Graphql { operation, variables, .. } => (
            format!("{operation} {variables}"),
            fixtures
                .iter()
                .filter(|f| f.operation.as_ref() == Some(operation))
                .collect(),
        ),
        Query::Raw { method, path } => (
            format!("{method} {path}"),
            fixtures.iter().filter(|f| f.request.is_some()).collect(),
        ),
    };
    let mut message = format!("no fixture for {what}");
    if candidates.is_empty() {
        message.push_str("\nthere are no fixtures of that kind");
    } else {
        message.push_str("\nfixtures of that kind:");
        for fixture in candidates {
            let _ = write!(
                message,
                "
  {}: {}",
                fixture.file.display(),
                fixture.key()
            );
        }
    }
    plain(500, message)
}

/// The matching core: the HTTP server and in-process fakes both call it.
pub fn answer(
    fixtures: &[Fixture],
    method: &str,
    path: &str,
    body: &[u8],
) -> Result<Reply, TransportError> {
    let query = match Query::parse(method, path, body) {
        Ok(query) => query,
        Err(message) => return Ok(plain(500, message)),
    };
    match fixtures.iter().find(|f| query.matches(f)) {
        Some(fixture) => serve(fixture, &query),
        None => Ok(missing(fixtures, &query)),
    }
}
