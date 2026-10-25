//! Tests that need `run` itself, one module per concept in GLOSSARY.md.

#![allow(
    dead_code,
    reason = "the harness modules are shared with the cases target, which uses \
              the rest"
)]

// The harness declares tests for the modules it shares; this target declares
// none.
macro_rules! module_tests {
    () => {};
}

mod client_deadline;
mod endpoint;
mod raw_file_cache;
mod raw_read;
mod usage_error;

mod envelope;
mod invocation;

// The transport the cases use, and the modules it is built from.
#[path = "../harness/case/mod.rs"]
mod case;
#[path = "../harness/record/mod.rs"]
mod record;
#[path = "../harness/replay.rs"]
mod replay;
#[path = "../harness/run.rs"]
mod run;
#[path = "../harness/shell/mod.rs"]
mod shell;
