use std::ffi::OsStr;
use std::io::{self, Read};
use std::time::{Duration, Instant};

use crate::cli::SYNOPSIS;
use crate::error::Error;

const DEFAULT_ENDPOINT: &str = "https://sourcegraph.com";
const VERSION: &str = env!("CARGO_PKG_VERSION");

pub const DEADLINE: Duration = Duration::from_secs(90);
pub const BODY_LIMIT: u64 = 64 * 1024 * 1024;
pub const GRAPHQL_PATH: &str = "/.api/graphql";

/// What the server answered to one request.
#[derive(Debug)]
pub struct Reply {
    /// The HTTP status code.
    pub status: u16,
    /// The `Content-Type` header.
    pub content_type: Option<String>,
    /// The `Location` header of a redirect.
    pub location: Option<String>,
    /// The `Retry-After` header.
    pub retry_after: Option<String>,
    /// The `CF-Mitigated` header, set on a Cloudflare challenge.
    pub cf_mitigated: Option<String>,
    /// The response body.
    pub body: Vec<u8>,
}

/// Why a request got no reply.
#[derive(Debug, PartialEq, Eq)]
pub enum TransportError {
    /// The connection failed; the text says how, without naming the host.
    Network(&'static str),
    /// The deadline passed before the reply arrived.
    Deadline,
    /// The body was larger than sego reads.
    TooLarge,
}

/// How `run` talks to the server.
pub trait Transport {
    /// The `host[:port]` that error messages name.
    fn host(&self) -> String {
        "sourcegraph.com".to_owned()
    }

    /// Posts `body` as JSON to `path` on the endpoint.
    fn post(
        &self,
        path: &str,
        body: &[u8],
        deadline: Instant,
    ) -> Result<Reply, TransportError>;

    /// Gets `path` from the endpoint.
    fn get(
        &self,
        path: &str,
        deadline: Instant,
    ) -> Result<Reply, TransportError>;
}

/// The server `run` reads from, validated from `SEGO_ENDPOINT`.
#[derive(Clone, Debug)]
pub struct Endpoint {
    base: String,
}

impl Endpoint {
    /// The endpoint named by `SEGO_ENDPOINT`, or sourcegraph.com when it is
    /// unset or empty.
    pub fn from_env(value: Option<&OsStr>) -> Result<Endpoint, Error> {
        let Some(value) = value.filter(|value| !value.is_empty()) else {
            return Ok(Endpoint { base: DEFAULT_ENDPOINT.to_owned() });
        };
        let Some(text) = value.to_str() else {
            return Err(Error::usage(
                "SEGO_ENDPOINT is not valid UTF-8",
                SYNOPSIS,
            ));
        };
        if !is_endpoint(text) {
            return Err(Error::usage(
                format!(
                    "SEGO_ENDPOINT must be an http:// or https:// URL with \
                     a host and no query or fragment: {text}"
                ),
                SYNOPSIS,
            ));
        }
        Ok(Endpoint { base: text.trim_end_matches('/').to_owned() })
    }

    /// The URL of `path` on this endpoint.
    pub fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    /// The base URL with a lowercase scheme and host and no `user[:pw]@`,
    /// so that spellings of one endpoint share a cache.
    pub fn cache_key(&self) -> String {
        let (scheme, rest) =
            self.base.split_once("://").unwrap_or(("", &self.base));
        let (authority, path) =
            rest.find('/').map_or((rest, ""), |at| rest.split_at(at));
        let host_port = authority.rsplit('@').next().unwrap_or_default();
        format!(
            "{}://{}{path}",
            scheme.to_ascii_lowercase(),
            host_port.to_ascii_lowercase(),
        )
    }

    /// The base URL's `host[:port]`, without any `user[:pw]@`.
    pub fn host(&self) -> String {
        let (_, rest) =
            self.base.split_once("://").unwrap_or(("", &self.base));
        let authority = rest.split('/').next().unwrap_or_default();
        authority.rsplit('@').next().unwrap_or_default().to_owned()
    }
}

fn is_endpoint(text: &str) -> bool {
    let Some((scheme, rest)) = text.split_once("://") else {
        return false;
    };
    if !scheme.eq_ignore_ascii_case("http")
        && !scheme.eq_ignore_ascii_case("https")
    {
        return false;
    }
    if text.contains(['?', '#']) || text.chars().any(char::is_whitespace) {
        return false;
    }
    let authority = rest.split('/').next().unwrap_or_default();
    let host_port = authority.rsplit('@').next().unwrap_or_default();
    let (host, port) = if host_port.starts_with('[') {
        host_port
            .split_once(']')
            .map_or(("", None), |(host, rest)| (host, rest.strip_prefix(':')))
    } else {
        match host_port.split_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (host_port, None),
        }
    };
    !host.is_empty()
        && host != "["
        && port.is_none_or(|port| {
            !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit())
        })
}

