use std::time::Instant;

use serde_json::Value;

use super::resolve::Request;
use super::transport::{GRAPHQL_PATH, Reply, Transport};
use crate::error::Error;

const COMPLEXITY_CODE: &str = "ErrQueryComplexityLimitExceeded";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Seg {
    Key(String),
    Index(usize),
    /// A path element that is neither a string nor an index; it matches
    /// nothing.
    Invalid,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GqlError {
    pub message: String,
    pub path: Vec<Seg>,
}

#[derive(Debug)]
pub struct Envelope {
    pub data: Value,
    pub errors: Vec<GqlError>,
    pub host: String,
}

impl Envelope {
    /// An error whose path is `path` or a prefix of it. Index segments never
    /// match a key.
    pub fn error_at(&self, path: &[&str]) -> Option<&GqlError> {
        self.errors.iter().find(|error| {
            error.path.len() <= path.len()
                && error
                    .path
                    .iter()
                    .zip(path)
                    .all(|(seg, key)| matches!(seg, Seg::Key(k) if k == key))
        })
    }

    /// An error whose path starts with `path`: at it or anywhere below it.
    pub fn error_beneath(&self, path: &[&str]) -> Option<&GqlError> {
        self.errors.iter().find(|error| {
            error.path.len() >= path.len()
                && error
                    .path
                    .iter()
                    .zip(path)
                    .all(|(seg, key)| matches!(seg, Seg::Key(k) if k == key))
        })
    }

    /// The value at `path`, or `None` when an error covers it, it is absent,
    /// or it is null.
    pub fn get(&self, path: &[&str]) -> Option<&Value> {
        if self.error_at(path).is_some() {
            return None;
        }
        let mut value = &self.data;
        for key in path {
            value = value.get(key)?;
        }
        (!value.is_null()).then_some(value)
    }
}

pub fn is_html(reply: &Reply) -> bool {
    reply
        .content_type
        .as_deref()
        .map(media_type)
        .is_some_and(|media| media.eq_ignore_ascii_case("text/html"))
}

/// The refusals every reply shares, whatever it was asked for: an edge
/// challenge, a redirect, a rate limit or a server error.
pub fn refusal(reply: &Reply, host: &str) -> Option<Error> {
    let status = reply.status;
    if reply.cf_mitigated.as_deref() == Some("challenge")
        || (is_html(reply) && matches!(status, 403 | 429))
    {
        return Some(Error::edge(format!(
            "{host}'s edge refused the request (HTTP {status}, \
             Cloudflare challenge)"
        )));
    }
    if (300..400).contains(&status) {
        return Some(Error::upstream(match &reply.location {
            Some(location) => format!(
                "{host} answered HTTP {status} with Location \
                 {location}"
            ),
            None => format!("{host} answered HTTP {status} with no Location"),
        }));
    }
    if status == 429 || status >= 500 {
        let error = Error::upstream(format!("{host} answered HTTP {status}"));
        return Some(match &reply.retry_after {
            Some(value) => error.fact(format!("Retry-After: {value}")),
            None => error,
        });
    }
    None
}

pub fn exchange(
    transport: &dyn Transport,
    request: &Request,
    deadline: Instant,
) -> Result<Envelope, Error> {
    let host = transport.host();
    let reply = transport
        .post(GRAPHQL_PATH, &request.body(), deadline)
        .map_err(|error| error.into_error(&host))?;
    classify(reply, &host)
}

pub fn classify(reply: Reply, host: &str) -> Result<Envelope, Error> {
    if let Some(error) = refusal(&reply, host) {
        return Err(error);
    }
    let Reply { status, content_type, body, .. } = reply;
    let media = content_type.as_deref().map(media_type);
    let json_type = media.is_some_and(is_json);
    let parsed = serde_json::from_slice::<Value>(&body);
    if status == 400
        && let Ok(json) = &parsed
        && let Some(message) = complexity_message(json)
    {
        return Err(Error::upstream(format!(
            "Sourcegraph rejected the request: {message}"
        )));
    }
    if status != 200 && !(json_type && parsed.is_ok()) {
        return Err(Error::upstream(format!(
            "{host} answered HTTP {status} {}",
            media.unwrap_or("without a content type")
        )));
    }
    if body.iter().all(u8::is_ascii_whitespace) {
        return Err(not_graphql(host, "empty"));
    }
    if !json_type {
        return Err(not_graphql(host, "not JSON"));
    }
    let json = match parsed {
        Ok(json) => json,
        Err(e) if e.is_eof() => return Err(not_graphql(host, "truncated")),
        Err(_) => return Err(not_graphql(host, "not JSON")),
    };
    let data = json.get("data").cloned().unwrap_or(Value::Null);
    let errors = read_errors(&json);
    let no_data = match &data {
        Value::Null => true,
        Value::Object(map) => map.is_empty(),
        _ => false,
    };
    if no_data && errors.is_empty() {
        return Err(not_graphql(host, "empty"));
    }
    if let Some(error) = errors.iter().find(|error| error.path.is_empty()) {
        let message = if error.message.is_empty() {
            "Sourcegraph rejected sego's request".to_owned()
        } else {
            format!("Sourcegraph rejected sego's request: {}", error.message)
        };
        return Err(Error::upstream(message));
    }
    Ok(Envelope { data, errors: dedup(errors), host: host.to_owned() })
}

fn not_graphql(host: &str, which: &str) -> Error {
    Error::upstream(format!(
        "{host}'s answer isn't a GraphQL response: {which}"
    ))
}

pub fn media_type(content_type: &str) -> &str {
    content_type.split(';').next().unwrap_or_default().trim()
}

fn is_json(media: &str) -> bool {
    media.eq_ignore_ascii_case("application/json")
        || media.to_ascii_lowercase().ends_with("+json")
}

fn complexity_message(body: &Value) -> Option<&str> {
    body.get("errors")?.as_array()?.iter().find_map(|error| {
        let code = error.get("extensions")?.get("code")?.as_str()?;
        (code == COMPLEXITY_CODE)
            .then(|| error.get("message").and_then(Value::as_str))
            .flatten()
    })
}

fn read_errors(body: &Value) -> Vec<GqlError> {
    let Some(list) = body.get("errors").and_then(Value::as_array) else {
        return Vec::new();
    };
    list.iter()
        .map(|error| GqlError {
            message: error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            path: error
                .get("path")
                .and_then(Value::as_array)
                .map(|path| path.iter().map(seg).collect())
                .unwrap_or_default(),
        })
        .collect()
}

fn seg(value: &Value) -> Seg {
    match value {
        Value::String(key) => Seg::Key(key.clone()),
        Value::Number(n) => n
            .as_u64()
            .and_then(|i| usize::try_from(i).ok())
            .map_or(Seg::Invalid, Seg::Index),
        _ => Seg::Invalid,
    }
}

fn dedup(errors: Vec<GqlError>) -> Vec<GqlError> {
    let mut kept: Vec<GqlError> = Vec::new();
    for error in errors {
        if !kept.contains(&error) {
            kept.push(error);
        }
    }
    kept
}

#[cfg(test)]
mod tests;
