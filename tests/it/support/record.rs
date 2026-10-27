use std::cell::RefCell;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;
use std::rc::Rc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde_json::Value;

use sego::{Endpoint, Response, Transport, TransportError};

use super::case::{Body, Case, Exchange, Recorded, Request};
use super::replay::{Sent, graphql, response};
use super::{Logged, Paced, case_paths, run};

/// Records every case against sourcegraph.com, one request a second, and
/// stops at the first refusal. Returns why each case that failed to record
/// failed.
pub fn every_case() -> Vec<String> {
    let endpoint = Endpoint::default();
    let stamp =
        Recorded { date: date(SystemTime::now()), endpoint: endpoint.url(""), live_values: false };
    let upstream = Rc::new(Paced::new(sego::connect(endpoint)));
    let paths = match case_paths() {
        Ok(paths) => paths,
        Err(e) => return vec![e],
    };
    let mut failures = Vec::new();
    for path in paths {
        if upstream.blocked() {
            failures.push("recording stopped after sourcegraph.com refused a request".to_owned());
            break;
        }
        if let Err(e) = record_case(&path, upstream.clone(), &stamp) {
            failures.push(e);
        }
    }
    failures
}

/// Runs the case, forwarding to `upstream` each request that has no exchange,
/// or whose recorded exchange answered a different request, and writes what
/// was recorded into the case file, with a raw read's body beside it. Nothing
/// is written if any request was refused.
fn record_case(path: &Path, upstream: Rc<dyn Transport>, stamp: &Recorded) -> Result<(), String> {
    let mut case = Case::load(path)?;
    let recording = Recording::new(&case, upstream, stamp.clone());
    run(&case, &recording)?;
    let state = recording.state.borrow();
    if !state.problems.is_empty() {
        return Err(state.problems.join("\n"));
    }
    if state.recorded.is_empty() {
        return Ok(());
    }
    for (at, exchange, _) in &state.recorded {
        case.set_exchange(*at, exchange)?;
    }
    for (_, exchange, raw) in &state.recorded {
        if let (Body::File(name), Some(raw)) = (&exchange.body, raw) {
            write(&case.dir().join(name), raw)?;
        }
    }
    write(&case.path, case.to_text().as_bytes())
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))
}

/// The transport a case records through: it serves the case's exchanges, and
/// forwards upstream a request that has none, or whose recorded exchange
/// answered another request.
#[derive(Clone)]
struct Recording {
    state: Rc<RefCell<State>>,
}

struct State {
    case: Case,
    upstream: Rc<dyn Transport>,
    stamp: Recorded,
    sent: Vec<Sent>,
    problems: Vec<String>,
    /// Each exchange recorded, where it goes in the case, and a raw read's
    /// body.
    recorded: Vec<(usize, Exchange, Option<Vec<u8>>)>,
}

type Forward<'a> = &'a dyn Fn(&dyn Transport) -> Result<Response, TransportError>;

impl Recording {
    fn new(case: &Case, upstream: Rc<dyn Transport>, stamp: Recorded) -> Recording {
        let state = State {
            case: case.clone(),
            upstream,
            stamp,
            sent: Vec::new(),
            problems: Vec::new(),
            recorded: Vec::new(),
        };
        Recording { state: Rc::new(RefCell::new(state)) }
    }

    fn serve(
        &self,
        request: Request,
        deadline: Instant,
        forward: Forward<'_>,
    ) -> Result<Response, TransportError> {
        let mut state = self.state.borrow_mut();
        let at = state.sent.len();
        state.sent.push(Sent { request: request.clone(), deadline });
        let written = state.case.exchanges.get(at).cloned();
        if let Some(written) = &written
            && !answers_another_request(written, &request)
        {
            return response(written, state.case.dir()).map_err(|problem| {
                state.problems.push(problem);
                TransportError::Network("the exchange can't be served")
            });
        }
        let refusal = |state: &mut State, message: String| {
            state
                .problems
                .push(format!("{}: recording failed: {message}", state.case.path.display()));
            TransportError::Network("recording failed")
        };
        let answer = match forward(&*state.upstream) {
            Ok(answer) => answer,
            Err(e) => return Err(refusal(&mut state, failure(&e).to_owned())),
        };
        if refused(&answer) {
            let mut message = format!("upstream answered HTTP {}", answer.status);
            if let Some(value) = &answer.cf_mitigated {
                let _ = write!(message, " (cf-mitigated: {value})");
            }
            return Err(refusal(&mut state, message));
        }
        let live_values =
            written.as_ref().and_then(|w| w.recorded.as_ref()).is_some_and(|r| r.live_values);
        let (body, raw) = match &request {
            Request::Graphql { .. } => match graphql_body(&answer.body) {
                Ok(text) => (Body::Text(text), None),
                Err(message) => return Err(refusal(&mut state, message)),
            },
            Request::Raw { .. } => {
                let name = match written.map(|w| w.body) {
                    Some(Body::File(name)) => name,
                    _ => format!(
                        "{}.body",
                        state.case.path.file_stem().and_then(|s| s.to_str()).unwrap_or("case")
                    ),
                };
                (Body::File(name), Some(answer.body.clone()))
            }
        };
        let exchange = Exchange {
            request: Some(request),
            status: answer.status,
            content_type: answer.content_type.clone(),
            recorded: Some(Recorded { live_values, ..state.stamp.clone() }),
            body,
        };
        state.recorded.push((at, exchange, raw));
        Ok(answer)
    }
}

