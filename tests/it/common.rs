//! What the tests run sego through: in-process, answered by a test's responses
//! or by the recordings, and held to the rules every test keeps. A test that
//! breaks one panics here, before its snapshot is compared or written.
//! `SEGO_RECORD=1` writes the recordings a test lacks.

pub mod case;
mod escape;
pub mod live;
pub mod record;
pub mod recording;
pub mod replay;

use std::cell::RefCell;
use std::ffi::{OsStr, OsString};
use std::fmt::{self, Write as _};
use std::fs;
use std::io::{self, Write};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;
use tempfile::TempDir;

use sego::{Endpoint, Env, Response, Transport, TransportError, run as sego_run};

use case::Case;
use recording::{Request, ServerResponse, Store};
use replay::{Replay, Sent};

/// The client deadline of ADR 0004.
const DEADLINE: Duration = Duration::from_secs(90);

/// Runs sego on `args`, answered by the recordings.
pub fn sego(args: &[&str]) -> Ran {
    Case::new(args, Vec::new()).ran()
}

/// Runs sego on `args`, answered by `responses` in order.
pub fn answered<R: Into<ServerResponse>>(
    args: &[&str],
    responses: impl IntoIterator<Item = R>,
) -> Ran {
    Case::new(args, responses.into_iter().map(Into::into).collect()).ran()
}

impl Case {
    /// Runs the case and panics if it breaks a rule. With `SEGO_RECORD`, records
    /// what it lacks instead of checking it.
    pub fn ran(&self) -> Ran {
        let outcome = if record::active() && self.responses.is_empty() {
            record_case(self)
        } else {
            checked_run(self)
        };
        outcome.map_or_else(|e| panic!("{e}"), Ran::from)
    }
}

fn record_case(case: &Case) -> Result<Outcome, String> {
    let upstream = Rc::new(Paced::new(sego::connect(Endpoint::default())));
    let store = if record::again() { Store::default().again() } else { Store::default() };
    let outcome = record::case(case, &store, upstream, &record::today())?;
    record::note_used(&outcome)?;
    Ok(outcome)
}

fn checked_run(case: &Case) -> Result<Outcome, String> {
    let outcome = run(case, &Replay::new(case))?;
    let failures = rules(case, &outcome);
    if failures.is_empty() { Ok(outcome) } else { Err(failures.join("\n")) }
}

/// What every test keeps to, whatever it prints.
fn rules(case: &Case, outcome: &Outcome) -> Vec<String> {
    let mut failures = every_case(outcome);
    failures.extend(responses_used(case, outcome));
    failures.extend(escape::check(case, outcome));
    failures
}

/// What sego printed, as a snapshot holds it. A snapshot ignores what ends it
/// and treats `\r\n` as `\n`, so the last line keeps every stream's own
/// whitespace in play, and `every_case` refuses a carriage return. JSON prints
/// as one line, which would differ whole, so it's laid out here with its keys
/// sorted, and `every_case` holds it to one line.
pub struct Ran {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl From<Outcome> for Ran {
    fn from(outcome: Outcome) -> Ran {
        Ran { code: outcome.code, stdout: outcome.stdout, stderr: outcome.stderr }
    }
}

impl fmt::Display for Ran {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let stdout = match json_answer(&self.stdout) {
            Some(answer) => laid_out(&answer)? + "\n",
            None => self.stdout.clone(),
        };
        write!(
            f,
            "exit: {}\n----- stdout -----\n{stdout}----- stderr -----\n{}----- end -----",
            self.code, self.stderr
        )
    }
}

/// `answer` on many lines, with what sego escapes escaped again, since reading
/// the JSON undid that.
fn laid_out(answer: &Value) -> Result<String, fmt::Error> {
    let text = serde_json::to_string_pretty(answer).map_err(|_| fmt::Error)?;
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if c != '\n' && escape::needs_escape(c) {
            for unit in c.encode_utf16(&mut [0; 2]) {
                write!(out, "\\u{unit:04x}").map_err(|_| fmt::Error)?;
            }
        } else {
            out.push(c);
        }
    }
    Ok(out)
}

/// `stdout` as a JSON answer, if it is one.
fn json_answer(stdout: &str) -> Option<Value> {
    serde_json::from_str(stdout).ok().filter(Value::is_object)
}

