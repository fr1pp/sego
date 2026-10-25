use std::fmt::Write as _;

use crate::case::Case;
use crate::run::Outcome;

/// What differs between the case's expected exit code, stdout and stderr and
/// the outcome's.
pub(crate) fn expectations(case: &Case, outcome: &Outcome) -> Vec<String> {
    let mut failures = Vec::new();
    if outcome.exit != case.exit {
        failures.push(format!(
            "exit: expected {}, got {}",
            case.exit, outcome.exit
        ));
    }
    if !case.stdout_closed && outcome.stdout != case.stdout {
        failures.push(format!(
            "stdout:\n{}",
            line_diff(&case.stdout, &outcome.stdout)
        ));
    }
    if outcome.stderr != case.stderr {
        failures.push(format!(
            "stderr:\n{}",
            line_diff(&case.stderr, &outcome.stderr)
        ));
    }
    failures
}

/// A longest-common-subsequence diff: `-` lines are expected only, `+` lines
/// are actual only.
fn line_diff(expected: &str, actual: &str) -> String {
    let a: Vec<&str> = expected.lines().collect();
    let b: Vec<&str> = actual.lines().collect();
    let mut table = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for i in (0..a.len()).rev() {
        for j in (0..b.len()).rev() {
            table[i][j] = if a[i] == b[j] {
                table[i + 1][j + 1] + 1
            } else {
                table[i + 1][j].max(table[i][j + 1])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    let mut diff = String::new();
    while i < a.len() || j < b.len() {
        if i < a.len() && j < b.len() && a[i] == b[j] {
            let _ = writeln!(diff, "  {}", a[i]);
            i += 1;
            j += 1;
        } else if j == b.len()
            || (i < a.len() && table[i + 1][j] >= table[i][j + 1])
        {
            let _ = writeln!(diff, "- {}", a[i]);
            i += 1;
        } else {
            let _ = writeln!(diff, "+ {}", b[j]);
            j += 1;
        }
    }
    if expected.ends_with('\n') != actual.ends_with('\n') {
        diff.push_str("(the trailing newline differs)\n");
    }
    diff
}