impl Logged for Recording {
    fn sent(&self) -> Vec<Sent> {
        self.state.borrow().sent.clone()
    }

    fn problems(&self) -> Vec<String> {
        self.state.borrow().problems.clone()
    }
}

impl Transport for Recording {
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

/// A recorded response is kept only for the request it answered.
fn answers_another_request(exchange: &Exchange, request: &Request) -> bool {
    exchange.recorded.is_some()
        && exchange.request.as_ref().is_some_and(|written| !written.same_as(request))
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

/// The text of a GraphQL response: JSON pretty-printed with sorted keys and
/// without `extensions` and `requestId`, or other text as sent.
fn graphql_body(bytes: &[u8]) -> Result<String, String> {
    let mut text = match serde_json::from_slice::<Value>(bytes) {
        Ok(mut value) => {
            strip(&mut value);
            serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?
        }
        Err(_) => String::from_utf8(bytes.to_vec())
            .map_err(|_| "the response is neither JSON nor text".to_owned())?,
    };
    if !text.ends_with('\n') {
        text.push('\n');
    }
    Ok(text)
}

fn strip(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.remove("extensions");
            map.remove("requestId");
            map.values_mut().for_each(strip);
        }
        Value::Array(items) => items.iter_mut().for_each(strip),
        _ => {}
    }
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
    use std::path::PathBuf;
    use std::time::Duration;

    use tempfile::TempDir;

    use super::*;
    use crate::support::replay::{Answers, response_of};

    const JSON: Option<&str> = Some("application/json");

    const NEW_CASE: &str =
        "# Not found.\nargs = [\"refs\", \"example.com/a/b\"]\nstatus.code = 1\n";

