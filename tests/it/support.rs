//! The case runner: each case in `tests/cases` runs in-process against a
//! replay of its own exchanges and is checked against what it expects and
//! against the rules every case keeps. `SEGO_BLESS=1` rewrites what a case
//! expects, and the list of cases in `cases.rs`.

pub mod case;
mod hostile;
pub mod live;
pub mod record;
pub mod replay;

use std::cell::{Cell, RefCell};
use std::ffi::{OsStr, OsString};
use std::fmt::Write as _;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use similar::TextDiff;
use tempfile::TempDir;

use sego::{Endpoint, Env, Response, Transport, TransportError, run as sego_run};

use case::{Case, Request, Stream};
use replay::{Replay, Sent};

/// The client deadline of ADR 0004.
const DEADLINE: Duration = Duration::from_secs(90);

pub fn cases_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/cases")
}

/// Where the cases' queries are stored, each under its name.
pub fn documents_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/documents")
}

/// Every case file under `tests/cases`, in order.
pub fn case_paths() -> Result<Vec<PathBuf>, String> {
    let mut paths = Vec::new();
    for dir in sorted(&cases_dir())?.into_iter().filter(|p| p.is_dir()) {
        paths.extend(
            sorted(&dir)?.into_iter().filter(|p| p.extension() == Some(OsStr::new("toml"))),
        );
    }
    Ok(paths)
}

fn sorted(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut paths: Vec<PathBuf> = fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .map(|entry| entry.map(|e| e.path()).map_err(|e| e.to_string()))
        .collect::<Result<_, _>>()?;
    paths.sort();
    Ok(paths)
}

pub fn blessing() -> bool {
    std::env::var("SEGO_BLESS").is_ok_and(|v| !v.is_empty() && v != "0")
}