/// Runs sego on `args` through `transport`, with `cache_dir` as
/// `SEGO_CACHE_DIR`.
pub fn sego_through(args: &[&str], cache_dir: &OsStr, transport: &replay::Answers) -> Ran {
    let env = Env { cache_dir: Some(cache_dir.to_owned()) };
    let handle = transport.clone();
    let connect = move |_: Endpoint| -> Box<dyn Transport> { Box::new(handle.clone()) };
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let code = sego_run(
        args.iter().map(OsString::from).collect(),
        &env,
        &connect,
        &mut stdout,
        &mut stderr,
    );
    Ran {
        code: i32::from(code),
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
    }
}

/// What one in-process run of a case did.
pub struct Outcome {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
    pub sent: Vec<Sent>,
    pub problems: Vec<String>,
    pub served: Vec<ServerResponse>,
    /// The endpoint of each call to `connect`, as its base URL.
    pub connected: Vec<String>,
    pub started: Instant,
    pub ended: Instant,
    /// The entries of the cache directory the run was given, when the case
    /// didn't set its own.
    pub cache_entries: Option<usize>,
}

/// A writer for a pipe whose reader has gone.
struct Closed;

impl Write for Closed {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::ErrorKind::BrokenPipe.into())
    }

    fn flush(&mut self) -> io::Result<()> {
        Err(io::ErrorKind::BrokenPipe.into())
    }
}

/// Runs the case's command line in-process through `transport`, with an
/// empty cache directory of its own unless the case sets one.
pub fn run<T: Transport + Logged + Clone + 'static>(
    case: &Case,
    transport: &T,
) -> Result<Outcome, String> {
    let cache = TempDir::new().map_err(|e| format!("cache directory: {e}"))?;
    let own_cache = case.cache_dir.is_none();
    let cache_dir = case.cache_dir.as_ref().map_or(cache.path().as_os_str().to_owned(), Into::into);
    let env = Env { cache_dir: Some(cache_dir) };
    let connected = RefCell::new(Vec::new());
    let connect = |endpoint: Endpoint| -> Box<dyn Transport> {
        connected.borrow_mut().push(endpoint.url(""));
        Box::new(transport.clone())
    };
    let args = case.args.iter().map(OsString::from).collect();
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let (mut closed_out, mut closed_err) = (Closed, Closed);
    let out: &mut dyn Write = if case.stdout_closed { &mut closed_out } else { &mut stdout };
    let err: &mut dyn Write = if case.stderr_closed { &mut closed_err } else { &mut stderr };
    let started = Instant::now();
    let code = sego_run(args, &env, &connect, out, err);
    let ended = Instant::now();
    let cache_entries = own_cache
        .then(|| fs::read_dir(cache.path()).map(Iterator::count))
        .transpose()
        .map_err(|e| format!("cache directory: {e}"))?;
    Ok(Outcome {
        code: i32::from(code),
        stdout: String::from_utf8(stdout).map_err(|e| format!("stdout: {e}"))?,
        stderr: String::from_utf8(stderr).map_err(|e| format!("stderr: {e}"))?,
        sent: transport.sent(),
        problems: transport.problems(),
        served: transport.served(),
        connected: connected.into_inner(),
        started,
        ended,
        cache_entries,
    })
}

/// What the transport a case runs through logged.
pub trait Logged {
    fn sent(&self) -> Vec<Sent>;
    /// What the transport couldn't make sense of, or refused to do.
    fn problems(&self) -> Vec<String>;
    /// What it answered with, in order.
    fn served(&self) -> Vec<ServerResponse>;
}

/// The checks every case gets, whatever it expects.
fn every_case(outcome: &Outcome) -> Vec<String> {
    let mut failures = Vec::new();
    let from_a_response = |line: &str| FOUND_FROM_A_RESPONSE.iter().any(|m| line.contains(m));
    let first = outcome.stderr.lines().find(|line| line.starts_with("sego: error: "));
    if outcome.code == 2 && !outcome.sent.is_empty() && !first.is_some_and(from_a_response) {
        failures
            .push(format!("{} request(s) reached the server on a usage error", outcome.sent.len()));
    }
    failures.extend(outcome.stderr.lines().filter_map(ends_in_a_period));
    failures.extend(outcome.problems.iter().cloned());
    if outcome.stdout.contains('\r') || outcome.stderr.contains('\r') {
        failures.push("a snapshot can't hold a carriage return".to_owned());
    }
    if json_answer(&outcome.stdout).is_some() && outcome.stdout.lines().count() != 1 {
        failures.push("a JSON answer isn't one line".to_owned());
    }
    failures.extend(deadline(outcome));
    failures.extend(endpoint(outcome));
    let raw = outcome.sent.iter().any(|s| matches!(s.request, Request::Raw { .. }));
    if outcome.cache_entries.is_some_and(|n| n > 0) && !raw {
        failures.push("the cache directory was written with no raw read sent".to_owned());
    }
    failures
}

