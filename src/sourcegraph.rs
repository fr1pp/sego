//! Everything sego asks Sourcegraph, and the HTTP seam it asks through.
//!
//! A document never selects a field that needs a token, such as
//! `Repository.externalServices`, `recordedCommands` or `codeIntelSummary`,
//! or `Usage.dataSource`: asked for anonymously, one nulls the whole
//! repository.

mod commits;
mod files;
mod graphql;
mod http;
mod raw;
mod refs;
mod resolve;
mod search;

use std::ffi::OsStr;
use std::str::FromStr;
use std::time::{Duration, Instant};

use crate::address::{Repo, Target};
use crate::cache::{Cache, Entry};
use crate::error::{Error, Kind};
use graphql::{Envelope, Request};

pub use commits::{COMMIT_PAGE, Change, CommitSummary, Diff, History, Tag};
pub use files::{Content, File, Submodule, Window};
pub use graphql::regexp_reason;
pub use http::Ureq;
pub use refs::{RefList, Refs};
pub use resolve::Resolved;
pub use search::{
    Alert, CommitResult, FileMatch, Item, NOT_INDEXED, Range, Results, Search, Symbol,
};

/// What a history covers: a revision, or the default branch for `None`; or
/// a range, its head's commits that aren't its base's.
#[derive(Clone, Copy)]
pub enum Revisions<'a> {
    One(Option<&'a str>),
    Range { base: &'a str, head: &'a str },
}

/// Part of a list: the items read, the whole list's length when the server
/// gives it, and whether there is more than the items.
#[derive(Debug, PartialEq, Eq)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub total: Option<u64>,
    pub more: bool,
}

/// What a command asks Sourcegraph through: the transport to `endpoint`, the
/// cache of raw reads, and the client deadline, which no request of the
/// invocation outlasts.
pub struct Client<'a> {
    transport: Box<dyn Transport>,
    endpoint: Endpoint,
    cache: &'a Cache,
    deadline: Instant,
}