pub struct Ureq {
    agent: ureq::Agent,
    endpoint: Endpoint,
}

impl Ureq {
    pub fn new(endpoint: Endpoint) -> Ureq {
        Ureq::build(endpoint, true)
    }

    /// Ignores `HTTP_PROXY` and friends, so tests that talk to 127.0.0.1
    /// don't depend on the environment of the process running them.
    #[doc(hidden)]
    pub fn without_proxy(endpoint: Endpoint) -> Ureq {
        Ureq::build(endpoint, false)
    }

    fn build(endpoint: Endpoint, env_proxy: bool) -> Ureq {
        let builder = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .max_redirects(0)
            .max_redirects_will_error(false)
            .user_agent(format!("sego/{VERSION}"));
        let builder = if env_proxy { builder } else { builder.proxy(None) };
        Ureq { agent: builder.build().into(), endpoint }
    }
}

fn identify<B>(request: ureq::RequestBuilder<B>) -> ureq::RequestBuilder<B> {
    request
        .header("X-Requested-With", format!("sego/{VERSION}"))
        .header("X-Sourcegraph-API-Client-Name", "sego")
        .header("X-Sourcegraph-API-Client-Version", VERSION)
}

fn remaining(deadline: Instant) -> Result<Duration, TransportError> {
    let left = deadline.saturating_duration_since(Instant::now());
    if left.is_zero() { Err(TransportError::Deadline) } else { Ok(left) }
}

impl Transport for Ureq {
    fn host(&self) -> String {
        self.endpoint.host()
    }

    fn post(
        &self,
        path: &str,
        body: &[u8],
        deadline: Instant,
    ) -> Result<Reply, TransportError> {
        let timeout = remaining(deadline)?;
        let request = identify(self.agent.post(self.endpoint.url(path)))
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .config()
            .timeout_global(Some(timeout))
            .build();
        reply(request.send(body))
    }

    fn get(
        &self,
        path: &str,
        deadline: Instant,
    ) -> Result<Reply, TransportError> {
        let timeout = remaining(deadline)?;
        let request = identify(self.agent.get(self.endpoint.url(path)))
            .config()
            .timeout_global(Some(timeout))
            .build();
        reply(request.call())
    }
}

fn header(
    response: &ureq::http::Response<ureq::Body>,
    name: &str,
) -> Option<String> {
    response
        .headers()
        .get(name)
        .map(|value| String::from_utf8_lossy(value.as_bytes()).into_owned())
}

fn reply(
    result: Result<ureq::http::Response<ureq::Body>, ureq::Error>,
) -> Result<Reply, TransportError> {
    let mut response = result.map_err(classify)?;
    let mut reply = Reply {
        status: response.status().as_u16(),
        content_type: header(&response, "content-type"),
        location: header(&response, "location"),
        retry_after: header(&response, "retry-after"),
        cf_mitigated: header(&response, "cf-mitigated"),
        body: Vec::new(),
    };
    response
        .body_mut()
        .as_reader()
        .take(BODY_LIMIT + 1)
        .read_to_end(&mut reply.body)
        .map_err(classify_io)?;
    if reply.body.len() as u64 > BODY_LIMIT {
        return Err(TransportError::TooLarge);
    }
    Ok(reply)
}

fn classify(error: ureq::Error) -> TransportError {
    match error {
        ureq::Error::Timeout(_) => TransportError::Deadline,
        ureq::Error::HostNotFound => {
            TransportError::Network("DNS lookup failed")
        }
        ureq::Error::Io(error) => classify_io(error),
        ureq::Error::Tls(_) | ureq::Error::Rustls(_) | ureq::Error::Pem(_) => {
            TransportError::Network("TLS failed")
        }
        _ => TransportError::Network("network error"),
    }
}

fn classify_io(error: io::Error) -> TransportError {
    let kind = error.kind();
    if let Some(inner) = error.into_inner()
        && let Ok(inner) = inner.downcast::<ureq::Error>()
    {
        return classify(*inner);
    }
    match kind {
        io::ErrorKind::ConnectionRefused => {
            TransportError::Network("connection refused")
        }
        io::ErrorKind::ConnectionReset | io::ErrorKind::ConnectionAborted => {
            TransportError::Network("connection reset")
        }
        io::ErrorKind::UnexpectedEof
        | io::ErrorKind::BrokenPipe
        | io::ErrorKind::NotConnected => {
            TransportError::Network("connection closed")
        }
        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock => {
            TransportError::Deadline
        }
        _ => TransportError::Network("network error"),
    }
}

impl TransportError {
    pub fn into_error(self, host: &str) -> Error {
        match self {
            TransportError::Network(cause) => {
                Error::network(format!("could not reach {host}: {cause}"))
            }
            TransportError::Deadline => Error::deadline(host),
            TransportError::TooLarge => {
                Error::upstream(format!("{host}'s answer is over 64 MiB"))
            }
        }
    }
}
