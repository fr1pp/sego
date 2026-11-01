use std::cell::RefCell;
use std::fmt::Write as _;
use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde_json::Value;

use sego::{Response, Transport, TransportError};

use super::case::Case;
use super::recording::{Request, ServerResponse, Store};
use super::replay::{Sent, graphql, response_of};
use super::{Logged, Outcome, run};

/// Whether this run records instead of testing: `SEGO_RECORD=1` for what the
/// tests lack, `again` to replace what they have, and `prune` for the test that
/// removes the recordings the recording run didn't use.
pub fn active() -> bool {
    matches!(mode().as_deref(), Some("1" | "again"))
}

/// Whether this run replaces the recordings its cases already have.
pub fn again() -> bool {
    mode().as_deref() == Some("again")
}

pub fn pruning() -> bool {
    mode().as_deref() == Some("prune")
}

fn mode() -> Option<String> {
    std::env::var("SEGO_RECORD").ok().filter(|mode| !mode.is_empty() && mode != "0")
}

/// Today's UTC date, as a recording's.
pub fn today() -> String {
    date(SystemTime::now())
}

/// Runs a case the recordings answer, forwarding to `upstream` each request
/// that has none, and writes what was recorded. Nothing is written if any
/// request was refused, or if a search it recorded stopped at its deadline,
/// since a cut search varies from run to run.
pub fn case(
    case: &Case,
    store: &Store,
    upstream: Rc<dyn Transport>,
    date: &str,
) -> Result<Outcome, String> {
    let recorder = Recorder::new(case, store, upstream);
    let outcome = run(case, &recorder)?;
    let state = recorder.state.borrow();
    if !state.problems.is_empty() {
        return Err(state.problems.join("\n"));
    }
    let searched = state.recorded.iter().any(
        |(request, _)| matches!(request, Request::Graphql { operation, .. } if operation == "Search"),
    );
    if searched && stopped(&outcome.stdout, &outcome.stderr) {
        return Err(format!(
            "{}: recording failed: the search stopped at its deadline; run `just record` again",
            case.describe()
        ));
    }
    for (request, response) in &state.recorded {
        store.write(request, response, date)?;
    }
    Ok(outcome)
}

/// The file a recording run lists the recordings it used in, which pruning
/// reads.
fn used_file() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/recordings-used")
}

