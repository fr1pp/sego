use std::ffi::OsStr;
use std::io::{self, Read};
use std::time::{Duration, Instant};

use crate::error::Error;

const SYNOPSIS: &str = "sego COMMAND [--json] ...";
const DEFAULT_ENDPOINT: &str = "https://sourcegraph.com";
const VERSION: &str = env!("CARGO_PKG_VERSION");

pub const DEADLINE: Duration = Duration::from_secs(90);
pub const BODY_LIMIT: u64 = 64 * 1024 * 1024;
pub const GRAPHQL_PATH: &str = "/.api/graphql";

#[derive(Debug)]
pub struct Reply {
    pub status: u16,
    pub content_type: Option<String>,
    pub location: Option<String>,
    pub retry_after: Option<String>,
    pub cf_mitigated: Option<String>,
    pub body: Vec<u8>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum TransportError {
    Network(&'static str),
    Deadline,
    TooLarge,
}

pub trait Transport {
    fn post(
        &self,
        path: &str,
        body: &[u8],
        deadline: Instant,
    ) -> Result<Reply, TransportError>;

    fn get(
        &self,
        path: &str,
        deadline: Instant,
    ) -> Result<Reply, TransportError>;
}

#[derive(Clone, Debug)]
pub struct Endpoint {
    base: String,
}

impl Endpoint {
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

    pub fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }
}

fn is_endpoint(text: &str) -> bool {
    let Some(rest) =
        text.strip_prefix("http://").or_else(|| text.strip_prefix("https://"))
    else {
        return false;
    };
    if text.contains(['?', '#']) || text.chars().any(char::is_whitespace) {
        return false;
    }
    let authority = rest.split('/').next().unwrap_or_default();
    let host_port = authority.rsplit('@').next().unwrap_or_default();
    let host = if host_port.starts_with('[') {
        host_port.split_once(']').map_or("", |(host, _)| host)
    } else {
        host_port.split(':').next().unwrap_or_default()
    };
    !host.is_empty() && host != "["
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
        ureq::Error::Tls(_) => TransportError::Network("TLS failed"),
        // `Error::Rustls` exists only under ureq's rustls feature, which
        // sego can't name in a cfg.
        other if format!("{other:?}").starts_with("Rustls(") => {
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

impl From<TransportError> for Error {
    fn from(error: TransportError) -> Error {
        match error {
            TransportError::Network(cause) => Error::network(format!(
                "could not reach sourcegraph.com: {cause}"
            )),
            TransportError::Deadline => Error::deadline(),
            TransportError::TooLarge => {
                Error::upstream("sourcegraph.com's answer is over 64 MiB")
            }
        }
    }
}
