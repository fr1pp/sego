use crate::error::Error;

use super::Response;
use super::graphql::{is_html, refusal};

/// The file a raw read's `response` carries: `None` when the route has none.
pub fn read(response: Response, host: &str) -> Result<Option<Vec<u8>>, Error> {
    if let Some(error) = refusal(&response, host) {
        return Err(error);
    }
    match response.status {
        404 => Ok(None),
        200 if is_html(&response) => {
            Err(Error::upstream(format!("{host} answered HTTP 200 text/html")))
        }
        200 => Ok(Some(response.body)),
        status => Err(Error::upstream(format!("{host} answered HTTP {status}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::error::Kind;
    use crate::sourcegraph::Response;

    fn failure(response: Response) -> (Kind, String) {
        match read(response, "sourcegraph.com") {
            Err(error) => (error.kind(), error.to_string()),
            Ok(body) => panic!("{body:?}"),
        }
    }

    #[test]
    fn a_200_answer_is_the_body_and_a_404_is_no_file() {
        let body = read(Response::of(200, Some("text/plain"), "a\nb\n"), "sourcegraph.com");
        assert_eq!(body.ok().flatten().unwrap_or_default(), b"a\nb\n");
        let missing = read(Response::of(404, Some("text/html"), "<html>"), "sourcegraph.com");
        assert!(matches!(missing, Ok(None)), "{missing:?}");
    }

    #[test]
    fn everything_else_is_a_failure() {
        assert_eq!(
            failure(Response::of(200, Some("text/html; charset=utf-8"), "<html>")),
            (Kind::Upstream, "sourcegraph.com answered HTTP 200 text/html".to_owned())
        );
        assert_eq!(
            failure(Response::of(502, Some("text/plain"), "bad")),
            (Kind::Upstream, "sourcegraph.com answered HTTP 502".to_owned())
        );
    }

    #[test]
    fn a_raw_read_refuses_what_every_response_refuses() {
        let mut challenge = Response::of(429, Some("text/html"), "<html>");
        challenge.cf_mitigated = Some("challenge".to_owned());
        assert_eq!(
            failure(challenge),
            (
                Kind::Edge,
                "sourcegraph.com's edge refused the request (HTTP 429, Cloudflare challenge)"
                    .to_owned()
            )
        );
        let mut moved = Response::of(302, None, "");
        moved.location = Some("/login".to_owned());
        assert_eq!(
            failure(moved),
            (Kind::Upstream, "sourcegraph.com answered HTTP 302 with Location /login".to_owned())
        );
    }
}
