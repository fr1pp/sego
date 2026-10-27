//! Everything sego asks Sourcegraph, and the HTTP seam it asks through.

mod commits;
mod files;
mod graphql;
mod http;
mod raw;
mod refs;
mod repository;
mod search;

use std::str::FromStr;
use std::time::{Duration, Instant};

use crate::address::{Repo, Target};
use crate::error::Error;
use graphql::{Envelope, Request};

pub use commits::{Change, Commit, Comparison, History, Log, NEWEST_TAGS, RangeHistory};
pub use files::{Content, Details, File, Resolved, Submodule, Tree};
pub use http::Ureq;
pub use refs::{RefList, Refs};
pub use search::{
    Alert, CommitResult, FileMatch, Item, Listing, Range, Results, Search, Symbol, UnindexedFile,
};

/// What a command asks Sourcegraph through: the transport, and the one
/// client deadline every request of the invocation shares (ADR 0004).
pub struct Client {
    transport: Box<dyn Transport>,
    deadline: Instant,
}

impl Client {
    /// The client deadline starts now.
    pub fn new(transport: Box<dyn Transport>) -> Client {
        Client { transport, deadline: Instant::now() + DEADLINE }
    }

    fn exchange(&self, request: &Request) -> Result<Envelope, Error> {
        graphql::exchange(&*self.transport, request, self.deadline)
    }

    /// The whole file at `route`, a site path from a file's details: `None`
    /// when the route has no file.
    pub fn raw(&self, route: &str) -> Result<Option<Vec<u8>>, Error> {
        let host = self.transport.host();
        let response =
            self.transport.get(route, self.deadline).map_err(|error| error.into_error(&host))?;
        raw::read(response, &host)
    }

    /// The commit `rev` names in `repo`, with the files it changed against its
    /// first parent.
    pub fn commit(&self, repo: &Repo, rev: &str) -> Result<Commit, Error> {
        let envelope = self.exchange(&commits::commit_request(repo, rev))?;
        commits::read_commit(&envelope, repo, rev)
    }

    /// The comparison of `head` with `base`, or with its first parent for
    /// `None`, over `paths`, listing at most `first` files or all for `None`.
    pub fn comparison(
        &self,
        repo: &Repo,
        base: Option<&str>,
        head: &str,
        paths: &[String],
        first: Option<u32>,
    ) -> Result<Comparison, Error> {
        let request = commits::comparison_request(repo, base, head, paths, first);
        commits::read_comparison(&self.exchange(&request)?, repo, base, head, paths)
    }

    /// The history of `rev`, or of `HEAD` for `None`, touching `path`: the
    /// first `first` commits.
    pub fn history(
        &self,
        repo: &Repo,
        rev: Option<&str>,
        path: Option<&str>,
        first: u32,
    ) -> Result<History, Error> {
        let envelope = self.exchange(&commits::history_request(repo, rev, path, first))?;
        commits::read_history(&envelope, repo, rev, path)
    }

    /// The commits in `head` and not in `base` touching `path`: the first
    /// `first`, or all for `None`.
    pub fn history_range(
        &self,
        repo: &Repo,
        (base, head): (&str, &str),
        path: Option<&str>,
        first: Option<u32>,
    ) -> Result<RangeHistory, Error> {
        let envelope = self.exchange(&commits::range_request(repo, (base, head), path, first))?;
        commits::read_range(&envelope, repo, (base, head), path)
    }

    /// The file `target` names in `repo`, with lines `start` to `end`, or to
    /// the last for `None`.
    pub fn file(
        &self,
        repo: &Repo,
        target: &Target,
        start: u32,
        end: Option<u32>,
    ) -> Result<(Resolved, File), Error> {
        let envelope = self.exchange(&files::file_request(repo, target, start, end))?;
        files::read_file(&envelope, repo, target, start)
    }

    /// The directory `target` names in `repo`, with every entry beneath it
    /// when `recursive`.
    pub fn tree(
        &self,
        repo: &Repo,
        target: &Target,
        recursive: bool,
    ) -> Result<(Resolved, Tree), Error> {
        let envelope = self.exchange(&files::tree_request(repo, target, recursive))?;
        files::read_tree(&envelope, repo, target)
    }

