//! Runs every case file in `tests/cases` as its own test: the command line
//! in-process against a replay of the case's own exchanges. `SEGO_BLESS=1`
//! rewrites what a case expects. `SEGO_RECORD=1` forwards the requests a case
//! has no response for to sourcegraph.com and writes the exchanges into it.

#[allow(
    dead_code,
    unused_imports,
    reason = "cargo compiles the module's tests here but runs them in the \
              harness target"
)]
mod case;
mod check;
mod compare;
#[allow(
    dead_code,
    unused_imports,
    reason = "cargo compiles the module's tests here but runs them in the \
              harness target"
)]
mod record;
mod replay;
mod run;
#[allow(
    dead_code,
    unused_imports,
    reason = "cargo compiles the module's tests here but runs them in the \
              harness target"
)]
mod shell;

use std::fs;
use std::path::Path;
use std::time::SystemTime;

use case::Case;
use record::Recorder;

fn blessing() -> bool {
    std::env::var("SEGO_BLESS").is_ok_and(|v| !v.is_empty() && v != "0")
}

fn recording() -> bool {
    std::env::var("SEGO_RECORD").is_ok_and(|v| !v.is_empty() && v != "0")
}

/// A failure whose `Debug` is its text, so the test runner prints it as
/// written.
struct Failure(String);

impl std::fmt::Debug for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Failure {}

fn run_case(path: &Path) -> datatest_stable::Result<()> {
    let mut case = Case::load(path)?;
    if recording() {
        if blessing() {
            return Err("SEGO_BLESS and SEGO_RECORD are both set".into());
        }
        let recorder = Recorder::sourcegraph(SystemTime::now())?;
        return Ok(record::record_case(&mut case, recorder)?);
    }
    let outcome = run::run_case(&case)?;
    let mut failures = check::every_case(&outcome);
    failures.extend(check::exchanges_used(&case, &outcome));
    if blessing() {
        let before = case.to_text();
        let blessed = case.bless(
            outcome.exit,
            (!case.stdout_closed).then_some(outcome.stdout.as_str()),
            &outcome.stderr,
            &outcome.sent,
        );
        match blessed {
            Ok(()) if case.to_text() != before => {
                fs::write(path, case.to_text())
                    .map_err(|e| format!("{}: {e}", path.display()))?;
            }
            Ok(()) => {}
            Err(failure) => failures.push(failure),
        }
    } else {
        failures.extend(compare::expectations(&case, &outcome));
        failures.extend(check::requests_match(&case, &outcome));
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(Box::new(Failure(failures.join("\n"))))
    }
}

datatest_stable::harness! {
    { test = run_case, root = "tests/cases", pattern = r"^refs/.*\.case$" },
}
