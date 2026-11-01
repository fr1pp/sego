//! Pointers (ADR 0003): each names the sego command that gets the rest of
//! a cut answer.

/// The log of `address` from where a cut log stopped, `skip` commits in,
/// listing `count` more when the cut log was asked for a count. `command` is
/// the words before the options.
pub fn log_skip(command: &str, address: &str, count: Option<u32>, skip: u64) -> String {
    let count = count.map(|count| format!("-n {count} ")).unwrap_or_default();
    format!("{command} {count}--skip {skip} {address} continues from the last commit shown")
}

/// The followed log of `address`, which can't page, asked for `count` commits.
pub fn log_more(command: &str, address: &str, count: u32) -> String {
    format!("{command} --follow -n {count} {address} lists more")
}
