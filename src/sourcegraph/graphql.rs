use serde_json::Value;

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Map;

use super::Response;
use crate::error::Error;

const COMPLEXITY_CODE: &str = "ErrQueryComplexityLimitExceeded";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Segment {
    Key(String),
    Index(usize),
    /// A path element that is neither a string nor an index; it matches
    /// nothing.
    Invalid,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GqlError {
    pub message: String,
    pub path: Vec<Segment>,
    /// `extensions.code`.
    pub code: Option<String>,
}

/// What a server error says, whatever its wording; each reader decides what
/// it means where the error sits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GqlErrorKind {
    /// A revision doesn't resolve, including after waiting for a fetch.
    RevisionNotFound,
    /// The file is over the content limit.
    OverLimit,
    /// A search's regular expression doesn't compile.
    Regexp,
    /// The query is over the server's complexity limit.
    Complexity,
    /// Anything else, which includes real server failures: gitserver already
    /// sorts its own not-found exits into `RevisionNotFound`.
    Other,
}

impl GqlError {
    pub fn kind(&self) -> GqlErrorKind {
        if self.code.as_deref() == Some(COMPLEXITY_CODE) {
            GqlErrorKind::Complexity
        } else if self.message.starts_with("revision not found") {
            GqlErrorKind::RevisionNotFound
        } else if self.message.starts_with("file exceeded size limit") {
            GqlErrorKind::OverLimit
        } else if self.regexp_reason().is_some() {
            GqlErrorKind::Regexp
        } else {
            GqlErrorKind::Other
        }
    }

    /// See `regexp_reason`.
    pub fn regexp_reason(&self) -> Option<&str> {
        regexp_reason(&self.message)
    }
}

/// The first line of the reason a Go regexp didn't compile, from an error's
/// message or an alert's description, which may come wrapped in gRPC
/// errors; `None` for any other text.
pub fn regexp_reason(text: &str) -> Option<&str> {
    let mut rest = text;
    while let Some(after) = rest.strip_prefix("rpc error: code = ") {
        rest = after.split_once(" desc = ")?.1;
    }
    let reason = rest
        .strip_prefix("error parsing regexp: ")
        .or_else(|| rest.strip_prefix("Error parsing regexp: "))?;
    Some(reason.lines().next()?.trim())
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
                    .all(|(segment, key)| matches!(segment, Segment::Key(k) if k == key))
        })
    }

    /// An error whose path starts with `path`: at it or anywhere below it.
    pub fn error_beneath(&self, path: &[&str]) -> Option<&GqlError> {
        self.errors_beneath(path).into_iter().next()
    }

    /// Every error whose path starts with `path`.
    pub fn errors_beneath(&self, path: &[&str]) -> Vec<&GqlError> {
        let beneath = |error: &&GqlError| {
            error.path.len() >= path.len()
                && error
                    .path
                    .iter()
                    .zip(path)
                    .all(|(segment, key)| matches!(segment, Segment::Key(k) if k == key))
        };
        self.errors.iter().filter(beneath).collect()
    }

    /// The value at `path`, or `None` when an error covers it, it is missing,
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

pub fn is_html(response: &Response) -> bool {
    response
        .content_type
        .as_deref()
        .map(media_type)
        .is_some_and(|media| media.eq_ignore_ascii_case("text/html"))
}

/// The refusals every response shares, whatever it was asked for: an edge
/// challenge, a redirect, a rate limit or a server error.
pub fn refusal(response: &Response, host: &str) -> Option<Error> {
    let status = response.status;
    if response.cf_mitigated.as_deref() == Some("challenge")
        || (is_html(response) && matches!(status, 403 | 429))
    {
        return Some(Error::edge(format!(
            "{host}'s edge refused the request (HTTP {status}, \
             Cloudflare challenge)"
        )));
    }
    if (300..400).contains(&status) {
        return Some(Error::upstream(match &response.location {
            Some(location) => format!(
                "{host} answered HTTP {status} with Location \
                 {location}"
            ),
            None => format!("{host} answered HTTP {status} with no Location"),
        }));
    }
    if status == 429 || status >= 500 {
        let error = Error::upstream(format!("{host} answered HTTP {status}"));
        return Some(match &response.retry_after {
            Some(value) => error.fact(format!("Retry-After: {value}")),
            None => error,
        });
    }
    None
}