    /// The results of `query`, and the listing of the side query
    /// `unindexed` when given.
    pub fn search(&self, query: &str, unindexed: Option<&str>) -> Result<Search, Error> {
        search::read(&self.exchange(&search::request(query, unindexed))?, unindexed.is_some())
    }

    /// The branches and tags of `repo` that match `text`, at most `first` of
    /// each kind, or as many as the server allows for `None`.
    pub fn refs(&self, repo: &Repo, text: Option<&str>, first: Option<u32>) -> Result<Refs, Error> {
        let envelope = self.exchange(&refs::request(repo, text, first))?;
        refs::read(&envelope, repo, text)
    }
}

const DEFAULT_ENDPOINT: &str = "https://sourcegraph.com";

pub const DEADLINE: Duration = Duration::from_secs(90);
pub const GRAPHQL_PATH: &str = "/.api/graphql";

/// What the server answered to one request.
#[derive(Debug)]
pub struct Response {
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

#[cfg(test)]
impl Response {
    /// A response with only the status, content type and body set.
    fn of(status: u16, content_type: Option<&str>, body: impl Into<Vec<u8>>) -> Response {
        Response {
            status,
            content_type: content_type.map(str::to_owned),
            location: None,
            retry_after: None,
            cf_mitigated: None,
            body: body.into(),
        }
    }
}

/// The envelope of a 200 JSON response with `body`.
#[cfg(test)]
fn parsed(body: &serde_json::Value) -> Envelope {
    let response = Response::of(200, Some("application/json"), body.to_string());
    graphql::classify(response, "sourcegraph.com").unwrap_or_else(|e| panic!("{e}"))
}

/// Why a request got no response.
#[derive(Debug, PartialEq, Eq)]
pub enum TransportError {
    /// The connection failed; the text says how, without naming the host.
    Network(&'static str),
    /// The deadline passed before the response arrived.
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
    fn post(&self, path: &str, body: &[u8], deadline: Instant) -> Result<Response, TransportError>;

    /// Gets `path` from the endpoint.
    fn get(&self, path: &str, deadline: Instant) -> Result<Response, TransportError>;
}

/// The Sourcegraph instance `run` reads from.
#[derive(Clone, Debug)]
pub struct Endpoint {
    base: String,
}

/// sourcegraph.com.
impl Default for Endpoint {
    fn default() -> Endpoint {
        Endpoint { base: DEFAULT_ENDPOINT.to_owned() }
    }
}

/// An http:// or https:// URL with a host; the error is the reason it isn't
/// one.
impl FromStr for Endpoint {
    type Err = String;

    fn from_str(text: &str) -> Result<Endpoint, String> {
        if !is_endpoint(text) {
            return Err("must be an http:// or https:// URL with a host and no query or fragment"
                .to_owned());
        }
        Ok(Endpoint { base: text.trim_end_matches('/').to_owned() })
    }
}

impl Endpoint {
    /// The URL of `path` on this endpoint.
    pub fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    /// The base URL's `host[:port]`, without any `user[:pw]@`.
    fn host(&self) -> String {
        let (_, rest) = self.base.split_once("://").unwrap_or(("", &self.base));
        let authority = rest.split('/').next().unwrap_or_default();
        authority.rsplit('@').next().unwrap_or_default().to_owned()
    }
}

fn is_endpoint(text: &str) -> bool {
    let Some((scheme, rest)) = text.split_once("://") else {
        return false;
    };
    if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
        return false;
    }
    if text.contains(['?', '#']) || text.chars().any(char::is_whitespace) {
        return false;
    }
    let authority = rest.split('/').next().unwrap_or_default();
    let host_port = authority.rsplit('@').next().unwrap_or_default();
    let (host, port) = if host_port.starts_with('[') {
        host_port.split_once(']').map_or(("", None), |(host, rest)| (host, rest.strip_prefix(':')))
    } else {
        match host_port.split_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (host_port, None),
        }
    };
    !host.is_empty()
        && host != "["
        && port.is_none_or(|port| !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()))
}

impl TransportError {
    pub fn into_error(self, host: &str) -> Error {
        match self {
            TransportError::Network(cause) => {
                Error::network(format!("could not reach {host}: {cause}"))
            }
            TransportError::Deadline => Error::deadline(host),
            TransportError::TooLarge => Error::upstream(format!("{host}'s answer is over 64 MiB")),
        }
    }
}