/// Adds what `outcome` sent to the recordings this run has used.
pub fn note_used(outcome: &Outcome) -> Result<(), String> {
    let path = used_file();
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    for sent in &outcome.sent {
        writeln!(file, "{}", sent.request.name())
            .map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(())
}

/// Removes the recordings that the last recording run didn't use.
pub fn prune(store: &Store) -> Result<(), String> {
    let path = used_file();
    let used = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    for name in unused(store, &used)? {
        store.remove(&name)?;
    }
    Ok(())
}

/// The recordings not named in `used`, a line each. An empty list is a run that
/// used nothing, which leaves them all.
fn unused(store: &Store, used: &str) -> Result<Vec<String>, String> {
    if used.is_empty() {
        return Ok(Vec::new());
    }
    let used: Vec<&str> = used.lines().collect();
    let names = store.names()?;
    Ok(names.into_iter().filter(|name| !used.contains(&name.as_str())).collect())
}

/// Whether sego's output says the search stopped: the text summary line or the
/// error for a stopped search with no results, or `"stopped": true` in JSON.
pub fn stopped(stdout: &str, stderr: &str) -> bool {
    stdout.lines().next().is_some_and(|summary| summary.contains("  stopped at "))
        || stderr.contains("sego: error: the search stopped at ")
        || serde_json::from_str::<Value>(stdout).is_ok_and(|json| json["stopped"] == true)
}

/// The transport a case records through: it answers from the recordings, and
/// forwards upstream a request that has none.
#[derive(Clone)]
struct Recorder {
    state: Rc<RefCell<State>>,
}

struct State {
    path: String,
    store: Store,
    upstream: Rc<dyn Transport>,
    sent: Vec<Sent>,
    served: Vec<ServerResponse>,
    problems: Vec<String>,
    recorded: Vec<(Request, Response)>,
}

type Forward<'a> = &'a dyn Fn(&dyn Transport) -> Result<Response, TransportError>;

impl Recorder {
    fn new(case: &Case, store: &Store, upstream: Rc<dyn Transport>) -> Recorder {
        let state = State {
            path: case.describe(),
            store: store.clone(),
            upstream,
            sent: Vec::new(),
            served: Vec::new(),
            problems: Vec::new(),
            recorded: Vec::new(),
        };
        Recorder { state: Rc::new(RefCell::new(state)) }
    }

    fn serve(
        &self,
        request: Request,
        deadline: Instant,
        forward: Forward<'_>,
    ) -> Result<Response, TransportError> {
        let mut state = self.state.borrow_mut();
        state.sent.push(Sent { request: request.clone(), deadline });
        if let (false, Ok(found)) = (state.store.again, state.store.find(&request)) {
            let served = found.served();
            state.served.push(found);
            return Ok(served);
        }
        let refusal = |state: &mut State, message: String| {
            state.problems.push(format!("{}: recording failed: {message}", state.path));
            TransportError::Network("recording failed")
        };
        let response = match forward(&*state.upstream) {
            Ok(response) => response,
            Err(e) => return Err(refusal(&mut state, failure(&e).to_owned())),
        };
        if refused(&response) {
            let mut message = format!("upstream answered HTTP {}", response.status);
            if let Some(value) = &response.cf_mitigated {
                let _ = write!(message, " (cf-mitigated: {value})");
            }
            return Err(refusal(&mut state, message));
        }
        let kept =
            response_of(response.status, response.content_type.as_deref(), response.body.clone());
        state.recorded.push((request, kept));
        Ok(response)
    }
}

impl Logged for Recorder {
    fn sent(&self) -> Vec<Sent> {
        self.state.borrow().sent.clone()
    }

    fn problems(&self) -> Vec<String> {
        self.state.borrow().problems.clone()
    }

    fn served(&self) -> Vec<ServerResponse> {
        self.state.borrow().served.clone()
    }
}

impl Transport for Recorder {
    fn post(&self, path: &str, body: &[u8], deadline: Instant) -> Result<Response, TransportError> {
        let request = graphql(path, body).map_err(|problem| {
            self.state.borrow_mut().problems.push(problem);
            TransportError::Network("not a GraphQL request")
        })?;
        self.serve(request, deadline, &|upstream| upstream.post(path, body, deadline))
    }

    fn get(&self, path: &str, deadline: Instant) -> Result<Response, TransportError> {
        let request = Request::Raw { path: path.to_owned() };
        self.serve(request, deadline, &|upstream| upstream.get(path, deadline))
    }
}

fn failure(error: &TransportError) -> &'static str {
    match error {
        TransportError::Network(how) => how,
        TransportError::Deadline => "the deadline passed",
        TransportError::TooLarge => "the response was too large",
    }
}

/// Redirects, rate limits, server errors and Cloudflare challenges describe
/// the moment of recording, not the API; other 4xx responses are the API's own
/// answers.
fn refused(response: &Response) -> bool {
    matches!(response.status, 300..=399 | 429 | 500..=599) || response.cf_mitigated.is_some()
}

