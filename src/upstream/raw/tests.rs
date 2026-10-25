use std::cell::Cell;
use std::time::Instant;

use crate::error::report;
use crate::upstream::{Reply, Transport, TransportError};

use super::{RawError, read};

struct Answers(Cell<Option<Reply>>);

impl Transport for Answers {
    fn post(
        &self,
        _: &str,
        _: &[u8],
        _: Instant,
    ) -> Result<Reply, TransportError> {
        unreachable!("a raw read does not post")
    }

    fn get(&self, _: &str, _: Instant) -> Result<Reply, TransportError> {
        Ok(self.0.take().expect("one raw read per transport"))
    }
}

fn reply(status: u16, content_type: Option<&str>) -> Reply {
    Reply {
        status,
        content_type: content_type.map(str::to_owned),
        location: None,
        retry_after: None,
        cf_mitigated: None,
        body: b"x".to_vec(),
    }
}

#[test]
fn a_raw_read_refuses_what_every_reply_refuses() {
    let located =
        Reply { location: Some("/login".to_owned()), ..reply(302, None) };
    let throttled = Reply {
        retry_after: Some("30".to_owned()),
        ..reply(503, Some("text/plain"))
    };
    let table = [
        (located, "sourcegraph.com answered HTTP 302 with Location /login"),
        (
            reply(302, None),
            "sourcegraph.com answered HTTP 302 with no Location",
        ),
        (throttled, "sourcegraph.com answered HTTP 503\n  Retry-After: 30"),
        (
            reply(200, Some("text/html; charset=utf-8")),
            "sourcegraph.com answered HTTP 200 text/html",
        ),
        (
            reply(403, Some("text/html")),
            "sourcegraph.com's edge refused the request (HTTP 403, \
             Cloudflare challenge)",
        ),
    ];
    for (given, expected) in table {
        let transport = Answers(Cell::new(Some(given)));
        let Err(RawError::Failed(error)) =
            read(&transport, "/raw", Instant::now())
        else {
            panic!("{expected}: the raw read did not fail");
        };
        let mut written = Vec::new();
        report(&error, &mut written).expect("a vector takes a write");
        assert_eq!(
            String::from_utf8_lossy(&written),
            format!("sego: error: {expected}\n")
        );
    }
}