/// What the three usage errors found from a response say (ADR 0005): LINES
/// for a directory, a regular expression the server can't compile, and
/// `owner/name` on two hosts.
const FOUND_FROM_A_RESPONSE: [&str; 3] =
    ["; LINES apply to a file", "the search's regular expression doesn't compile: ", " exists as "];

/// A note, an error or a fact line ends without a period, as in rustc, cargo
/// and git. A message that starts in uppercase is Sourcegraph's own text,
/// quoted as sent.
fn ends_in_a_period(line: &str) -> Option<String> {
    let text = line
        .strip_prefix("sego: note: ")
        .or_else(|| line.strip_prefix("sego: error: "))
        .unwrap_or(line.trim_start());
    if text.starts_with(|c: char| c.is_ascii_uppercase()) {
        return None;
    }
    (text.ends_with('.') && !text.ends_with("..."))
        .then(|| format!("a note or error line ends with a period: {line}"))
}

/// The first request carries the client deadline, set while `run` ran, and
/// no request carries a later one: a request with a deadline of its own
/// ends before it.
fn deadline(outcome: &Outcome) -> Option<String> {
    let first = outcome.sent.first()?.deadline;
    if outcome.sent.iter().any(|s| s.deadline > first) {
        return Some("a request carries a deadline after the client deadline".to_owned());
    }
    let window = outcome.started + DEADLINE..=outcome.ended + DEADLINE;
    (!window.contains(&first)).then(|| {
        format!("the deadline is not {} s after the invocation started", DEADLINE.as_secs())
    })
}

/// `connect` is called at most once, with sourcegraph.com.
fn endpoint(outcome: &Outcome) -> Option<String> {
    let expected = Endpoint::default().url("");
    match &outcome.connected[..] {
        [] => None,
        [got] if *got == expected => None,
        [got] => Some(format!("connect was given {got}, not {expected}")),
        all => Some(format!("connect was called {} times", all.len())),
    }
}

/// A request past a case's last response, and responses that went unused.
fn responses_used(case: &Case, outcome: &Outcome) -> Vec<String> {
    if case.responses.is_empty() {
        return Vec::new();
    }
    let mut failures = Vec::new();
    for (i, sent) in outcome.sent.iter().enumerate().skip(case.responses.len()) {
        failures.push(format!(
            "{}: sego sent request {} with no response left to answer it\n{}",
            case.describe(),
            i + 1,
            sent.request.describe()
        ));
    }
    let unused = case.responses.len().saturating_sub(outcome.sent.len());
    if unused > 0 {
        failures.push(format!(
            "{}: {unused} of {} responses went unused; sego sent {} request(s)",
            case.describe(),
            case.responses.len(),
            outcome.sent.len()
        ));
    }
    failures
}

const PAUSE: Duration = Duration::from_millis(500);

/// What every request to a live server goes through, whichever test sends it:
/// one at a time, two a second, and none once the server has refused one with
/// HTTP 429 or a Cloudflare challenge.
pub struct Gate {
    last: Mutex<Option<Instant>>,
    blocked: AtomicBool,
    pause: Duration,
}

static GATE: Gate = Gate::new(PAUSE);

const REFUSED: &str = "stopped after the server refused a request";

impl Gate {
    const fn new(pause: Duration) -> Gate {
        Gate { last: Mutex::new(None), blocked: AtomicBool::new(false), pause }
    }
}

/// The transport to a live server for `just record` and `just live`.
pub struct Paced {
    inner: Box<dyn Transport>,
    gate: &'static Gate,
}

impl Paced {
    pub fn new(inner: Box<dyn Transport>) -> Paced {
        Paced { inner, gate: &GATE }
    }

