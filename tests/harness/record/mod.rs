use std::ffi::OsStr;
use std::fmt::Write as _;
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

use sego::internal::upstream::Ureq;
use sego::{Endpoint, Reply, Transport, TransportError};

use crate::case::{
    self, Case, Exchange, Headers, Request, Response, Served, Stamp,
};
use crate::replay::Replay;
use crate::run;

#[cfg(test)]
mod tests;

/// Forwards the requests a case has no response for to an upstream and turns
/// the replies into recorded exchanges.
pub struct Recorder {
    upstream: Box<dyn Transport>,
    stamp: Stamp,
}

/// An exchange as recorded. A raw read's body is kept apart from the
/// exchange, which holds none.
#[derive(Clone, Debug)]
pub struct Recorded {
    pub exchange: Exchange,
    pub raw_body: Option<Vec<u8>>,
}

impl Recorder {
    pub fn new(upstream: Box<dyn Transport>, stamp: Stamp) -> Recorder {
        Recorder { upstream, stamp }
    }

    /// A recorder that reads from sourcegraph.com, stamped with the date of
    /// `now`.
    pub fn sourcegraph(now: SystemTime) -> Result<Recorder, String> {
        let endpoint =
            Endpoint::from_env(None::<&OsStr>).map_err(|e| e.to_string())?;
        let stamp = Stamp {
            date: date(now),
            endpoint: endpoint.url(""),
            live_values: false,
        };
        Ok(Recorder::new(Box::new(Ureq::new(endpoint)), stamp))
    }

    /// Sends `request` upstream with `forward` and records the reply. The
    /// error says why the reply was refused.
    pub fn record(
        &self,
        request: &Request,
        forward: &dyn Fn(&dyn Transport) -> Result<Reply, TransportError>,
    ) -> Result<Recorded, String> {
        let reply = forward(&*self.upstream)
            .map_err(|e| format!("recording failed: {}", failure(&e)))?;
        if refused(&reply) {
            let mut message = format!(
                "recording failed: upstream answered HTTP {}",
                reply.status
            );
            if let Some(value) = &reply.cf_mitigated {
                let _ = write!(message, " (cf-mitigated: {value})");
            }
            return Err(message);
        }
        let (body, raw_body) = match request {
            Request::Graphql { .. } => (graphql_body(&reply.body)?, None),
            Request::Raw { .. } => (String::new(), Some(reply.body)),
        };
        Ok(Recorded {
            exchange: Exchange {
                request: Some(request.clone()),
                response: Response {
                    served: Served::Reply {
                        status: reply.status,
                        content_type: reply.content_type,
                        headers: Headers::default(),
                    },
                    stamp: Some(self.stamp.clone()),
                    body,
                },
            },
            raw_body,
        })
    }
}

fn failure(error: &TransportError) -> &'static str {
    match error {
        TransportError::Network(how) => how,
        TransportError::Deadline => "the deadline passed",
        TransportError::TooLarge => "the reply was too large",
    }
}

/// Redirects, rate limits, server errors and Cloudflare challenges describe
/// the moment of recording, not the API; other 4xx replies are the API's own
/// answers.
fn refused(reply: &Reply) -> bool {
    matches!(reply.status, 300..=399 | 429 | 500..=599)
        || reply.cf_mitigated.is_some()
}

/// The text of a GraphQL reply: JSON pretty-printed with sorted keys and
/// without `extensions` and `requestId`, or other text as sent.
fn graphql_body(bytes: &[u8]) -> Result<String, String> {
    let mut text = match serde_json::from_slice::<Value>(bytes) {
        Ok(mut value) => {
            strip(&mut value);
            serde_json::to_string_pretty(&value)
                .map_err(|e| format!("recording failed: {e}"))?
        }
        Err(_) => String::from_utf8(bytes.to_vec()).map_err(|_| {
            "recording failed: the reply is neither JSON nor text".to_owned()
        })?,
    };
    if text.lines().any(case::is_marker) {
        return Err("recording failed: the reply has a line that starts a \
                    section of a case"
            .to_owned());
    }
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

/// Runs the case, forwarding what it has no response for, and writes what
/// was recorded into the case file, and a raw read's body beside it. Nothing
/// is written if any reply is refused.
pub fn record_case(case: &mut Case, recorder: Recorder) -> Result<(), String> {
    let replay = Replay::recording(case, recorder);
    run::run_with(case, &replay)?;
    write_recorded(case, &replay)
}

fn write_recorded(case: &mut Case, replay: &Replay) -> Result<(), String> {
    let problems = replay.problems();
    if !problems.is_empty() {
        return Err(problems.join("\n"));
    }
    let recorded = replay.recorded();
    if recorded.is_empty() {
        return Ok(());
    }
    for (at, recorded) in &recorded {
        case.set_exchange(*at, &recorded.exchange)
            .map_err(|e| format!("{}: {e}", case.path.display()))?;
    }
    for (_, recorded) in &recorded {
        if let Some(body) = &recorded.raw_body {
            write(&case.body_path(), body)?;
        }
    }
    write(&case.path, case.to_text().as_bytes())
}

fn write(path: &std::path::Path, bytes: &[u8]) -> Result<(), String> {
    fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))
}

/// The UTC date of `now` as `YYYY-MM-DD`.
pub fn date(now: SystemTime) -> String {
    let secs = now.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let days = i64::try_from(secs / 86_400).unwrap_or(0) + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days.rem_euclid(146_097);
    let year_of_era = (day_of_era - day_of_era / 1_460 + day_of_era / 36_524
        - day_of_era / 146_096)
        / 365;
    let day_of_year =
        day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month =
        if shifted_month < 10 { shifted_month + 3 } else { shifted_month - 9 };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}