/// Runs the case at `name` under `tests/cases` and panics with every way it
/// fails; with `SEGO_BLESS`, rewrites what it expects instead.
pub fn case(name: &str) {
    let failures = check_case(&cases_dir().join(name)).unwrap_or_else(|e| vec![e]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

fn check_case(path: &Path) -> Result<Vec<String>, String> {
    let mut case = Case::load(path, &documents_dir())?;
    let outcome = run(&case, &Replay::new(&case))?;
    let mut failures = every_case(&case, &outcome);
    failures.extend(exchanges_used(&case, &outcome));
    if blessing() {
        let before = case.to_text();
        let sent: Vec<Request> = outcome.sent.iter().map(|s| s.request.clone()).collect();
        match case.bless(outcome.code, &outcome.stdout, &outcome.stderr, &sent) {
            Ok(()) if case.to_text() != before => {
                fs::write(path, case.to_text()).map_err(|e| format!("{}: {e}", path.display()))?;
            }
            Ok(()) => {}
            Err(failure) => failures.push(failure),
        }
    } else {
        failures.extend(expectations(&case, &outcome));
        failures.extend(requests_match(&case, &outcome));
    }
    failures.extend(hostile::check(&case, &outcome));
    failures.extend(evidence(&case, &cases_dir()));
    Ok(failures)
}

/// What `evidence` may start with besides a recorded case.
const EVIDENCE_FORMS: [&str; 2] = ["GqlError::kind table: ", "none: "];

/// Each hand-written response with a located error names its evidence: a
/// recorded case under `cases` whose response has a located error, an entry
/// of `GqlError::kind`'s table, or `none: ` and the reason there is none.
fn evidence(case: &Case, cases: &Path) -> Vec<String> {
    let mut failures = Vec::new();
    for (i, exchange) in case.exchanges.iter().enumerate() {
        if exchange.recorded.is_some() || !located_error(exchange) {
            continue;
        }
        let at = format!("{}: exchange {}", case.path.display(), i + 1);
        let Some(evidence) = &exchange.evidence else {
            failures.push(format!("{at} has a hand-written located error and no evidence"));
            continue;
        };
        if EVIDENCE_FORMS.iter().any(|form| evidence.starts_with(form)) {
            continue;
        }
        let cited = Case::load(&cases.join(format!("{evidence}.toml")), &documents_dir());
        let recorded = cited.is_ok_and(|cited| {
            cited.exchanges.iter().any(|e| e.recorded.is_some() && located_error(e))
        });
        if !recorded {
            failures.push(format!(
                "{at} cites {evidence}, which is no case with a recorded located error"
            ));
        }
    }
    failures
}

/// Whether the exchange's response holds a GraphQL error with a path.
fn located_error(exchange: &case::Exchange) -> bool {
    let case::Body::Text(text) = &exchange.body else { return false };
    let Ok(body) = serde_json::from_str::<serde_json::Value>(text) else { return false };
    body["errors"].as_array().is_some_and(|errors| {
        errors.iter().any(|e| e["path"].as_array().is_some_and(|path| !path.is_empty()))
    })
}

/// The documents no case names: each is deleted with `SEGO_BLESS`, unless
/// written after this check started, since a case blessed meanwhile may name
/// it.
pub fn unnamed_documents() -> Result<Vec<String>, String> {
    let started = std::time::SystemTime::now();
    let mut named = std::collections::BTreeSet::new();
    for path in case_paths()? {
        let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let document: toml_edit::DocumentMut =
            text.parse().map_err(|e| format!("{}: {e}", path.display()))?;
        for table in
            document.get("exchange").and_then(|i| i.as_array_of_tables()).iter().copied().flatten()
        {
            if let Some(name) = table.get("document").and_then(|i| i.as_str()) {
                named.insert(format!("{name}.graphql"));
            }
        }
    }
    let mut unnamed = Vec::new();
    for path in sorted(&documents_dir())? {
        let file = path.file_name().and_then(OsStr::to_str).unwrap_or_default().to_owned();
        if named.contains(&file) {
            continue;
        }
        let modified = fs::metadata(&path).and_then(|m| m.modified());
        if blessing() && modified.is_ok_and(|at| at < started) {
            fs::remove_file(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        } else {
            unnamed.push(file);
        }
    }
    Ok(unnamed)
}

/// What one invocation of `run` wrote and returned.
pub struct Ran {
    pub code: u8,
    pub stdout: String,
    pub stderr: String,
}

/// Runs `sego` with `args` and `cache_dir` as `SEGO_CACHE_DIR`, connecting
/// through `transport`.
pub fn sego(args: &[&str], cache_dir: &OsStr, transport: &replay::Answers) -> Ran {
    let env = Env { endpoint: None, cache_dir: Some(cache_dir.to_owned()) };
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
        code,
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
/// empty cache directory of its own unless the case sets `SEGO_CACHE_DIR`.
pub fn run<T: Transport + Logged + Clone + 'static>(
    case: &Case,
    transport: &T,
) -> Result<Outcome, String> {
    let cache = TempDir::new().map_err(|e| format!("cache directory: {e}"))?;
    let mut endpoint = None;
    let mut cache_dir = None;
    for (name, value) in &case.env {
        match name.as_str() {
            "SEGO_ENDPOINT" => endpoint = Some(OsString::from(value)),
            "SEGO_CACHE_DIR" => cache_dir = Some(OsString::from(value)),
            _ => return Err(format!("env: {name} is not read by run")),
        }
    }
    let own_cache = cache_dir.is_none();
    let env = Env {
        endpoint,
        cache_dir: Some(cache_dir.unwrap_or_else(|| cache.path().as_os_str().to_owned())),
    };
    let connected = RefCell::new(Vec::new());
    let connect = |endpoint: Endpoint| -> Box<dyn Transport> {
        connected.borrow_mut().push(endpoint.url(""));
        Box::new(transport.clone())
    };
    let args = case.args.iter().map(OsString::from).collect();
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let (mut closed_out, mut closed_err) = (Closed, Closed);
    let out: &mut dyn Write =
        if case.stdout == Stream::Closed { &mut closed_out } else { &mut stdout };
    let err: &mut dyn Write =
        if case.stderr == Stream::Closed { &mut closed_err } else { &mut stderr };
    let started = Instant::now();
    let code = sego_run(args, &env, &connect, out, err);
    let ended = Instant::now();
    let cache_entries = own_cache
        .then(|| fs::read_dir(cache.path()).map(Iterator::count))
        .transpose()
        .map_err(|e| format!("cache directory: {e}"))?;
    Ok(Outcome {
        code: i32::from(code),
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
        sent: transport.sent(),
        problems: transport.problems(),
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
}

/// The checks every case gets, whatever it expects.
fn every_case(case: &Case, outcome: &Outcome) -> Vec<String> {
    let mut failures = Vec::new();
    let from_a_response = |line: &str| FOUND_FROM_A_RESPONSE.iter().any(|m| line.contains(m));
    let first = outcome.stderr.lines().find(|line| line.starts_with("sego: error: "));
    if outcome.code == 2 && !outcome.sent.is_empty() && !first.is_some_and(from_a_response) {
        failures
            .push(format!("{} request(s) reached the server on a usage error", outcome.sent.len()));
    }
    failures.extend(outcome.stderr.lines().filter_map(ends_in_a_period));
    failures.extend(outcome.problems.iter().cloned());
    failures.extend(deadline(outcome));
    failures.extend(endpoint(case, outcome));
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

/// Every request carries the one client deadline, set while `run` ran.
fn deadline(outcome: &Outcome) -> Option<String> {
    let first = outcome.sent.first()?.deadline;
    if outcome.sent.iter().any(|s| s.deadline != first) {
        return Some("the requests carry different deadlines".to_owned());
    }
    let window = outcome.started + DEADLINE..=outcome.ended + DEADLINE;
    (!window.contains(&first)).then(|| {
        format!("the deadline is not {} s after the invocation started", DEADLINE.as_secs())
    })
}

/// `connect` is called at most once, with the case's `SEGO_ENDPOINT` or the
/// default.
fn endpoint(case: &Case, outcome: &Outcome) -> Option<String> {
    let value = case.env.iter().find(|(name, _)| name == "SEGO_ENDPOINT").map(|(_, v)| v);
    let expected = match value.filter(|value| !value.is_empty()) {
        Some(value) => value.parse::<Endpoint>().map(|e| e.url("")),
        None => Ok(Endpoint::default().url("")),
    };
    match (&outcome.connected[..], expected) {
        ([], _) => None,
        ([got], Ok(expected)) if *got == expected => None,
        ([got], Ok(expected)) => Some(format!("connect was given {got}, not {expected}")),
        ([got], Err(_)) => Some(format!("connect was given {got} for an invalid SEGO_ENDPOINT")),
        (all, _) => Some(format!("connect was called {} times", all.len())),
    }
}

/// A request with no exchange left, and exchanges that went unused.
fn exchanges_used(case: &Case, outcome: &Outcome) -> Vec<String> {
    let mut failures = Vec::new();
    for (i, sent) in outcome.sent.iter().enumerate().skip(case.exchanges.len()) {
        failures.push(format!(
            "{}: sego sent request {} with no exchange left to answer it\n{}",
            case.path.display(),
            i + 1,
            sent.request.describe()
        ));
    }
    let unused = case.exchanges.len().saturating_sub(outcome.sent.len());
    if unused > 0 {
        failures.push(format!(
            "{}: {unused} of {} exchanges went unused; sego sent {} request(s)",
            case.path.display(),
            case.exchanges.len(),
            outcome.sent.len()
        ));
    }
    failures
}

/// Each request sent against the exchange written for it, both shown when
/// they differ.
fn requests_match(case: &Case, outcome: &Outcome) -> Vec<String> {
    let mut failures = Vec::new();
    for (i, (sent, exchange)) in outcome.sent.iter().zip(&case.exchanges).enumerate() {
        let written = exchange.request.as_ref();
        if written.is_some_and(|written| written.same_as(&sent.request)) {
            continue;
        }
        let mut message = format!(
            "{}: request {} differs from the exchange written for it\n--- written\n{}\n--- \
             sent\n{}",
            case.path.display(),
            i + 1,
            written.map_or_else(|| "(none)".to_owned(), Request::describe),
            sent.request.describe()
        );
        if exchange.recorded.is_some() {
            message.push_str("\nits response is recorded; record it again with `just record`");
        }
        failures.push(message);
    }
    failures
}

/// What differs between the exit code and streams the case expects and the
/// outcome's.
fn expectations(case: &Case, outcome: &Outcome) -> Vec<String> {
    let mut failures = Vec::new();
    if outcome.code != case.code {
        failures.push(format!("exit: expected {}, got {}", case.code, outcome.code));
    }
    for (name, expected, actual) in
        [("stdout", &case.stdout, &outcome.stdout), ("stderr", &case.stderr, &outcome.stderr)]
    {
        if let Stream::Text(expected) = expected
            && expected != actual
        {
            let diff = TextDiff::from_lines(expected, actual)
                .unified_diff()
                .header("expected", "actual")
                .to_string();
            failures.push(format!("{name}:\n{diff}"));
        }
    }
    failures
}

/// The text of `cases.rs`: a module per directory of `tests/cases` and a
/// test per case in it.
pub fn case_list(root: &Path) -> Result<String, String> {
    let mut text = String::from(
        "// Generated from tests/cases by `SEGO_BLESS=1 cargo test --test it`. Do not edit.\n",
    );
    for dir in sorted(root)?.into_iter().filter(|p| p.is_dir()) {
        let module = name(&dir)?;
        let _ = write!(text, "\nmod {module} {{\n    use crate::support::case;\n");
        let mut names = Vec::new();
        for path in sorted(&dir)?.into_iter().filter(|p| p.extension() == Some(OsStr::new("toml")))
        {
            let function = name(&path)?;
            if names.contains(&function) {
                return Err(format!("two cases in {module} are named {function}"));
            }
            let relative = path.strip_prefix(root).map_err(|e| e.to_string())?;
            let _ = write!(
                text,
                "\n    #[test]\n    fn {function}() {{\n        case(\"{}\");\n    }}\n",
                relative.display()
            );
            names.push(function);
        }
        text.push_str("}\n");
    }
    Ok(text)
}

const KEYWORDS: [&str; 52] = [
    "abstract", "as", "async", "await", "become", "box", "break", "const", "continue", "crate",
    "do", "dyn", "else", "enum", "extern", "false", "final", "fn", "for", "gen", "if", "impl",
    "in", "let", "loop", "macro", "match", "mod", "move", "mut", "override", "priv", "pub", "ref",
    "return", "self", "Self", "static", "struct", "super", "trait", "true", "try", "type",
    "typeof", "unsafe", "unsized", "use", "virtual", "where", "while", "yield",
];

/// The Rust name of a case file or directory: its name without the extension,
/// with `-` as `_`, prefixed with `case_` if it would start with a digit or be
/// a keyword.
fn name(path: &Path) -> Result<String, String> {
    let stem = path.file_stem().and_then(OsStr::to_str).ok_or("a case name isn't UTF-8")?;
    let name = stem.replace('-', "_");
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(format!("{} can't name a test", path.display()));
    }
    if name.starts_with(|c: char| c.is_ascii_digit()) || KEYWORDS.contains(&name.as_str()) {
        Ok(format!("case_{name}"))
    } else {
        Ok(name)
    }
}

const PAUSE: Duration = Duration::from_secs(1);

/// The transport to a live server for `just record` and `just live`: one
/// request a second, and none once the server has refused one with HTTP 429
/// or a Cloudflare challenge.
pub struct Paced {
    inner: Box<dyn Transport>,
    last: Cell<Option<Instant>>,
    blocked: Cell<bool>,
}

impl Paced {
    pub fn new(inner: Box<dyn Transport>) -> Paced {
        Paced { inner, last: Cell::new(None), blocked: Cell::new(false) }
    }

    /// Whether the server has refused a request.
    pub fn blocked(&self) -> bool {
        self.blocked.get()
    }

    fn send(
        &self,
        send: impl FnOnce(&dyn Transport) -> Result<Response, TransportError>,
    ) -> Result<Response, TransportError> {
        if self.blocked.get() {
            return Err(TransportError::Network("stopped after the server refused a request"));
        }
        if let Some(at) = self.last.get() {
            thread::sleep(PAUSE.saturating_sub(at.elapsed()));
        }
        self.last.set(Some(Instant::now()));
        let response = send(&*self.inner);
        if let Ok(response) = &response
            && (response.status == 429 || response.cf_mitigated.is_some())
        {
            self.blocked.set(true);
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

    fn case(env: &str) -> Case {
        let text = format!(
            "args = [\"refs\", \"example.com/a/b\"]\n{env}status.code = 0\n\n[[exchange]]\n\
             operation = \"Refs\"\nvariables = '{{\"repo0\":\"example.com/a/b\"}}'\n\
             document = \"{}\"\nstatus = 200\nresponse = \"{{}}\"\n",
            case::scratch("Refs", QUERY)
        );
        Case::parse(&text, Path::new("refs/x.toml"), case::SCRATCH.path())
            .unwrap_or_else(|e| panic!("{e}"))
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
            connected: vec!["https://sourcegraph.com".to_owned()],
            started,
            ended: started,
            cache_entries: Some(0),
        }
    }

    fn checked(outcome: &Outcome) -> Vec<String> {
        every_case(&case(""), outcome)
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

    fn with_response(response: &str, evidence: Option<&str>, recorded: bool) -> Case {
        let mut text = format!(
            "args = [\"x\"]\nstatus.code = 0\n\n[[exchange]]\nstatus = 200\nresponse = '{response}'\n"
        );
        if let Some(evidence) = evidence {
            text = text.replace("response = ", &format!("evidence = \"{evidence}\"\nresponse = "));
        }
        if recorded {
            text = text.replace(
                "response = ",
                "recorded = { date = 2026-10-07, endpoint = \"https://sourcegraph.com\" }\nresponse = ",
            );
        }
        Case::parse(&text, Path::new("show/x.toml"), case::SCRATCH.path())
            .unwrap_or_else(|e| panic!("{e}"))
    }

    #[test]
    fn a_hand_written_located_error_names_its_evidence() {
        let located = r#"{"errors": [{"message": "boom", "path": ["r0", "c0"]}]}"#;
        let pathless = r#"{"errors": [{"message": "boom"}]}"#;
        let cases = cases_dir();
        let failures = evidence(&with_response(located, None, false), &cases);
        assert!(failures[0].contains("no evidence"), "{failures:?}");
        assert_eq!(evidence(&with_response(located, None, true), &cases), [""; 0]);
        assert_eq!(evidence(&with_response(pathless, None, false), &cases), [""; 0]);
        for cited in ["none: why", "GqlError::kind table: git exit 128", "commit/root-commit"] {
            assert_eq!(evidence(&with_response(located, Some(cited), false), &cases), [""; 0]);
        }
        for cited in ["show/no-such-case", "cli/closed-pipe"] {
            let failures = evidence(&with_response(located, Some(cited), false), &cases);
            assert!(failures[0].contains("no case with a recorded located error"), "{failures:?}");
        }
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
    fn every_request_carries_one_deadline_set_while_run_ran() {
        let mut two = outcome(vec![refs("example.com/a/b", QUERY), raw()]);
        assert_eq!(checked(&two), [""; 0]);
        two.sent[1].deadline += Duration::from_millis(1);
        assert_eq!(checked(&two), ["the requests carry different deadlines"]);
        let mut late = outcome(vec![raw()]);
        late.sent[0].deadline += Duration::from_secs(1);
        assert_eq!(checked(&late), ["the deadline is not 90 s after the invocation started"]);
    }

    #[test]
    fn connect_is_given_the_cases_endpoint_or_the_default() {
        let custom = case("env = { SEGO_ENDPOINT = \"http://127.0.0.1:9/\" }\n");
        assert_eq!(
            every_case(&custom, &outcome(Vec::new())),
            ["connect was given https://sourcegraph.com, not http://127.0.0.1:9"]
        );
        let connected = vec!["http://127.0.0.1:9".to_owned()];
        assert_eq!(every_case(&custom, &Outcome { connected, ..outcome(Vec::new()) }), [""; 0]);
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
    fn a_request_that_differs_from_its_exchange_fails_with_both_texts() {
        let changed = "query Refs($repo0: String!) { r0: repository(name: $repo0) { id } }";
        let failures = requests_match(&case(""), &outcome(vec![refs("example.com/a/b", changed)]));
        let [failure] = &failures[..] else { panic!("{failures:?}") };
        assert!(failure.contains("--- written\nRefs") && failure.contains("--- sent\nRefs"));
        let spaced = refs("example.com/a/b", &QUERY.replace('\n', " "));
        assert_eq!(requests_match(&case(""), &outcome(vec![spaced])), [""; 0]);
    }

    #[test]
    fn a_request_with_no_exchange_left_or_an_unused_exchange_fails_the_case() {
        let failures = exchanges_used(&case(""), &outcome(Vec::new()));
        assert_eq!(failures, ["refs/x.toml: 1 of 1 exchanges went unused; sego sent 0 request(s)"]);
        let two = outcome(vec![refs("example.com/a/b", QUERY), raw()]);
        let failures = exchanges_used(&case(""), &two);
        assert!(failures[0].contains("sent request 2 with no exchange left"), "{failures:?}");
    }

    #[test]
    fn a_stream_that_differs_fails_with_a_diff() {
        let failures =
            expectations(&case(""), &Outcome { stdout: "one\n".to_owned(), ..outcome(Vec::new()) });
        assert_eq!(failures, ["stdout:\n--- expected\n+++ actual\n@@ -0,0 +1 @@\n+one\n"]);
    }

    #[test]
    fn a_case_is_named_for_its_file() {
        let named = |path: &str| name(Path::new(path)).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(named("show/raw-read-in-json.toml"), "raw_read_in_json");
        assert_eq!(named("show/404-page.toml"), "case_404_page");
        assert_eq!(named("show/match.toml"), "case_match");
        assert!(name(Path::new("show/a b.toml")).is_err());
    }
}
