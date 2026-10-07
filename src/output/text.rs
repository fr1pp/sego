use super::Marker;
use crate::address::Pinned;
use crate::escape;

pub fn header(address: &Pinned, fields: &[String]) -> String {
    header_line(&address.to_string(), fields)
}

/// `lead` is written as it is: the caller has already made it safe.
pub fn header_line(lead: &str, fields: &[String]) -> String {
    let mut line = lead.to_owned();
    for field in fields {
        line.push_str("  ");
        line.push_str(&escape::one_line(field));
    }
    line.push('\n');
    line
}

pub fn gutter_width(last_line: u32) -> usize {
    (last_line.checked_ilog10().unwrap_or(0) as usize + 1).max(4)
}

pub fn code_line(
    number: u32,
    width: usize,
    marker: Marker,
    text: &str,
    focus: Option<usize>,
) -> String {
    let marker = match marker {
        Marker::Match => ':',
        Marker::Context => '-',
    };
    format!("{number:>width$}{marker}{}\n", escape::render_line(text, focus))
}