pub fn classify(response: Response, host: &str) -> Result<Envelope, Error> {
    if let Some(error) = refusal(&response, host) {
        return Err(error);
    }
    let Response { status, content_type, body, .. } = response;
    let media = content_type.as_deref().map(media_type);
    let json_type = media.is_some_and(is_json);
    let parsed = serde_json::from_slice::<Value>(&body);
    if status == 400
        && let Ok(json) = &parsed
        && let Some(message) = complexity_message(json)
    {
        return Err(Error::upstream(format!("Sourcegraph rejected the request: {message}")));
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
    Error::upstream(format!("{host}'s answer isn't a GraphQL response: {which}"))
}

pub fn media_type(content_type: &str) -> &str {
    content_type.split(';').next().unwrap_or_default().trim()
}

fn is_json(media: &str) -> bool {
    media.eq_ignore_ascii_case("application/json") || media.to_ascii_lowercase().ends_with("+json")
}

fn complexity_message(body: &Value) -> Option<String> {
    read_errors(body)
        .into_iter()
        .find(|error| error.kind() == GqlErrorKind::Complexity)
        .map(|error| error.message)
}

fn read_errors(body: &Value) -> Vec<GqlError> {
    let Some(list) = body.get("errors").and_then(Value::as_array) else {
        return Vec::new();
    };
    list.iter()
        .map(|error| GqlError {
            message: error.get("message").and_then(Value::as_str).unwrap_or_default().to_owned(),
            path: error
                .get("path")
                .and_then(Value::as_array)
                .map(|path| path.iter().map(segment).collect())
                .unwrap_or_default(),
            code: error
                .get("extensions")
                .and_then(|e| e.get("code"))
                .and_then(Value::as_str)
                .map(str::to_owned),
        })
        .collect()
}

fn segment(value: &Value) -> Segment {
    match value {
        Value::String(key) => Segment::Key(key.clone()),
        Value::Number(n) => n
            .as_u64()
            .and_then(|i| usize::try_from(i).ok())
            .map_or(Segment::Invalid, Segment::Index),
        _ => Segment::Invalid,
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

pub struct Request {
    pub operation: String,
    pub document: String,
    pub variables: Map<String, Value>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Body<'a> {
    query: &'a str,
    variables: &'a Map<String, Value>,
    operation_name: &'a str,
}

impl Request {
    pub fn new(operation: &str, document: String, variables: Map<String, Value>) -> Request {
        Request { operation: operation.to_owned(), document, variables }
    }

    /// # Panics
    ///
    /// Never in practice: the body is strings and JSON values.
    #[expect(clippy::expect_used, reason = "a body of strings and JSON values always serializes")]
    pub fn body(&self) -> Vec<u8> {
        let body = Body {
            query: &self.document,
            variables: &self.variables,
            operation_name: &self.operation,
        };
        serde_json::to_vec(&body).expect("a body of strings and JSON values serializes")
    }
}

/// `selection` once for each of `count` items, a line each, with every `#`
/// in it replaced by the item's index: the aliases and variables of item
/// `n` end in `n`. A variable declaration repeats this way too, since
/// GraphQL reads a line break as it reads a comma.
pub fn repeated(selection: &str, count: usize) -> String {
    (0..count).map(|n| selection.replace('#', &n.to_string())).collect::<Vec<_>>().join("\n")
}

/// An envelope read relative to the commit's alias path, such as
/// `["r0", "c0"]`.
pub struct View<'a> {
    envelope: &'a Envelope,
    at: &'a [String],
}

impl<'a> View<'a> {
    pub fn new(envelope: &'a Envelope, at: &'a [String]) -> View<'a> {
        View { envelope, at }
    }

    fn path<'p>(&'p self, tail: &[&'p str]) -> Vec<&'p str> {
        let mut path: Vec<&str> = self.at.iter().map(String::as_str).collect();
        path.extend_from_slice(tail);
        path
    }

    /// See `Envelope::get`.
    pub fn get(&self, tail: &[&str]) -> Option<&Value> {
        self.envelope.get(&self.path(tail))
    }

    /// Whether an error sits at `tail` or anywhere below it.
    pub fn errored_beneath(&self, tail: &[&str]) -> bool {
        self.envelope.error_beneath(&self.path(tail)).is_some()
    }

    /// See `Envelope::errors_beneath`.
    pub fn errors_beneath(&self, tail: &[&str]) -> Vec<&GqlError> {
        self.envelope.errors_beneath(&self.path(tail))
    }

    /// The value at `tail` read as a `T`, or `None` when `get` finds none or
    /// it doesn't have `T`'s shape.
    pub fn read<T: DeserializeOwned>(&self, tail: &[&str]) -> Option<T> {
        T::deserialize(self.get(tail)?).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use serde_json::{Value, json};

    use crate::error::{Error, Kind, report};
    use crate::sourcegraph::Response;

    const NOT_GRAPHQL: &str = "sourcegraph.com's answer isn't a GraphQL response";

    fn error(message: &str) -> GqlError {
        GqlError {
            message: message.to_owned(),
            path: vec![Segment::Key("r0".to_owned())],
            code: None,
        }
    }

    /// Messages from responses recorded from sourcegraph.com, each with the
    /// kind it is.
    const KINDS: [(&str, GqlErrorKind); 9] = [
        ("revision not found: repo 1700 @deadbeefdeadbeef^0", GqlErrorKind::RevisionNotFound),
        (
            "revision not found after waiting for a fetch already in progress: repo 1700 @v9.9.9",
            GqlErrorKind::RevisionNotFound,
        ),
        (
            "file exceeded size limit: BytesRead=1572865 (may be lower than file size)",
            GqlErrorKind::OverLimit,
        ),
        (
            "rpc error: code = Unknown desc = error parsing regexp: invalid or unsupported Perl \
             syntax: `(?<`",
            GqlErrorKind::Regexp,
        ),
        ("not a blob: \".github\"", GqlErrorKind::Other),
        ("not a directory: \"mux.go\"", GqlErrorKind::Other),
        // Recorded 2026-10-27 in show/upstream-revision: git exit 128 at a
        // revision lookup.
        (
            "rpc error: code = Unknown desc = git command [git -c gc.auto=0 -c \
             maintenance.auto=0 rev-parse @{upstream}^0 --] failed with status code 128",
            GqlErrorKind::Other,
        ),
        // Recorded 2026-10-27 in diff/path-that-leaves-the-root: git exit 128
        // at a path probe.
        (
            "rpc error: code = Unknown desc = git command [git -c gc.auto=0 -c \
             maintenance.auto=0 ls-tree --long --full-name -z \
             98cb6bf42e086f6af920b965c38cacc07402d51b -- :(literal)../mux.go] failed with status \
             code 128",
            GqlErrorKind::Other,
        ),
        ("panic occurred: runtime error: slice bounds out of range [:-1]", GqlErrorKind::Other),
    ];

    #[test]
    fn an_error_is_sorted_by_its_message() {
        for (message, kind) in KINDS {
            assert_eq!(error(message).kind(), kind, "{message}");
        }
    }

    #[test]
    fn a_query_over_the_complexity_limit_is_known_by_its_code() {
        let over = GqlError { code: Some(COMPLEXITY_CODE.to_owned()), ..error("too complex") };
        assert_eq!(over.kind(), GqlErrorKind::Complexity);
    }

    #[test]
    fn a_regexp_error_gives_the_first_line_of_its_reason() {
        let wrapped = error(
            "rpc error: code = Unknown desc = error parsing regexp: missing closing ): `(a`\nmore",
        );
        assert_eq!(wrapped.regexp_reason(), Some("missing closing ): `(a`"));
        assert_eq!(error("not a blob").regexp_reason(), None);
    }

    const HOST: &str = "sourcegraph.com";

    const JSON: Option<&str> = Some("application/json");

    fn json_response(status: u16, body: &Value) -> Response {
        Response::of(status, JSON, body.to_string())
    }

    fn refused(response: Response) -> Error {
        match classify(response, HOST) {
            Ok(envelope) => panic!("accepted {envelope:?}"),
            Err(error) => error,
        }
    }

    fn lfs_failures_response() -> Response {
        let failure = json!({
            "message": "git blob failed",
            "path": ["r0", "c0", "meta", "lfs"],
        });
        json_response(
            200,
            &json!({
                "data": {"r0": {"c0": {"meta": {"lfs": null, "size": 3}}}},
                "errors": [
                    failure,
                    failure,
                    {"message": "other", "path": ["r0", "c0", "meta", "lfs"]},
                ],
            }),
        )
    }

    fn lfs_failures() -> Envelope {
        classify(lfs_failures_response(), HOST).unwrap_or_else(|e| panic!("{e}"))
    }

    type Row = (&'static str, Response, Kind, String);

    /// Responses refused by their status and headers.
    fn refusal_rows() -> Vec<Row> {
        let challenge = "sourcegraph.com's edge refused the request (HTTP 403, \
                     Cloudflare challenge)";
        vec![
            (
                "challenge",
                Response {
                    cf_mitigated: Some("challenge".to_owned()),
                    ..Response::of(
                        403,
                        Some("text/html; charset=UTF-8"),
                        "<html><title>Just a moment...</title></html>",
                    )
                },
                Kind::Edge,
                challenge.into(),
            ),
            (
                "redirect",
                Response {
                    location: Some("https://sourcegraph.com/.api/graphql/".to_owned()),
                    ..Response::of(301, None, "")
                },
                Kind::Upstream,
                "sourcegraph.com answered HTTP 301 with Location \
             https://sourcegraph.com/.api/graphql/"
                    .into(),
            ),
            (
                "complexity",
                json_response(
                    400,
                    &json!({"errors": [{
                        "message": "query complexity limit exceeded (cost 5001, \
                                    limit 5000)",
                        "extensions": {"code": "ErrQueryComplexityLimitExceeded"},
                    }]}),
                ),
                Kind::Upstream,
                "Sourcegraph rejected the request: query complexity limit \
             exceeded (cost 5001, limit 5000)"
                    .into(),
            ),
            (
                "rate",
                json_response(429, &json!({"error": "rate limited"})),
                Kind::Upstream,
                "sourcegraph.com answered HTTP 429".into(),
            ),
            (
                "bad-gateway",
                Response::of(502, Some("text/plain"), "Bad Gateway"),
                Kind::Upstream,
                "sourcegraph.com answered HTTP 502".into(),
            ),
            (
                "html-404",
                Response::of(
                    404,
                    Some("text/html; charset=utf-8"),
                    "<html><body>Not found</body></html>",
                ),
                Kind::Upstream,
                "sourcegraph.com answered HTTP 404 text/html".into(),
            ),
            (
                "redirect-no-location",
                Response::of(302, None, ""),
                Kind::Upstream,
                "sourcegraph.com answered HTTP 302 with no Location".into(),
            ),
            (
                "html-403",
                Response::of(
                    403,
                    Some("Text/HTML; charset=utf-8"),
                    "<html><body>Forbidden</body></html>",
                ),
                Kind::Edge,
                challenge.into(),
            ),
            (
                "challenge-text",
                Response {
                    cf_mitigated: Some("challenge".to_owned()),
                    ..Response::of(403, Some("text/plain"), "Forbidden")
                },
                Kind::Edge,
                challenge.into(),
            ),
        ]
    }

    /// Responses refused by their content type or body.
    fn body_rows() -> Vec<Row> {
        vec![
            (
                "text-plain-envelope",
                Response::of(200, Some("text/plain"), r#"{"data":{"r0":{"name":"x"}}}"#),
                Kind::Upstream,
                format!("{NOT_GRAPHQL}: not JSON"),
            ),
            (
                "no-content-type",
                Response::of(200, None, r#"{"data":{"r0":{"name":"x"}}}"#),
                Kind::Upstream,
                format!("{NOT_GRAPHQL}: not JSON"),
            ),
            (
                "null-no-errors",
                json_response(200, &json!({"data": null, "errors": []})),
                Kind::Upstream,
                format!("{NOT_GRAPHQL}: empty"),
            ),
            (
                "neither",
                json_response(200, &json!({"hello": "world"})),
                Kind::Upstream,
                format!("{NOT_GRAPHQL}: empty"),
            ),
            (
                "bad-request",
                json_response(
                    400,
                    &json!({"errors": [{
                        "message": "Syntax Error",
                        "extensions": {"code": "ErrOther"},
                    }]}),
                ),
                Kind::Upstream,
                "Sourcegraph rejected sego's request: Syntax Error".into(),
            ),
            (
                "no-message",
                json_response(
                    200,
                    &json!({"data": null, "errors": [{"extensions": {"code": "x"}}]}),
                ),
                Kind::Upstream,
                "Sourcegraph rejected sego's request".into(),
            ),
            (
                "not-json",
                Response::of(200, Some("text/plain"), "this is not json"),
                Kind::Upstream,
                format!("{NOT_GRAPHQL}: not JSON"),
            ),
            (
                "truncated",
                Response::of(200, JSON, r#"{"data":{"r0":{"c0":{"meta":"#),
                Kind::Upstream,
                format!("{NOT_GRAPHQL}: truncated"),
            ),
            (
                "empty",
                json_response(200, &json!({"data": {}})),
                Kind::Upstream,
                format!("{NOT_GRAPHQL}: empty"),
            ),
            (
                "pathless",
                json_response(
                    200,
                    &json!({"data": null, "errors": [
                        {"message": "Unknown field \"nope\" on type \"Query\""},
                        {"message": "second"},
                    ]}),
                ),
                Kind::Upstream,
                "Sourcegraph rejected sego's request: Unknown field \"nope\" on \
             type \"Query\""
                    .into(),
            ),
        ]
    }

    #[test]
    fn a_response_is_classified_into_an_error_kind_and_message() {
        let rows = refusal_rows().into_iter().chain(body_rows());
        for (name, response, kind, message) in rows {
            let error = refused(response);
            assert_eq!(error.kind(), kind, "{name}");
            assert_eq!(error.to_string(), message, "{name}");
        }
        assert!(classify(lfs_failures_response(), HOST).is_ok());
    }

    #[test]
    fn messages_name_the_host_given() {
        let Err(error) = classify(Response::of(500, Some("text/plain"), "down"), "127.0.0.1:9")
        else {
            panic!("a 500 was accepted");
        };
        assert_eq!(error.to_string(), "127.0.0.1:9 answered HTTP 500");

        let envelope =
            classify(lfs_failures_response(), "127.0.0.1:9").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(envelope.host, "127.0.0.1:9");
    }

    #[test]
    fn rate_limit_carries_retry_after() {
        let limited = Response {
            retry_after: Some("30".to_owned()),
            ..json_response(429, &json!({"error": "rate limited"}))
        };
        assert_eq!(
            report(&refused(limited)),
            "sego: error: sourcegraph.com answered HTTP 429\n  Retry-After: 30\n"
        );
        let failing =
            Response { retry_after: Some("30".to_owned()), ..Response::of(502, None, "") };
        assert_eq!(
            report(&refused(failing)),
            "sego: error: sourcegraph.com answered HTTP 502\n  Retry-After: 30\n"
        );
        let gateway = Response::of(502, Some("text/plain"), "Bad Gateway");
        assert!(!report(&refused(gateway)).contains("Retry-After"));
    }

    #[test]
    fn an_error_covers_the_values_at_and_beneath_its_path() {
        let envelope = lfs_failures();
        assert!(envelope.get(&["r0", "c0", "meta"]).is_some());
        assert!(envelope.get(&["r0", "c0", "meta", "lfs"]).is_none());
        assert!(envelope.get(&["r0", "c0", "meta", "size"]).is_some());
        assert!(envelope.error_at(&["r0", "c0", "meta", "lfs", "byteSize"]).is_some());
        assert!(envelope.error_at(&["r0", "c0", "meta"]).is_none());
        assert!(envelope.error_at(&["r0", "c0", "meta", "lfs"]).is_some());
        let messages: Vec<&str> = envelope.errors.iter().map(|e| e.message.as_str()).collect();
        assert_eq!(messages, ["git blob failed", "other"]);
        let path: Vec<Segment> =
            ["r0", "c0", "meta", "lfs"].into_iter().map(|k| Segment::Key(k.to_owned())).collect();
        assert_eq!(envelope.errors[0].path, path);
    }

    #[test]
    fn errors_beneath_a_path() {
        let envelope = lfs_failures();
        let beneath = |path: &[&str]| envelope.error_beneath(path).is_some();
        assert!(beneath(&["r0", "c0", "meta", "lfs"]));
        assert!(beneath(&["r0", "c0", "meta"]));
        assert!(beneath(&["r0"]));
        assert!(beneath(&[]));
        assert!(!beneath(&["r0", "c0", "meta", "lfs", "byteSize"]));
        assert!(!beneath(&["r0", "c0", "body"]));
        assert!(!beneath(&["r1"]));
        assert!(envelope.error_at(&["r0", "c0", "meta"]).is_none());
        let first = envelope.error_beneath(&["r0", "c0", "meta"]);
        assert_eq!(first.map(|e| e.message.as_str()), Some("git blob failed"));
    }

    #[test]
    fn odd_paths_stay_located() {
        let odd = json_response(
            200,
            &json!({
                "data": {"r0": {"name": "x"}},
                "errors": [
                    {"message": "null element", "path": [null]},
                    {"message": "negative index", "path": ["r0", -1, "x"]},
                ],
            }),
        );
        let envelope = classify(odd, HOST).unwrap_or_else(|e| panic!("{e}"));
        let key = |k: &str| Segment::Key(k.to_owned());
        assert_eq!(envelope.errors.len(), 2);
        assert_eq!(envelope.errors[0].path, [Segment::Invalid]);
        assert_eq!(envelope.errors[1].path, [key("r0"), Segment::Invalid, key("x")]);
        assert!(envelope.error_at(&["r0", "x"]).is_none());
        assert!(envelope.get(&["r0", "name"]).is_some());
    }

    #[test]
    fn a_repeated_selection_numbers_each_repeat() {
        assert_eq!(
            repeated("p#: path(path: $path#)", 2),
            "p0: path(path: $path0)\np1: path(path: $path1)"
        );
        assert_eq!(repeated("p#", 0), "");
    }
}
