use serde_json::Value;

use super::transport::Reply;
use crate::error::Error;

const NOT_GRAPHQL: &str = "sourcegraph.com's answer isn't a GraphQL response";
const COMPLEXITY_CODE: &str = "ErrQueryComplexityLimitExceeded";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Seg {
    Key(String),
    Index(usize),
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

pub fn classify(reply: Reply) -> Result<Envelope, Error> {
    let Reply {
        status,
        content_type,
        location,
        retry_after,
        cf_mitigated,
        body,
    } = reply;
    let content_type = content_type.as_deref();
    let html = content_type
        .is_some_and(|ct| ct.trim_start().starts_with("text/html"));
    if cf_mitigated.as_deref() == Some("challenge")
        || (html && matches!(status, 403 | 429))
    {
        return Err(Error::edge(format!(
            "sourcegraph.com's edge refused the request (HTTP {status}, \
             Cloudflare challenge)"
        )));
    }
    if (300..400).contains(&status) {
        return Err(Error::upstream(match &location {
            Some(location) => format!(
                "sourcegraph.com answered HTTP {status} with Location \
                 {location}"
            ),
            None => format!(
                "sourcegraph.com answered HTTP {status} with no Location"
            ),
        }));
    }
    let parsed = serde_json::from_slice::<Value>(&body);
    if status == 400
        && let Ok(json) = &parsed
        && let Some(message) = complexity_message(json)
    {
        return Err(Error::upstream(format!(
            "Sourcegraph rejected the request: {message}"
        )));
    }
    if status == 429 || status >= 500 {
        let error =
            Error::upstream(format!("sourcegraph.com answered HTTP {status}"));
        return Err(match &retry_after {
            Some(value) => error.fact(format!("Retry-After: {value}")),
            None => error,
        });
    }
    if status != 200 && !is_json(content_type, parsed.is_ok()) {
        return Err(Error::upstream(format!(
            "sourcegraph.com answered HTTP {status} {}",
            content_type.unwrap_or("without a content type")
        )));
    }
    let json = match parsed {
        Ok(json) => json,
        Err(_) if body.iter().all(u8::is_ascii_whitespace) => {
            return Err(not_graphql("empty"));
        }
        Err(e) if e.is_eof() => return Err(not_graphql("truncated")),
        Err(_) => return Err(not_graphql("not JSON")),
    };
    let data = json.get("data").cloned().unwrap_or(Value::Null);
    let errors = read_errors(&json);
    let no_data = match &data {
        Value::Null => true,
        Value::Object(map) => map.is_empty(),
        _ => false,
    };
    if no_data && errors.is_empty() {
        return Err(not_graphql("empty"));
    }
    if let Some(error) = errors.iter().find(|error| error.path.is_empty()) {
        return Err(Error::upstream(format!(
            "Sourcegraph rejected sego's request: {}",
            error.message
        )));
    }
    Ok(Envelope { data, errors: dedup(errors) })
}

fn not_graphql(which: &str) -> Error {
    Error::upstream(format!("{NOT_GRAPHQL}: {which}"))
}

fn is_json(content_type: Option<&str>, parses: bool) -> bool {
    let media =
        content_type.and_then(|ct| ct.split(';').next()).map_or("", str::trim);
    parses
        && (media.eq_ignore_ascii_case("application/json")
            || media.to_ascii_lowercase().ends_with("+json"))
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
                .map(|path| path.iter().filter_map(seg).collect())
                .unwrap_or_default(),
        })
        .collect()
}

fn seg(value: &Value) -> Option<Seg> {
    match value {
        Value::String(key) => Some(Seg::Key(key.clone())),
        Value::Number(n) => {
            n.as_u64().and_then(|i| usize::try_from(i).ok()).map(Seg::Index)
        }
        _ => None,
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