/// Each method takes the client by value: an invocation asks one question.
impl<'a> Client<'a> {
    pub fn new(
        transport: Box<dyn Transport>,
        endpoint: Endpoint,
        cache: &'a Cache,
        deadline: Instant,
    ) -> Client<'a> {
        Client { transport, endpoint, cache, deadline }
    }

    /// Sends a request through `send`, unless the client deadline has
    /// passed: every request of an invocation goes through here.
    fn send(
        &self,
        send: impl FnOnce(&dyn Transport) -> Result<Response, TransportError>,
    ) -> Result<Response, Error> {
        let host = self.endpoint.host();
        if Instant::now() >= self.deadline {
            return Err(Error::deadline(&host));
        }
        send(&*self.transport).map_err(|error| error.into_error(&host))
    }

    fn exchange(&self, request: &Request) -> Result<Envelope, Error> {
        let response =
            self.send(|transport| transport.post(GRAPHQL_PATH, &request.body(), self.deadline))?;
        graphql::classify(response, &self.endpoint.host())
    }

    /// `exchange` with a deadline of its own, `within` from now, for a fact
    /// sego adds that can take long enough to cost the answer.
    fn exchange_within(&self, request: &Request, within: Duration) -> Result<Envelope, Error> {
        let deadline = self.deadline.min(Instant::now() + within);
        let host = self.endpoint.host();
        let response = self
            .send(|transport| transport.post(GRAPHQL_PATH, &request.body(), deadline))
            .map_err(|error| match error.kind() {
                Kind::Deadline => Error::deadline_within(&host, within.as_secs()),
                _ => error,
            })?;
        graphql::classify(response, &host)
    }

    /// The whole file at `route`, a site path from a file's details: `None`
    /// when the route has no file.
    fn raw(&self, route: &str) -> Result<Option<Vec<u8>>, Error> {
        let response = self.send(|transport| transport.get(route, self.deadline))?;
        raw::read(response, &self.endpoint.host())
    }

    /// What `target` names in `repo`: a file with the lines of `window` when
    /// given, or a directory, with every entry beneath it when `recursive`.
    pub fn file(
        self,
        repo: &Repo,
        target: &Target,
        window: Option<Window>,
        recursive: bool,
    ) -> Result<(Resolved, File), Error> {
        let request = files::request(repo, target, window, recursive);
        let files::Read { resolved, mut file, body, raw } =
            files::read(&self.exchange(&request)?, repo, target)?;
        if let (File::Blob(blob), Some(window)) = (&mut file, window) {
            let download = || {
                let route = raw.as_deref().ok_or_else(|| Error::failed("the file's details"))?;
                let entry = Entry {
                    repo: &resolved.repo,
                    commit: &resolved.commit,
                    path: resolved.path.as_deref().unwrap_or_default(),
                };
                let (bytes, unavailable) =
                    self.cache.read_through(entry, blob.byte_size, || {
                        self.raw(route)?.ok_or_else(|| Error::failed("the file content"))
                    })?;
                Ok((bytes, unavailable.map(|reason| reason.to_string())))
            };
            blob.content = files::content(body, window, blob.total_lines, download)?;
        }
        Ok((resolved, file))
    }

    /// The changes `revisions` covers over `paths`: a revision's against
    /// its first parent, with the commit, or a range's head's against its
    /// base, listing at most `first` files or all for `None`. A range is read
    /// by its commits' IDs once both ends resolve.
    pub fn diff(
        self,
        repo: &Repo,
        revisions: Revisions<'_>,
        paths: &[String],
        first: Option<u32>,
    ) -> Result<Diff, Error> {
        let ends = match revisions {
            Revisions::One(rev) => {
                let rev = rev.unwrap_or("HEAD");
                let request = commits::commit_request(repo, rev, paths, first);
                return commits::read_commit(&self.exchange(&request)?, repo, rev, paths);
            }
            Revisions::Range { base, head } => self.ends(repo, base, head)?,
        };
        let request = commits::diff_request(&ends, paths, first);
        commits::read_diff(&self.exchange(&request)?, ends, paths)
    }

    /// The commit `rev` names, its changes against its first parent, listing
    /// at most `first` files or all for `None`, and the tags that hold it.
    /// The tags are asked for once the commit resolves, under
    /// `TAGS_DEADLINE`: on an old commit of a repository with thousands of
    /// tags they can take over a minute.
    pub fn commit(self, repo: &Repo, rev: &str, first: Option<u32>) -> Result<Diff, Error> {
        let request = commits::commit_request(repo, rev, &[], first);
        let mut diff = commits::read_commit(&self.exchange(&request)?, repo, rev, &[])?;
        let request = commits::commit_tags_request(&diff.head);
        let tags = self
            .exchange_within(&request, TAGS_DEADLINE)
            .and_then(|envelope| commits::read_commit_tags(&envelope));
        if let Some(commit) = diff.commit.as_mut() {
            commit.tags = Some(tags);
        }
        Ok(diff)
    }

    /// The commits `revisions` covers touching `path`, across renames when
    /// `follow`: after the first `skip`, the next `first`, or all for `None`.
    /// A range is read by its commits' IDs once both ends resolve. A followed
    /// history is one request, since `after:` can't page one
    /// (docs/research/log-paging.md). If the deadline passes once a page is
    /// read, the history is what was read, `stopped`.
    pub fn history(
        self,
        repo: &Repo,
        revisions: Revisions<'_>,
        path: Option<&str>,
        follow: bool,
        first: Option<u32>,
        skip: u32,
    ) -> Result<History, Error> {
        let wanted = first.map_or(usize::MAX, |first| first as usize);
        let size = page_size(wanted);
        let mut history = match revisions {
            Revisions::One(head) => {
                let request = commits::history_request(repo, head, path, follow, size, skip);
                commits::read_history(&self.exchange(&request)?, repo, head, path, skip)?
            }
            Revisions::Range { base, head } => {
                let ends = self.ends(repo, base, head)?;
                let request = commits::range_history_request(&ends, path, size, skip);
                commits::read_range_history(&self.exchange(&request)?, ends, path, skip)?
            }
        };
        if !follow {
            self.read_on(&mut history, path, wanted)?;
        }
        if history.commits.items.len() > wanted {
            history.commits.items.truncate(wanted);
            history.commits.more = true;
        }
        Ok(history)
    }

    /// Reads the pages after the first until `history` holds `wanted` commits
    /// or has no more, and marks it `stopped` if the deadline comes first.
    fn read_on(
        &self,
        history: &mut History,
        path: Option<&str>,
        wanted: usize,
    ) -> Result<(), Error> {
        while history.commits.more && history.commits.items.len() < wanted {
            let Some(after) = history.cursor.take() else {
                return Ok(());
            };
            let size = page_size(wanted - history.commits.items.len());
            let request = commits::page_request(history, path, size, &after);
            let range = history.base.is_some();
            match self.exchange(&request).and_then(|page| commits::read_page(&page, range)) {
                Ok((page, cursor)) => {
                    history.commits.items.extend(page.items);
                    history.commits.more = page.more;
                    history.cursor = cursor;
                }
                Err(error) if error.kind() == Kind::Deadline => {
                    history.stopped = true;
                    return Ok(());
                }
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }

    /// Both ends of the range `base` and `head` in `repo`, resolved in one
    /// request, so that what follows reads them by their commits' IDs.
    fn ends(&self, repo: &Repo, base: &str, head: &str) -> Result<resolve::Ends, Error> {
        let request = resolve::range_request(repo, base, head);
        resolve::read_range(&self.exchange(&request)?, repo, base, head)
    }

    /// The results of `query`.
    pub fn search(self, query: &str) -> Result<Search, Error> {
        search::read(&self.exchange(&search::request(query))?)
    }

    /// The branches and tags of `repo` that match `text`, at most `first` of
    /// each kind, or as many as the server allows for `None`.
    pub fn refs(self, repo: &Repo, text: Option<&str>, first: Option<u32>) -> Result<Refs, Error> {
        let envelope = self.exchange(&refs::request(repo, text, first))?;
        refs::read(&envelope, repo, text)
    }
}

/// The `first:` that reads `left` commits. `first: 1` always reports a next
/// page, so a single commit is asked for as two.
fn page_size(left: usize) -> u32 {
    u32::try_from(left).map_or(COMMIT_PAGE, |left| left.clamp(2, COMMIT_PAGE))
}

const DEFAULT_ENDPOINT: &str = "https://sourcegraph.com";

pub const DEADLINE: Duration = Duration::from_secs(90);

/// The deadline of the request for the tags that hold a commit.
const TAGS_DEADLINE: Duration = Duration::from_secs(10);

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

/// Why a request got no response.
#[derive(Clone, Debug, PartialEq, Eq)]
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
    /// The cache for files read from this endpoint; `var` is the value of
    /// `SEGO_CACHE_DIR`.
    pub fn cache(&self, var: Option<&OsStr>) -> Cache {
        Cache::new(var, &self.base)
    }

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

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::ffi::OsStr;
    use std::rc::Rc;
    use std::thread::sleep;

    use serde_json::json;

    use super::*;
    use crate::error::{Kind, report};

    /// A transport that answers each request with the next of its responses,
    /// after `delay`, and keeps the paths it was asked and the deadline of
    /// each post.
    #[derive(Clone, Default)]
    struct Script {
        responses: Rc<RefCell<VecDeque<Result<Response, TransportError>>>>,
        asked: Rc<RefCell<Vec<String>>>,
        bodies: Rc<RefCell<Vec<serde_json::Value>>>,
        deadlines: Rc<RefCell<Vec<Instant>>>,
        delay: Duration,
    }

    impl Script {
        fn new(responses: Vec<Result<Response, TransportError>>, delay: Duration) -> Script {
            Script {
                responses: Rc::new(RefCell::new(responses.into())),
                delay,
                ..Script::default()
            }
        }

        fn answer(&self, what: String) -> Result<Response, TransportError> {
            self.asked.borrow_mut().push(what);
            sleep(self.delay);
            self.responses
                .borrow_mut()
                .pop_front()
                .unwrap_or(Err(TransportError::Network("none left")))
        }
    }

    impl Transport for Script {
        fn post(
            &self,
            path: &str,
            body: &[u8],
            deadline: Instant,
        ) -> Result<Response, TransportError> {
            self.deadlines.borrow_mut().push(deadline);
            self.bodies.borrow_mut().extend(serde_json::from_slice(body));
            self.answer(format!("POST {path}"))
        }

        fn get(&self, path: &str, _: Instant) -> Result<Response, TransportError> {
            self.answer(format!("GET {path}"))
        }
    }

    fn json(body: &serde_json::Value) -> Response {
        Response::of(200, Some("application/json"), body.to_string())
    }

    /// The first request of `diff gorilla/mux@a...b`: both ends resolve.
    fn ends() -> serde_json::Value {
        json!({"data": {"r0": {
            "name": "github.com/gorilla/mux",
            "defaultBranch": {"displayName": "main"},
            "mirrorInfo": null,
            "a": {"oid": "a".repeat(40)},
            "b": {"oid": "b".repeat(40)}
        }}})
    }

    /// The first request of `show gorilla/mux@a`: a root commit.
    fn root_commit() -> serde_json::Value {
        json!({"data": {"r0": {
            "name": "github.com/gorilla/mux",
            "defaultBranch": {"displayName": "main"},
            "mirrorInfo": null,
            "commit": {
                "oid": "a".repeat(40),
                "subject": "Start",
                "message": "Start",
                "author": {"date": "2012-01-01T00:00:00Z", "person": {"displayName": "Ada"}},
                "committer": null,
                "parents": []
            }
        }}})
    }

    fn run<T>(
        script: &Script,
        deadline: Duration,
        ask: impl FnOnce(Client<'_>) -> Result<T, Error>,
    ) -> Result<T, Error> {
        let endpoint = Endpoint::default();
        let cache = endpoint.cache(Some(OsStr::new("off")));
        let client =
            Client::new(Box::new(script.clone()), endpoint, &cache, Instant::now() + deadline);
        ask(client)
    }

    fn range(client: Client<'_>) -> Result<Diff, Error> {
        client.diff(
            &Repo::Exact("github.com/gorilla/mux".to_owned()),
            Revisions::Range { base: "a", head: "b" },
            &[],
            None,
        )
    }

    /// A text file at `big.c` with `content` as its window, or over the content
    /// limit for `None`.
    fn a_file(content: Option<&str>) -> serde_json::Value {
        let body = content.map(|text| json!({"__typename": "GitBlob", "content": text}));
        let mut response = json!({"data": {"r0": {
            "name": "github.com/gorilla/mux",
            "defaultBranch": {"displayName": "main"},
            "mirrorInfo": null,
            "commit": {
                "oid": "a".repeat(40),
                "meta": {"__typename": "GitBlob", "path": "big.c", "byteSize": 3, "totalLines": 1,
                         "binary": false, "rawURL": "/r/big.c", "lfs": null, "submodule": null},
                "body": body,
                "gm": null
            }
        }}});
        if content.is_none() {
            response["errors"] = json!([{"message": "file exceeded size limit",
                                         "path": ["r0", "commit", "body", "content"]}]);
        }
        response
    }

    fn show(client: Client<'_>) -> Result<(Resolved, File), Error> {
        let target = Target { rev: None, path: Some("big.c".to_owned()) };
        let window = Some(Window { start: 1, end: None });
        client.file(&Repo::Exact("github.com/gorilla/mux".to_owned()), &target, window, false)
    }

    #[test]
    fn a_raw_read_that_fails_fails_a_file_over_the_limit_and_degrades_exact_bytes() {
        let refused = || Err(TransportError::Network("connection reset"));
        let over = Script::new(vec![Ok(json(&a_file(None))), refused()], Duration::ZERO);
        assert_eq!(run(&over, DEADLINE, show).map_err(|e| e.kind()).err(), Some(Kind::Network));
        let replaced =
            Script::new(vec![Ok(json(&a_file(Some("a\u{fffd}b")))), refused()], Duration::ZERO);
        let (_, file) = run(&replaced, DEADLINE, show).unwrap_or_else(|e| panic!("{e}"));
        let File::Blob(blob) = file else { panic!("not a blob") };
        let content = blob.content.unwrap_or_else(|| panic!("no content"));
        assert_eq!(content.utf8, Err(Error::failed("the raw read")));
        assert_eq!(content.lines[0].bytes, "a\u{fffd}b".as_bytes());
        assert_eq!(replaced.asked.borrow()[1], "GET /r/big.c");
    }

    #[test]
    fn a_failure_at_either_request_of_a_range_fails_it() {
        for (failure, kind) in [
            (TransportError::Network("connection refused"), Kind::Network),
            (TransportError::Deadline, Kind::Deadline),
            (TransportError::TooLarge, Kind::Upstream),
        ] {
            let first = Script::new(vec![Err(failure.clone())], Duration::ZERO);
            assert_eq!(run(&first, DEADLINE, range).map_err(|e| e.kind()).err(), Some(kind));
            let second = Script::new(vec![Ok(json(&ends())), Err(failure)], Duration::ZERO);
            assert_eq!(run(&second, DEADLINE, range).map_err(|e| e.kind()).err(), Some(kind));
            assert_eq!(second.asked.borrow().len(), 2);
        }
    }

    #[test]
    fn tags_past_their_own_deadline_leave_the_commit_answered() {
        let script = Script::new(
            vec![Ok(json(&root_commit())), Err(TransportError::Deadline)],
            Duration::ZERO,
        );
        let repo = Repo::Exact("github.com/gorilla/mux".to_owned());
        let diff = run(&script, DEADLINE, |client| client.commit(&repo, "a", None))
            .unwrap_or_else(|e| panic!("{e}"));
        let tags = diff.commit.and_then(|commit| commit.tags);
        assert_eq!(tags, Some(Err(Error::deadline_within("sourcegraph.com", 10))));
        let deadlines = script.deadlines.borrow();
        let gap = deadlines[0].saturating_duration_since(deadlines[1]);
        assert!(gap + TAGS_DEADLINE + Duration::from_secs(1) > DEADLINE, "{gap:?}");
    }

    /// A page of a log: one commit, with `cursor` when more follow.
    fn page_of_a_log(cursor: Option<&str>) -> serde_json::Value {
        json!({
            "nodes": [{
                "oid": "c".repeat(40),
                "subject": "Fix",
                "author": {"date": "2026-01-01T00:00:00Z", "person": {"displayName": "Ada"}},
                "parents": [],
            }],
            "pageInfo": {"hasNextPage": cursor.is_some(), "endCursor": cursor},
        })
    }

    #[test]
    fn a_later_page_sends_the_servers_cursor_as_it_came_at_the_commits_full_id() {
        let first = json!({"data": {
            "r0": {
                "name": "github.com/gorilla/mux",
                "defaultBranch": {"displayName": "main"},
                "mirrorInfo": null,
                "commit": {
                    "oid": "a".repeat(40),
                    "w": {"commit": {"ancestors": page_of_a_log(Some("not a number"))}},
                },
            },
            "t0": {"gitRefs": {"nodes": []}},
        }});
        let later = json!({"data": {"repository": {"commit": {
            "ancestors": page_of_a_log(None),
        }}}});
        let script = Script::new(vec![Ok(json(&first)), Ok(json(&later))], Duration::ZERO);
        let repo = Repo::Exact("github.com/gorilla/mux".to_owned());
        let history = run(&script, DEADLINE, |client| {
            client.history(&repo, Revisions::One(Some("v1")), None, false, Some(5), 0)
        })
        .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(history.commits.items.len(), 2);
        let bodies = script.bodies.borrow();
        assert_eq!(bodies[0]["variables"]["first"], 5);
        assert_eq!(bodies[1]["operationName"], "LogPage");
        assert_eq!(bodies[1]["variables"]["after"], "not a number");
        assert_eq!(bodies[1]["variables"]["commit"], "a".repeat(40));
        assert_eq!(bodies[1]["variables"]["first"], 4);
    }

    /// What paging a commit list relies on and Sourcegraph doesn't promise
    /// (docs/research/log-paging.md). Each also sends a document no recording
    /// can reach, since a recorded log is shorter than a page.
    mod live_rules {
        use std::time::Instant;

        use super::super::*;

        fn asked<T>(ask: impl FnOnce(Client<'_>) -> Result<T, Error>) -> T {
            let endpoint = Endpoint::default();
            let cache = endpoint.cache(Some(OsStr::new("off")));
            let deadline = Instant::now() + DEADLINE;
            let client =
                Client::new(crate::connect(Endpoint::default()), endpoint, &cache, deadline);
            ask(client).unwrap_or_else(|e| panic!("{e}"))
        }

        fn mux() -> Repo {
            Repo::Exact("github.com/gorilla/mux".to_owned())
        }

        fn ids(commits: &[CommitSummary]) -> Vec<&str> {
            commits.iter().map(|commit| commit.oid.as_str()).collect()
        }

        fn history(repo: &Repo, rev: &str, first: u32, skip: u32) -> History {
            let rev = Some(rev);
            asked(|client| {
                let request = commits::history_request(repo, rev, None, false, first, skip);
                commits::read_history(&client.exchange(&request)?, repo, rev, None, skip)
            })
        }

        fn range(base: &str, head: &str, first: u32) -> History {
            let repo = mux();
            asked(|client| {
                let ends = client.ends(&repo, base, head)?;
                let request = commits::range_history_request(&ends, None, first, 0);
                commits::read_range_history(&client.exchange(&request)?, ends, None, 0)
            })
        }

        fn page(history: &History, first: u32, after: &str) -> Vec<CommitSummary> {
            let range = history.base.is_some();
            asked(|client| {
                let request = commits::page_request(history, None, first, after);
                commits::read_page(&client.exchange(&request)?, range)
            })
            .0
            .items
        }

        #[test]
        #[ignore = "sends requests to sourcegraph.com; run with `just live`"]
        fn a_cursor_is_an_offset_and_a_page_continues_a_revision() {
            let whole = history(&mux(), "v1.8.0", 6, 0);
            let first = history(&mux(), "v1.8.0", 3, 0);
            assert_eq!(first.cursor.as_deref(), Some("3"));
            let later = page(&first, 3, "3");
            let skipped = history(&mux(), "v1.8.0", 3, 3);
            assert_eq!(ids(&later), ids(&whole.commits.items[3..]));
            assert_eq!(ids(&skipped.commits.items), ids(&whole.commits.items[3..]));
        }

        #[test]
        #[ignore = "sends requests to sourcegraph.com; run with `just live`"]
        fn a_cursor_is_an_offset_and_a_page_continues_a_range() {
            let whole = range("v1.6.2", "v1.8.0", 6);
            let first = range("v1.6.2", "v1.8.0", 3);
            assert_eq!(first.cursor.as_deref(), Some("3"));
            let later = page(&first, 3, "3");
            assert_eq!(ids(&later), ids(&whole.commits.items[3..]));
        }

        #[test]
        #[ignore = "sends requests to sourcegraph.com; run with `just live`"]
        fn a_page_passes_the_cost_check_in_every_document_that_asks_for_one() {
            let repo = Repo::Exact("github.com/kubernetes/kubernetes".to_owned());
            let first = Some(COMMIT_PAGE);
            let size = usize::try_from(COMMIT_PAGE).unwrap_or(usize::MAX);
            let one = Revisions::One(Some("v1.28.0"));
            let listed = asked(|client| client.history(&repo, one, None, false, first, 0));
            assert_eq!(listed.commits.items.len(), size);
            let span = Revisions::Range { base: "v1.20.0", head: "v1.28.0" };
            let listed = asked(|client| client.history(&repo, span, None, false, first, 0));
            assert_eq!(listed.commits.items.len(), size);
        }

        #[test]
        #[ignore = "sends requests to sourcegraph.com; run with `just live`"]
        fn a_page_of_two_says_truly_where_a_history_ends() {
            let last = history(&mux(), "v1.8.0", 2, 286);
            assert_eq!(last.commits.items.len(), 1);
            assert!(!last.commits.more);
            assert_eq!(last.cursor, None);
        }
    }

    #[test]
    fn nothing_is_sent_once_the_client_deadline_has_passed() {
        let later = Duration::from_millis(50);
        let script = Script::new(vec![Ok(json(&ends())), Ok(json(&ends()))], later * 2);
        let error = run(&script, later, range).err().map(|e| report(&e));
        assert_eq!(
            error.as_deref(),
            Some("sego: error: no answer from sourcegraph.com within 90 s\n")
        );
        assert_eq!(script.asked.borrow().len(), 1);
    }
}
