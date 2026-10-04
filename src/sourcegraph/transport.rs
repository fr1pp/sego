use std::fmt;
use std::io::ErrorKind;
use std::thread;
use std::time::Duration;

use ureq::Agent;

use super::{FetchError, GLOBAL_TIMEOUT, Plan, Reply};
use crate::diagnostic::{Diagnostic, Outcome};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const BODY_LIMIT: u64 = 32 * 1024 * 1024;
/// The longest sego waits before its one retry.
const RETRY_BUDGET: Duration = Duration::from_secs(10);

/// Sends a plan to `<endpoint>/.api/graphql`.
pub struct Transport {
    url: String,
    agent: Agent,
}

impl Transport {
    pub fn new(endpoint: &str) -> Result<Transport, InvalidEndpoint> {
        if !endpoint.starts_with("http://")
            && !endpoint.starts_with("https://")
        {
            return Err(InvalidEndpoint(endpoint.to_owned()));
        }
        let agent = Agent::config_builder()
            .http_status_as_error(false)
            .max_redirects(0)
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .timeout_global(Some(GLOBAL_TIMEOUT))
            .user_agent(concat!("sego/", env!("CARGO_PKG_VERSION")))
            .build()
            .new_agent();
        let url = format!("{}/.api/graphql", endpoint.trim_end_matches('/'));
        Ok(Transport { url, agent })
    }

    #[cfg(test)]
    pub fn url(&self) -> &str {
        &self.url
    }

    /// POSTs the plan, once more after a transient failure. Any status is a
    /// `Reply`, for the decoder to judge.
    pub fn fetch(&self, plan: &Plan) -> Result<Reply, FetchError> {
        let body = plan.request().to_string();
        let first = self.send(&body);
        let attempt = match &first {
            Ok((reply, retry_after)) => Attempt::Status {
                status: reply.status,
                retry_after: *retry_after,
            },
            Err(ureq::Error::Io(error)) => Attempt::Io(error.kind()),
            Err(_) => Attempt::Other,
        };
        let last = match retry_delay(&attempt) {
            Some(delay) => {
                thread::sleep(delay);
                self.send(&body)
            }
            None => first,
        };
        match last {
            Ok((reply, _)) => Ok(reply),
            Err(error) => Err(FetchError::Transport {
                url: self.url.clone(),
                cause: match error {
                    ureq::Error::Io(error) => error.to_string(),
                    error => error.to_string(),
                },
            }),
        }
    }

    /// One exchange: the reply and its `Retry-After`.
    fn send(
        &self,
        body: &str,
    ) -> Result<(Reply, Option<Duration>), ureq::Error> {
        let response = self
            .agent
            .post(&self.url)
            .header("Content-Type", "application/json")
            .send(body)?;
        let header = |name: &str| {
            response.headers().get(name).map(|value| {
                String::from_utf8_lossy(value.as_bytes()).into_owned()
            })
        };
        let trace = header("x-trace");
        let location = header("location");
        let retry_after = header("retry-after")
            .and_then(|seconds| seconds.trim().parse().ok())
            .map(Duration::from_secs);
        let status = response.status().as_u16();
        let body = response
            .into_body()
            .into_with_config()
            .limit(BODY_LIMIT)
            .read_to_vec()?;
        Ok((Reply { status, body, trace, location }, retry_after))
    }
}

/// How a request went, as far as retrying is concerned.
enum Attempt {
    Io(ErrorKind),
    Status { status: u16, retry_after: Option<Duration> },
    Other,
}

/// How long to wait before retrying, if the failure is transient: a dropped
/// connection; a 502, 503 or 504; a 429 whose `Retry-After` fits the budget.
fn retry_delay(attempt: &Attempt) -> Option<Duration> {
    let delay = match *attempt {
        Attempt::Io(
            ErrorKind::ConnectionReset
            | ErrorKind::ConnectionAborted
            | ErrorKind::UnexpectedEof
            | ErrorKind::BrokenPipe,
        ) => Some(Duration::from_millis(250)),
        Attempt::Status { status: 502..=504, .. } => {
            Some(Duration::from_secs(1))
        }
        Attempt::Status { status: 429, retry_after } => retry_after,
        Attempt::Io(_) | Attempt::Status { .. } | Attempt::Other => None,
    };
    delay.filter(|delay| *delay <= RETRY_BUDGET)
}

#[derive(Debug)]
pub struct InvalidEndpoint(String);

impl fmt::Display for InvalidEndpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "SEGO_ENDPOINT must start with http:// or https://: {}",
            self.0
        )
    }
}

impl Diagnostic for InvalidEndpoint {
    fn code(&self) -> &'static str {
        "usage"
    }

    fn outcome(&self) -> Outcome {
        Outcome::Invalid
    }
}

#[cfg(test)]
mod tests {
    use expect_test::expect;

    use super::*;
    use crate::diagnostic::report;
    use crate::sourcegraph::Fetch;

    #[test]
    fn transient_failures_are_retried_once() {
        use std::io::ErrorKind::*;
        for kind in
            [ConnectionReset, ConnectionAborted, UnexpectedEof, BrokenPipe]
        {
            assert_eq!(
                retry_delay(&Attempt::Io(kind)),
                Some(Duration::from_millis(250))
            );
        }
        for status in [502, 503, 504] {
            assert_eq!(
                retry_delay(&Attempt::Status { status, retry_after: None }),
                Some(Duration::from_secs(1))
            );
        }
        assert_eq!(
            retry_delay(&Attempt::Status {
                status: 429,
                retry_after: Some(Duration::from_secs(3))
            }),
            Some(Duration::from_secs(3))
        );
        assert_eq!(
            retry_delay(&Attempt::Status {
                status: 429,
                retry_after: Some(Duration::from_secs(30))
            }),
            None
        );
        assert_eq!(
            retry_delay(&Attempt::Status { status: 429, retry_after: None }),
            None
        );
        assert_eq!(retry_delay(&Attempt::Io(ConnectionRefused)), None);
        assert_eq!(
            retry_delay(&Attempt::Status { status: 500, retry_after: None }),
            None
        );
        assert_eq!(retry_delay(&Attempt::Other), None);
    }

    #[test]
    fn endpoints_need_a_scheme_and_lose_a_trailing_slash() {
        assert!(Transport::new("sourcegraph.com").is_err());
        assert_eq!(
            Transport::new("https://sourcegraph.com/").unwrap().url(),
            "https://sourcegraph.com/.api/graphql"
        );
        assert_eq!(
            Transport::new("http://localhost:3080").unwrap().url(),
            "http://localhost:3080/.api/graphql"
        );
    }

    #[test]
    fn an_invalid_endpoint_is_a_usage_error() {
        let error = Transport::new("ftp://example.com").err().unwrap();
        assert_eq!(error.code(), "usage");
        assert_eq!(error.outcome(), Outcome::Invalid);
        expect![[r#"
            error: SEGO_ENDPOINT must start with http:// or https://: ftp://example.com
        "#]].assert_eq(&report(&error));
    }

    #[test]
    fn a_request_with_no_response_is_a_transport_error() {
        let transport = Transport::new("http://bad host").unwrap();
        let fetch: Fetch = &|plan| transport.fetch(plan);
        let plan = Plan { query: "{ x }", variables: serde_json::json!({}) };
        let error = fetch(&plan).unwrap_err();
        assert_eq!(error.code(), "transport");
        expect![[r#"
            error: the request to http://bad host/.api/graphql failed
            caused by: http: invalid uri character
        "#]]
        .assert_eq(&report(&error));
    }
}
