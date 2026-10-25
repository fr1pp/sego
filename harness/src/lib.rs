//! The test harness for sego: runs the case files in `tests/cases` against a
//! replay of each case's own exchanges, and re-sends their recorded requests to
//! sourcegraph.com. `SEGO_BLESS=1` rewrites what a case expects. `SEGO_LIVE=1`
//! sends each recorded request and compares the response with the recorded
//! one. `SEGO_RECORD=1` forwards the requests a case has no response for to
//! sourcegraph.com and writes the exchanges into it.

mod case;
mod check;
mod compare;
mod drift;
mod hostile;
pub mod literals;
mod live;
mod record;
pub mod replay;
mod run;
mod shell;

use std::fmt;
use std::fs;
use std::path::Path;
use std::time::SystemTime;

use case::Case;
use record::Recorder;

fn flag(name: &str) -> bool {
    std::env::var(name).is_ok_and(|v| !v.is_empty() && v != "0")
}

/// A failure whose `Debug` is its text, so the test runner prints it as
/// written.
struct Failure(String);

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl std::error::Error for Failure {}

fn failed(failures: &[String]) -> datatest_stable::Result<()> {
    if failures.is_empty() {
        Ok(())
    } else {
        Err(Box::new(Failure(failures.join("\n"))))
    }
}

/// Runs one case file: against its replay, or with `SEGO_LIVE` or
/// `SEGO_RECORD`, against sourcegraph.com.
pub fn run_case(path: &Path) -> datatest_stable::Result<()> {
    let mut case = Case::load(path)?;
    if flag("SEGO_LIVE") {
        live_case(&case)
    } else if flag("SEGO_RECORD") {
        record_case(&mut case)
    } else {
        compare_case(path, &mut case)
    }
}

fn live_case(case: &Case) -> datatest_stable::Result<()> {
    failed(&live::check(case, &live::connect))
}

fn record_case(case: &mut Case) -> datatest_stable::Result<()> {
    if flag("SEGO_BLESS") {
        return Err("SEGO_BLESS and SEGO_RECORD are both set".into());
    }
    let recorder = Recorder::sourcegraph(SystemTime::now())?;
    Ok(record::record_case(case, recorder)?)
}

/// Runs the case and compares what it did with what the case expects; with
/// `SEGO_BLESS`, rewrites the case to what it did.
fn compare_case(path: &Path, case: &mut Case) -> datatest_stable::Result<()> {
    let outcome = run::run_case(case)?;
    let mut failures = check::every_case(&outcome);
    failures.extend(check::exchanges_used(case, &outcome));
    if flag("SEGO_BLESS") {
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
        failures.extend(compare::expectations(case, &outcome));
        failures.extend(check::requests_match(case, &outcome));
    }
    failures.extend(hostile::check(case, &outcome));
    failed(&failures)
}
