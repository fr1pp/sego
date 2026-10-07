use std::time::Instant;

use crate::error::Error;

use super::{Transport, is_html, refusal};

#[derive(Debug)]
pub enum RawError {
    NotFound,
    Failed(Error),
}

impl From<Error> for RawError {
    fn from(error: Error) -> RawError {
        RawError::Failed(error)
    }
}

/// Reads the raw bytes at `route`, a site-relative path.
pub fn read(
    transport: &dyn Transport,
    route: &str,
    deadline: Instant,
) -> Result<Vec<u8>, RawError> {
    let reply = transport.get(route, deadline).map_err(Error::from)?;
    if let Some(error) = refusal(&reply) {
        return Err(error.into());
    }
    match reply.status {
        404 => Err(RawError::NotFound),
        200 if is_html(&reply) => {
            Err(Error::upstream("sourcegraph.com answered HTTP 200 text/html")
                .into())
        }
        200 => Ok(reply.body),
        status => Err(Error::upstream(format!(
            "sourcegraph.com answered HTTP {status}"
        ))
        .into()),
    }
}
