use std::borrow::Cow;
use std::fmt::Write as _;

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

mod table;

pub const CUT_COLUMNS: usize = 500;

pub fn is_unsafe(c: char) -> bool {
    table::UNSAFE
        .binary_search_by(|&(first, last)| {
            if last < c {
                std::cmp::Ordering::Less
            } else if first > c {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

fn push_escaped(out: &mut String, c: char) {
    if is_unsafe(c) {
        let _ = write!(out, "\\u{{{:x}}}", u32::from(c));
    } else {
        out.push(c);
    }
}

pub fn escape(s: &str) -> Cow<'_, str> {
    if !s.chars().any(is_unsafe) {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        push_escaped(&mut out, c);
    }
    Cow::Owned(out)
}

pub fn one_line(s: &str) -> String {
    escape(&s.replace("\r\n", " ").replace('\n', " ")).into_owned()
}

pub fn strip_cr(line: &str) -> (&str, bool) {
    match line.strip_suffix('\r') {
        Some(rest) => (rest, true),
        None => (line, false),
    }
}

/// A run of escaped text that the cut never splits: a base character with
/// the zero-width characters, emoji modifiers and ZWJ-joined characters that
/// follow it, or a pair of regional indicators (a flag).
struct Unit {
    text: String,
    width: usize,
    holds_focus: bool,
}

fn units(line: &str, focus: Option<usize>) -> Vec<Unit> {
    let mut units: Vec<Unit> = Vec::new();
    let mut after_joiner = false;
    let mut indicators = 0;
    for (index, c) in line.chars().enumerate() {
        let mut piece = String::new();
        push_escaped(&mut piece, c);
        let indicator = ('\u{1f1e6}'..='\u{1f1ff}').contains(&c);
        let modifier = ('\u{1f3fb}'..='\u{1f3ff}').contains(&c);
        let attaches = !is_unsafe(c)
            && (after_joiner
                || modifier
                || UnicodeWidthChar::width(c) == Some(0)
                || (indicator && indicators % 2 == 1));
        after_joiner = c == '\u{200d}';
        indicators = if !indicator {
            0
        } else if attaches {
            indicators + 1
        } else {
            1
        };
        let holds_focus = focus == Some(index);
        match units.last_mut() {
            Some(last) if attaches => {
                last.text.push_str(&piece);
                last.holds_focus |= holds_focus;
            }
            _ => units.push(Unit { text: piece, width: 0, holds_focus }),
        }
    }
    for unit in &mut units {
        unit.width = unit.text.width();
    }
    units
}

/// Escapes `line` and, past `CUT_COLUMNS` display columns, keeps a window of
/// at most that many: centred on the character at `focus` (a code-point
/// index, clamped to the line), or from column 0 without one.
pub fn render_line(line: &str, focus: Option<usize>) -> String {
    let escaped = escape(line);
    let total = escaped.width();
    if total <= CUT_COLUMNS {
        return escaped.into_owned();
    }

    let focus = focus.map(|f| f.min(line.chars().count().saturating_sub(1)));
    let units = units(line, focus);
    let focus_unit = units.iter().position(|u| u.holds_focus);

    let mut start = 0;
    if let Some(focus_unit) = focus_unit {
        let mut before = 0;
        let mut centre = 0;
        for (i, unit) in units.iter().enumerate() {
            if i == focus_unit {
                centre = before + unit.width / 2;
                break;
            }
            before += unit.width;
        }
        let target = centre.saturating_sub(CUT_COLUMNS / 2);
        let mut column = 0;
        start = units
            .iter()
            .position(|unit| {
                let at = column;
                column += unit.width;
                at >= target
            })
            .unwrap_or(units.len())
            .min(focus_unit);
    }

    let mut end = start;
    let mut kept = 0;
    while end < units.len() && kept + units[end].width <= CUT_COLUMNS {
        kept += units[end].width;
        end += 1;
    }
    while end == units.len() && start > 0 {
        let width = units[start - 1].width;
        if kept + width > CUT_COLUMNS {
            break;
        }
        start -= 1;
        kept += width;
    }

    let window: String =
        units[start..end].iter().map(|unit| unit.text.as_str()).collect();
    let mut out = String::with_capacity(window.len() + 64);
    if start > 0 {
        out.push('…');
    }
    out.push_str(&window);
    if start > 0 || end < units.len() {
        let hidden = total.saturating_sub(window.width());
        let _ = write!(
            out,
            "… (+{hidden} more columns; --json has the full line)"
        );
    }
    out
}

#[cfg(test)]
mod tests;
