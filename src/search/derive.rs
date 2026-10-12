use super::query::Prepared;
use super::response::{FileMatch, Results};

const PLACEHOLDER: &str = "NOT-INDEXED: ";
/// A search that comes back this close to its deadline may have been cut.
const MARGIN_MS: u64 = 1000;

/// Whether an answer that took `elapsed_ms` came back clear of the deadline.
pub fn in_time(elapsed_ms: u64, deadline_ms: u64) -> bool {
    elapsed_ms + MARGIN_MS < deadline_ms
}

/// The server's stand-in for a file the index skipped, listed as if it were
/// a match.
pub fn is_placeholder(file: &FileMatch) -> bool {
    file.symbols.is_empty()
        && matches!(
            file.chunk_matches.as_slice(),
            [chunk] if chunk.content_start.line == 0
                && chunk.content.starts_with(PLACEHOLDER)
        )
}

/// What the answer holds, from the request and the response alone.
#[derive(Debug, PartialEq, Eq)]
pub struct Counted {
    pub count: u64,
    pub at_least: bool,
    pub stopped: bool,
}

/// `placeholder_ranges` is how many ranges the placeholders in `results`
/// contributed to `matchCount`. The cap is compared with the raw
/// `matchCount`, since the server counted the placeholders against it.
pub fn counted(
    results: &Results,
    prepared: &Prepared,
    placeholder_ranges: u64,
) -> Counted {
    let count = results.match_count.saturating_sub(placeholder_ranges);
    let in_time = in_time(results.elapsed_milliseconds, prepared.deadline_ms);
    let capped = results.limit_hit
        && prepared.cap.is_some_and(|cap| results.match_count >= cap);
    // `limitHit` below the cap is not a cut: select:symbol, select:file.owners
    // and select:file.directory set it without one, and a cut by the clock
    // always lands past the deadline.
    let stopped = !capped && (!results.timedout.is_empty() || !in_time);
    Counted {
        count,
        // A zero has nothing to be a lower bound of.
        at_least: count > 0 && (results.limit_hit || stopped || prepared.diff),
        stopped,
    }
}