/// The UTC date of `now` as `YYYY-MM-DD`.
fn date(now: SystemTime) -> String {
    let secs = now.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let days = i64::try_from(secs / 86_400).unwrap_or(0) + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 { shifted_month + 3 } else { shifted_month - 9 };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tempfile::TempDir;

    use super::*;
    use crate::common::replay::Answers;

    const JSON: Option<&str> = Some("application/json");

    const REFS: [&str; 2] = ["refs", "example.com/a/b"];

    struct Written {
        dir: TempDir,
        case: Case,
        store: Store,
    }

    impl Written {
        fn new(args: &[&str]) -> Written {
            let dir = TempDir::new().unwrap_or_else(|e| panic!("{e}"));
            let store = Store::at(dir.path());
            Written { dir, case: Case::new(args, Vec::new()), store }
        }

        fn record(&self, responses: Vec<Response>) -> (Result<Outcome, String>, Answers) {
            let upstream = Answers::new(responses.into_iter().map(Ok).collect());
            let upstream_used = upstream.clone();
            (case(&self.case, &self.store, Rc::new(upstream), "2026-10-07"), upstream_used)
        }

        fn files(&self) -> Vec<PathBuf> {
            let entries = std::fs::read_dir(self.dir.path()).unwrap_or_else(|e| panic!("{e}"));
            entries.flatten().map(|entry| entry.path()).collect()
        }
    }

    #[test]
    fn a_request_with_no_recording_is_forwarded_and_recorded_with_the_date() {
        let written = Written::new(&REFS);
        let (result, upstream) = written.record(vec![response_of(200, JSON, "{}")]);
        assert!(result.is_ok());
        assert_eq!(upstream.methods(), ["POST"]);
        let names = written.store.names().unwrap_or_else(|e| panic!("{e}"));
        let [name] = &names[..] else { panic!("{names:?}") };
        let recording = written.store.load(name).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(recording.date, "2026-10-07");
        assert!(
            matches!(recording.request, Request::Graphql { operation, .. } if operation == "Refs")
        );
    }

    #[test]
    fn a_request_with_a_recording_is_not_forwarded() {
        let written = Written::new(&REFS);
        assert!(written.record(vec![response_of(200, JSON, "{}")]).0.is_ok());
        let (result, upstream) = written.record(vec![response_of(200, JSON, "{}")]);
        assert!(result.is_ok());
        assert_eq!(upstream.methods(), [""; 0]);
    }

    #[test]
    fn a_request_with_a_recording_is_forwarded_when_asked_to_record_again() {
        let written = Written::new(&REFS);
        assert!(written.record(vec![response_of(200, JSON, "{}")]).0.is_ok());
        let upstream = Answers::new(vec![Ok(response_of(200, JSON, r#"{"new":1}"#))]);
        let store = written.store.clone().again();
        let recorded = case(&written.case, &store, Rc::new(upstream.clone()), "2026-10-08");
        assert!(recorded.is_ok());
        assert_eq!(upstream.methods(), ["POST"]);
        let names = written.store.names().unwrap_or_else(|e| panic!("{e}"));
        let recording = written.store.load(&names[0]).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(recording.date, "2026-10-08");
    }

    #[test]
    fn a_refused_response_fails_naming_the_case_and_writes_nothing() {
        for (status, mitigated, message) in [
            (302, None, "HTTP 302"),
            (429, None, "HTTP 429"),
            (502, None, "HTTP 502"),
            (403, Some("challenge"), "HTTP 403 (cf-mitigated: challenge)"),
        ] {
            let written = Written::new(&REFS);
            let refused = Response {
                cf_mitigated: mitigated.map(str::to_owned),
                ..response_of(status, JSON, "no")
            };
            let error =
                written.record(vec![refused]).0.err().unwrap_or_else(|| panic!("{status} kept"));
            assert!(
                error.ends_with(&format!("recording failed: upstream answered {message}")),
                "{error}"
            );
            assert!(error.starts_with("refs example.com/a/b: "), "{error}");
            assert_eq!(written.files(), Vec::<PathBuf>::new());
        }
    }

    #[test]
    fn a_response_that_isnt_json_is_kept_as_text() {
        let written = Written::new(&REFS);
        let (result, _) = written.record(vec![response_of(400, Some("text/plain"), "Bad Request")]);
        assert!(result.is_ok());
        assert_eq!(written.files().len(), 1);
    }

    #[test]
    fn a_recording_no_run_used_is_unused_and_a_run_that_used_none_leaves_all() {
        let written = Written::new(&REFS);
        assert!(written.record(vec![response_of(200, JSON, "{}")]).0.is_ok());
        let names = written.store.names().unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(unused(&written.store, "other\n"), Ok(names.clone()));
        assert_eq!(unused(&written.store, &format!("{}\n", names[0])), Ok(Vec::new()));
        assert_eq!(unused(&written.store, ""), Ok(Vec::new()));
    }

    #[test]
    fn dates_are_utc_and_count_leap_days() {
        let at = |days: u64| date(UNIX_EPOCH + Duration::from_secs(days * 86_400 + 86_399));
        assert_eq!(at(0), "1970-01-01");
        assert_eq!(at(10_956), "1999-12-31");
        assert_eq!(at(11_017), "2000-03-01");
        assert_eq!(at(19_782), "2024-02-29");
        assert_eq!(at(20_733), "2026-10-07");
    }

    #[test]
    fn a_search_that_stopped_at_its_deadline_is_not_written() {
        let stopped = r#"{"data": {"search": {"results": {"alert": null, "approximateResultCount": "0",
            "dynamicFilters": [], "elapsedMilliseconds": 15, "limitHit": false, "matchCount": 0,
            "results": [], "timedout": []}}}}"#;
        let written = Written::new(&["search", "repo:example.com/a func timeout:1ms"]);
        let (result, _) = written.record(vec![response_of(200, JSON, stopped)]);
        let error = result.err().unwrap_or_else(|| panic!("recorded"));
        assert!(error.contains("the search stopped at its deadline"), "{error}");
        assert_eq!(written.files(), Vec::<PathBuf>::new());
    }

    #[test]
    fn a_result_line_that_says_stopped_at_is_not_a_stopped_search() {
        assert!(!stopped("4+ matches  1 repo  2.8 s\na.go\n3:  stopped at 20 s\n", ""));
    }
}
