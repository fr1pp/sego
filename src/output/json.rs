use std::io::{self, Write};

use serde::Serialize;
use serde_json::Serializer;
use serde_json::ser::{CompactFormatter, Formatter};

use crate::escape;

/// Writes every unsafe character as `\u` escapes of its UTF-16 units; the
/// serializer itself escapes quotes, backslashes and C0 controls.
struct EscapingFormatter;

impl Formatter for EscapingFormatter {
    fn write_string_fragment<W>(
        &mut self,
        writer: &mut W,
        fragment: &str,
    ) -> io::Result<()>
    where
        W: ?Sized + Write,
    {
        let mut rest = fragment;
        while let Some(at) = rest.find(escape::is_unsafe) {
            CompactFormatter.write_string_fragment(writer, &rest[..at])?;
            let mut chars = rest[at..].chars();
            let c = chars.next().unwrap_or_default();
            let mut units = [0u16; 2];
            for unit in c.encode_utf16(&mut units) {
                write!(writer, "\\u{unit:04x}")?;
            }
            rest = chars.as_str();
        }
        CompactFormatter.write_string_fragment(writer, rest)
    }
}

pub fn write<T: Serialize>(value: &T, out: &mut dyn Write) -> io::Result<()> {
    let mut serializer =
        Serializer::with_formatter(&mut *out, EscapingFormatter);
    value.serialize(&mut serializer).map_err(io::Error::from)?;
    out.write_all(b"\n")
}