    const SERVER_JSON: &str = r#"{
      "data": {"r0": null, "list": [{"requestId": "r", "k": 2}]},
      "extensions": {"trace": "t"},
      "requestId": "q"
    }"#;

    const RECORDED_JSON: &str = "{\n  \"data\": {\n    \"list\": [\n      {\n        \"k\": 2\n      }\n    ],\n    \"r0\": null\n  }\n}\n";

    struct Written {
        _dir: TempDir,
        path: PathBuf,
    }

    impl Written {
        fn new(text: &str) -> Written {
            let dir = TempDir::new().unwrap_or_else(|e| panic!("{e}"));
            let path = dir.path().join("x.toml");
            fs::write(&path, text).unwrap_or_else(|e| panic!("{e}"));
            Written { _dir: dir, path }
        }

        fn text(&self) -> String {
            fs::read_to_string(&self.path).unwrap_or_else(|e| panic!("{e}"))
        }

        fn case(&self) -> Case {
            Case::load(&self.path).unwrap_or_else(|e| panic!("{e}"))
        }

        fn record(&self, responses: Vec<Response>) -> (Result<(), String>, Answers) {
            let upstream = Answers::new(responses.into_iter().map(Ok).collect());
            let stamp = Recorded {
                date: "2026-10-07".to_owned(),
                endpoint: "https://sourcegraph.com".to_owned(),
                live_values: false,
            };
            (record_case(&self.path, Rc::new(upstream.clone()), &stamp), upstream)
        }
    }

    #[test]
    fn a_request_with_no_exchange_is_forwarded_and_written_with_its_stamp() {
        let written = Written::new(NEW_CASE);
        let (result, upstream) = written.record(vec![response_of(200, JSON, SERVER_JSON)]);
        result.unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(upstream.methods(), ["POST"]);
        let case = written.case();
        let [exchange] = &case.exchanges[..] else { panic!("{}", written.text()) };
        assert!(
            matches!(&exchange.request, Some(Request::Graphql { operation, .. }) if operation == "Refs")
        );
        assert_eq!(exchange.body, Body::Text(RECORDED_JSON.to_owned()));
        assert_eq!(exchange.recorded.as_ref().map(|r| r.date.as_str()), Some("2026-10-07"));
        assert!(written.text().starts_with(NEW_CASE), "{}", written.text());
    }

    #[test]
    fn a_request_that_has_an_exchange_is_not_forwarded() {
        let written = Written::new(NEW_CASE);
        written
            .record(vec![response_of(200, JSON, SERVER_JSON)])
            .0
            .unwrap_or_else(|e| panic!("{e}"));
        let recorded = written.text();
        let (result, upstream) = written.record(vec![response_of(200, JSON, "{}")]);
        result.unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(upstream.methods(), [""; 0]);
        assert_eq!(written.text(), recorded);
    }

    #[test]
    fn a_recorded_exchange_whose_request_changed_is_recorded_again_with_its_live_mark() {
        let written = Written::new(NEW_CASE);
        written
            .record(vec![response_of(200, JSON, SERVER_JSON)])
            .0
            .unwrap_or_else(|e| panic!("{e}"));
        let stale = written
            .text()
            .replace("\"repo0\":\"example.com/a/b\"", "\"repo0\":\"example.com/c/d\"")
            .replace(
                "https://sourcegraph.com\" }",
                "https://sourcegraph.com\", live = \"values\" }",
            );
        fs::write(&written.path, stale).unwrap_or_else(|e| panic!("{e}"));
        let (result, upstream) = written.record(vec![response_of(200, JSON, SERVER_JSON)]);
        result.unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(upstream.methods(), ["POST"]);
        let text = written.text();
        assert!(!text.contains("example.com/c/d"), "{text}");
        assert!(written.case().exchanges[0].recorded.as_ref().is_some_and(|r| r.live_values));
    }

    #[test]
    fn an_exchange_that_isnt_recorded_is_never_forwarded() {
        let written = Written::new(&format!(
            "{NEW_CASE}\n[[exchange]]\noperation = \"Refs\"\nquery = \"stale\"\nstatus = 200\nresponse = '{{\"data\": null}}'\n"
        ));
        let before = written.text();
        let (result, upstream) = written.record(vec![response_of(200, JSON, "{}")]);
        result.unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(upstream.methods(), [""; 0]);
        assert_eq!(written.text(), before);
    }

    #[test]
    fn a_refused_response_fails_naming_the_case_and_writes_nothing() {
        for (status, mitigated, message) in [
            (302, None, "HTTP 302"),
            (429, None, "HTTP 429"),
            (502, None, "HTTP 502"),
            (403, Some("challenge"), "HTTP 403 (cf-mitigated: challenge)"),
        ] {
            let written = Written::new(NEW_CASE);
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
            assert!(error.contains("x.toml"), "{error}");
            assert_eq!(written.text(), NEW_CASE);
        }
    }

    #[test]
    fn a_response_that_isnt_json_is_kept_as_text() {
        let written = Written::new(NEW_CASE);
        let (result, _) = written.record(vec![response_of(400, Some("text/plain"), "Bad Request")]);
        result.unwrap_or_else(|e| panic!("{e}"));
        let exchange = &written.case().exchanges[0];
        assert_eq!(
            (exchange.status, &exchange.body),
            (400, &Body::Text("Bad Request\n".to_owned()))
        );
    }

    #[test]
    fn a_raw_read_is_recorded_with_its_body_beside_the_case() {
        let written = Written::new(NEW_CASE);
        let stamp = Recorded { date: "d".to_owned(), endpoint: "e".to_owned(), live_values: false };
        let upstream =
            Answers::new(vec![Ok(response_of(200, Some("text/plain"), b"\xff\0bytes".to_vec()))]);
        let recording = Recording::new(&written.case(), Rc::new(upstream), stamp);
        let deadline = Instant::now() + Duration::from_secs(5);
        recording.get("/r/x/-/raw/f", deadline).unwrap_or_else(|e| panic!("{e:?}"));
        let state = recording.state.borrow();
        let [(0, exchange, Some(body))] = &state.recorded[..] else { panic!() };
        assert_eq!(exchange.body, Body::File("x.body".to_owned()));
        assert_eq!(body, b"\xff\0bytes");
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
}
