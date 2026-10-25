/// Splits like a POSIX shell, as bytes: single quotes are literal, double
/// quotes honour backslash escapes, a backslash outside quotes escapes the
/// next character, and `$'…'` decodes bash's ANSI-C escapes.
pub(crate) fn split(line: &str) -> Result<Vec<Vec<u8>>, String> {
    let mut words = Vec::new();
    let mut word = Vec::new();
    let mut started = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            c if c.is_whitespace() => {
                if started {
                    words.push(std::mem::take(&mut word));
                    started = false;
                }
            }
            '\'' => {
                started = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(c) => push(&mut word, c),
                        None => return Err("unterminated '".to_owned()),
                    }
                }
            }
            '"' => {
                started = true;
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some(c @ ('"' | '\\')) => push(&mut word, c),
                            Some(c) => {
                                push(&mut word, '\\');
                                push(&mut word, c);
                            }
                            None => return Err("unterminated \"".to_owned()),
                        },
                        Some(c) => push(&mut word, c),
                        None => return Err("unterminated \"".to_owned()),
                    }
                }
            }
            '$' if chars.peek() == Some(&'\'') => {
                chars.next();
                started = true;
                ansi_c(&mut chars, &mut word)?;
            }
            '\\' => {
                started = true;
                match chars.next() {
                    Some(c) => push(&mut word, c),
                    None => return Err("trailing backslash".to_owned()),
                }
            }
            c => {
                started = true;
                push(&mut word, c);
            }
        }
    }
    if started {
        words.push(word);
    }
    Ok(words)
}

fn push(word: &mut Vec<u8>, c: char) {
    word.extend_from_slice(c.encode_utf8(&mut [0; 4]).as_bytes());
}

/// The body of a `$'…'`, after its opening quote.
fn ansi_c(
    chars: &mut impl Iterator<Item = char>,
    word: &mut Vec<u8>,
) -> Result<(), String> {
    let unterminated = || "unterminated $'".to_owned();
    loop {
        match chars.next().ok_or_else(unterminated)? {
            '\'' => return Ok(()),
            '\\' => match chars.next().ok_or_else(unterminated)? {
                '0' => word.push(0),
                'e' => word.push(0x1b),
                'n' => word.push(b'\n'),
                'r' => word.push(b'\r'),
                't' => word.push(b'\t'),
                c @ ('\\' | '\'' | '"') => push(word, c),
                'x' => word.push(hex_byte(chars)?),
                'u' => push(word, unicode(chars)?),
                c => return Err(format!("unknown escape \\{c} in $'…'")),
            },
            c => push(word, c),
        }
    }
}

fn hex_byte(chars: &mut impl Iterator<Item = char>) -> Result<u8, String> {
    let digits: String = chars.take(2).collect();
    if digits.len() != 2 {
        return Err("\\x needs two hex digits".to_owned());
    }
    u8::from_str_radix(&digits, 16)
        .map_err(|_| format!("\\x{digits} is not hex"))
}

/// `\uHHHH` or `\u{H…}`.
fn unicode(chars: &mut impl Iterator<Item = char>) -> Result<char, String> {
    let mut digits = String::new();
    let first = chars.next();
    if first == Some('{') {
        loop {
            match chars.next() {
                Some('}') => break,
                Some(c) => digits.push(c),
                None => return Err("unterminated \\u{".to_owned()),
            }
        }
    } else {
        digits.extend(first);
        digits.extend(chars.take(3));
        if digits.chars().count() != 4 {
            return Err("\\u needs four hex digits".to_owned());
        }
    }
    u32::from_str_radix(&digits, 16)
        .ok()
        .and_then(char::from_u32)
        .ok_or_else(|| format!("\\u{digits} is not a character"))
}

#[cfg(test)]
mod tests;