    fn send(
        &self,
        send: impl FnOnce(&dyn Transport) -> Result<Response, TransportError>,
    ) -> Result<Response, TransportError> {
        let refused = TransportError::Network(REFUSED);
        let mut last = self.gate.last.lock().unwrap_or_else(PoisonError::into_inner);
        if self.gate.blocked.load(Ordering::SeqCst) {
            return Err(refused);
        }
        if let Some(at) = *last {
            thread::sleep(self.gate.pause.saturating_sub(at.elapsed()));
        }
        let response = send(&*self.inner);
        *last = Some(Instant::now());
        if let Ok(response) = &response
            && (response.status == 429 || response.cf_mitigated.is_some())
        {
            self.gate.blocked.store(true, Ordering::SeqCst);
        }
        response
    }
}

impl Transport for Paced {
    fn post(&self, path: &str, body: &[u8], deadline: Instant) -> Result<Response, TransportError> {
        self.send(|inner| inner.post(path, body, deadline))
    }

    fn get(&self, path: &str, deadline: Instant) -> Result<Response, TransportError> {
        self.send(|inner| inner.get(path, deadline))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const QUERY: &str =
        "query Refs($repo0: String!) {\n  r0: repository(name: $repo0) { name }\n}\n";

    fn case() -> Case {
        Case::new(&["refs", "example.com/a/b"], vec![json!({}).into()])
    }

    fn refs(repo: &str, query: &str) -> Request {
        Request::Graphql {
            operation: "Refs".to_owned(),
            variables: json!({"repo0": repo}),
            query: query.to_owned(),
        }
    }

    fn raw() -> Request {
        Request::Raw { path: "/r/x/-/raw/f".to_owned() }
    }

    fn outcome(requests: Vec<Request>) -> Outcome {
        let started = Instant::now();
        let deadline = started + DEADLINE;
        Outcome {
            code: 0,
            stdout: String::new(),
            stderr: String::new(),
            sent: requests.into_iter().map(|request| Sent { request, deadline }).collect(),
            problems: Vec::new(),
            served: Vec::new(),
            connected: vec!["https://sourcegraph.com".to_owned()],
            started,
            ended: started,
            cache_entries: Some(0),
        }
    }

    fn checked(outcome: &Outcome) -> Vec<String> {
        every_case(outcome)
    }

    #[test]
    fn a_request_on_a_usage_error_fails_the_case_unless_the_response_found_it() {
        let failures =
            checked(&Outcome { code: 2, ..outcome(vec![refs("example.com/a/b", QUERY)]) });
        assert!(failures[0].contains("reached the server on a usage error"), "{failures:?}");
        assert_eq!(checked(&Outcome { code: 2, ..outcome(Vec::new()) }), [""; 0]);
        let found = Outcome {
            code: 2,
            stderr: "sego: error: gorilla/mux exists as github.com/gorilla/mux and \
                     gitlab.com/gorilla/mux\n"
                .to_owned(),
            ..outcome(vec![refs("example.com/a/b", QUERY)])
        };
        assert_eq!(checked(&found), [""; 0]);
    }

    #[test]
    fn a_note_or_error_line_ending_in_a_period_fails_the_case() {
        let noted =
            |stderr: &str| checked(&Outcome { stderr: stderr.to_owned(), ..outcome(Vec::new()) });
        for stderr in [
            "sego: note: tags marked from the 1000 newest.\n",
            "sego: error: no such file.\n",
            "  repo:, lang: or file: narrows what the search reads.\n",
        ] {
            let failures = noted(stderr);
            assert!(
                failures.len() == 1 && failures[0].contains("ends with a period"),
                "{failures:?}"
            );
        }
        assert_eq!(
            noted("sego: note: the rest follows...\nsego: error: Sourcegraph says no.\n"),
            [""; 0]
        );
    }

    #[test]
    fn no_request_outlasts_the_client_deadline_set_while_run_ran() {
        let mut two = outcome(vec![refs("example.com/a/b", QUERY), raw()]);
        assert_eq!(checked(&two), [""; 0]);
        two.sent[1].deadline -= Duration::from_secs(80);
        assert_eq!(checked(&two), [""; 0]);
        two.sent[1].deadline += Duration::from_secs(80) + Duration::from_millis(1);
        assert_eq!(checked(&two), ["a request carries a deadline after the client deadline"]);
        let mut late = outcome(vec![raw()]);
        late.sent[0].deadline += Duration::from_secs(1);
        assert_eq!(checked(&late), ["the deadline is not 90 s after the invocation started"]);
    }

    #[test]
    fn connect_is_given_sourcegraph_com_once() {
        let other = vec!["http://127.0.0.1:9".to_owned()];
        assert_eq!(
            checked(&Outcome { connected: other, ..outcome(Vec::new()) }),
            ["connect was given http://127.0.0.1:9, not https://sourcegraph.com"]
        );
        let twice = vec!["https://sourcegraph.com".to_owned(); 2];
        assert_eq!(
            checked(&Outcome { connected: twice, ..outcome(Vec::new()) }),
            ["connect was called 2 times"]
        );
    }

    #[test]
    fn the_cache_directory_is_left_alone_unless_a_raw_read_was_sent() {
        let written = Outcome { cache_entries: Some(2), ..outcome(vec![refs("a", QUERY)]) };
        assert_eq!(checked(&written), ["the cache directory was written with no raw read sent"]);
        assert_eq!(checked(&Outcome { cache_entries: Some(2), ..outcome(vec![raw()]) }), [""; 0]);
        assert_eq!(checked(&Outcome { cache_entries: None, ..outcome(Vec::new()) }), [""; 0]);
    }

    #[test]
    fn a_request_past_the_last_response_or_an_unused_response_fails_the_case() {
        let failures = responses_used(&case(), &outcome(Vec::new()));
        assert_eq!(
            failures,
            ["refs example.com/a/b: 1 of 1 responses went unused; sego sent 0 request(s)"]
        );
        let two = outcome(vec![refs("example.com/a/b", QUERY), raw()]);
        let failures = responses_used(&case(), &two);
        assert!(failures[0].contains("sent request 2 with no response left"), "{failures:?}");
    }

    #[test]
    fn a_refused_request_stops_every_transport_through_the_gate() {
        let gate: &'static Gate = Box::leak(Box::new(Gate::new(Duration::ZERO)));
        let refused = Response { ..replay::response_of(429, None, "") };
        let answers = replay::Answers::new(vec![Ok(refused)]);
        let first = Paced { inner: Box::new(answers.clone()), gate };
        let second = Paced { inner: Box::new(answers.clone()), gate };
        let far = Instant::now() + DEADLINE;
        assert!(first.post("/", b"", far).is_ok());
        assert!(second.post("/", b"", far).is_err());
        assert_eq!(answers.methods(), ["POST"]);
    }

    #[test]
    fn a_carriage_return_fails_the_case() {
        let out = Outcome { stdout: "a\r\n".to_owned(), ..outcome(Vec::new()) };
        assert_eq!(checked(&out), ["a snapshot can't hold a carriage return"]);
    }

    #[test]
    fn a_json_answer_must_be_one_line() {
        let spread = Outcome { stdout: "{\n}\n".to_owned(), ..outcome(Vec::new()) };
        assert_eq!(checked(&spread), ["a JSON answer isn't one line"]);
        let one = Outcome { stdout: "{}\n".to_owned(), ..outcome(Vec::new()) };
        assert_eq!(checked(&one), [""; 0]);
    }

    fn rendered(stdout: &str, stderr: &str) -> String {
        Ran { code: 1, stdout: stdout.to_owned(), stderr: stderr.to_owned() }.to_string()
    }

    #[test]
    fn what_sego_printed_is_held_between_lines_of_its_own() {
        assert_eq!(
            rendered("out  \n\n", "err\n"),
            "exit: 1\n----- stdout -----\nout  \n\n----- stderr -----\nerr\n----- end -----"
        );
    }

    #[test]
    fn a_stream_with_no_final_newline_runs_into_the_next_line() {
        assert!(rendered("out", "").contains("out----- stderr -----"));
    }

    #[test]
    fn a_json_answer_keeps_what_sego_escaped_escaped() {
        let text = rendered("{\"a\":\"b\\u202ec\\u200bd\\ud83d\\ude00\"}\n", "");
        assert!(text.contains(r#""a": "b\u202ec\u200bd"#), "{text}");
        assert!(!text.chars().any(|c| c != '\n' && escape::needs_escape(c)), "{text}");
    }

    #[test]
    fn a_json_answer_is_laid_out_and_other_text_is_not() {
        assert!(rendered("{\"a\":1}\n", "").contains("{\n  \"a\": 1\n}\n"));
        assert!(rendered("1\n2\n", "").contains("-----\n1\n2\n-----"));
    }
}
