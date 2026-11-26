use std::fmt::Write as _;

use serde::Serialize as _;
use serde_json::{Serializer, Value, ser::PrettyFormatter};

use crate::http::HttpError;

use super::ResourceResult;

/// Controls the JSON wire format of a resource response.
#[derive(Clone, Copy, Default)]
pub struct JsonOptions {
    pretty: bool,
    escape_slashes: bool,
    escape_unicode: bool,
}

impl JsonOptions {
    #[must_use]
    pub const fn pretty_print(mut self) -> Self {
        self.pretty = true;
        self
    }

    #[must_use]
    pub const fn escape_slashes(mut self) -> Self {
        self.escape_slashes = true;
        self
    }

    #[must_use]
    pub const fn escape_unicode(mut self) -> Self {
        self.escape_unicode = true;
        self
    }

    pub(super) fn encode(self, data: &Value) -> ResourceResult<String> {
        let output = if self.pretty {
            let mut bytes = Vec::new();
            let formatter = PrettyFormatter::with_indent(b"    ");
            data.serialize(&mut Serializer::with_formatter(&mut bytes, formatter))?;
            String::from_utf8(bytes).map_err(internal)?
        } else {
            serde_json::to_string(data)?
        };
        if !self.escape_slashes && !self.escape_unicode {
            return Ok(output);
        }

        // Serialized JSON only contains slashes and non-ASCII characters inside strings.
        let mut escaped = String::with_capacity(output.len());
        for character in output.chars() {
            if character == '/' && self.escape_slashes {
                escaped.push_str("\\/");
            } else if !character.is_ascii() && self.escape_unicode {
                for unit in character.encode_utf16(&mut [0; 2]) {
                    write!(escaped, "\\u{unit:04x}").expect("writing to a String is infallible");
                }
            } else {
                escaped.push(character);
            }
        }
        Ok(escaped)
    }
}

fn internal(source: impl std::error::Error + Send + Sync + 'static) -> HttpError {
    HttpError::Internal {
        source: Box::new(source),
    }
}
