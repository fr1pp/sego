//! The integration tests: a test file per command, each test running sego on a
//! command line and pinning what it prints with a snapshot in `snapshots/`;
//! the default limits, the built binary, the cache across two runs, and the
//! check of the recordings against sourcegraph.com.

#![allow(clippy::too_many_lines, reason = "a test's length is its server response")]

mod binary;
mod cache;
mod cli;
mod commit;
mod common;
mod diff;
mod limits;
mod log;
mod ls;
mod refs;
mod search;
mod show;

use common::record;
use common::recording::Store;

/// Run by `just record`, after the run it prunes for.
#[test]
fn prune_recordings() {
    if record::pruning() {
        record::prune(&Store::default()).unwrap_or_else(|e| panic!("{e}"));
    }
}

#[test]
#[ignore = "sends requests to sourcegraph.com; run with `just live`"]
fn every_recording_answers_as_it_did() {
    let filter = std::env::var("SEGO_LIVE").unwrap_or_default();
    let drifted = common::live::drift(&filter).unwrap_or_else(|e| panic!("{e}"));
    assert!(drifted.is_empty(), "{}", drifted.join("\n"));
}
