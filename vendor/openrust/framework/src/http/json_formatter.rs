use std::io;

use serde::Serialize;
use serde_json::ser::{CharEscape, CompactFormatter, Formatter};

/// String encoding of the fixed PHP JSON defaults used by the response factories.
#[derive(Clone, Copy)]
pub(super) struct JsonFormatter {
    ascii_only: bool,
    hex_html: bool,
}

impl JsonFormatter {
    pub(super) const JSONP: Self = Self {
        ascii_only: true,
        hex_html: false,
    };
    pub(super) const STREAM: Self = Self {
        ascii_only: true,
        hex_html: true,
    };
    pub(super) const EVENT: Self = Self {
        ascii_only: false,
        hex_html: true,
    };

    pub(super) fn write(
        self,
        writer: impl io::Write,
        value: &impl Serialize,
    ) -> serde_json::Result<()> {
        value.serialize(&mut serde_json::Serializer::with_formatter(writer, self))
    }
}

impl Formatter for JsonFormatter {
    fn write_string_fragment<W: ?Sized + io::Write>(
        &mut self,
        writer: &mut W,
        fragment: &str,
    ) -> io::Result<()> {
        let mut start = 0;
        for (index, character) in fragment.char_indices() {
            let hex_escape = (self.hex_html && matches!(character, '<' | '>' | '\'' | '&'))
                || (self.ascii_only && !character.is_ascii())
                || matches!(character, '\u{2028}' | '\u{2029}');
            if !hex_escape && character != '/' {
                continue;
            }
            writer.write_all(&fragment.as_bytes()[start..index])?;
            if self.hex_html && character == '<' {
                writer.write_all(br"\u003C")?;
            } else if self.hex_html && character == '>' {
                writer.write_all(br"\u003E")?;
            } else if hex_escape {
                for unit in character.encode_utf16(&mut [0; 2]) {
                    write!(writer, "\\u{unit:04x}")?;
                }
            } else {
                writer.write_all(br"\/")?;
            }
            start = index + character.len_utf8();
        }
        writer.write_all(&fragment.as_bytes()[start..])
    }

    fn write_char_escape<W: ?Sized + io::Write>(
        &mut self,
        writer: &mut W,
        escape: CharEscape,
    ) -> io::Result<()> {
        if self.hex_html && matches!(escape, CharEscape::Quote) {
            writer.write_all(br"\u0022")
        } else {
            CompactFormatter.write_char_escape(writer, escape)
        }
    }
}
