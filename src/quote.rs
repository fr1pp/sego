use std::borrow::Cow;
use std::fmt::Write as _;

use crate::escape::is_unsafe;

fn is_bare(c: char) -> bool {
    c.is_ascii_alphanumeric() || "@%+=:,./_-".contains(c)
}

fn needs_ansi_c(c: char) -> bool {
    c.is_control() || is_unsafe(c)
}

pub fn shell_word(s: &str) -> Cow<'_, str> {
    if !s.is_empty() && !s.starts_with(['-', '=']) && s.chars().all(is_bare) {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len() + 3);
    if s.chars().any(needs_ansi_c) {
        out.push_str("$'");
        for c in s.chars() {
            match c {
                '\n' => out.push_str("\\n"),
                '\t' => out.push_str("\\t"),
                '\\' => out.push_str("\\\\"),
                '\'' => out.push_str("\\'"),
                c if needs_ansi_c(c) => {
                    let mut buf = [0; 4];
                    for byte in c.encode_utf8(&mut buf).bytes() {
                        let _ = write!(out, "\\{byte:03o}");
                    }
                }
                c => out.push(c),
            }
        }
        out.push('\'');
    } else {
        out.push('\'');
        for c in s.chars() {
            if c == '\'' {
                out.push_str("'\\''");
            } else {
                out.push(c);
            }
        }
        out.push('\'');
    }
    Cow::Owned(out)
}

#[cfg(test)]
mod tests;
