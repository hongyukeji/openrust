use std::io::{self, Write};

use serde::Serialize;
use serde_json::{
    Value,
    ser::{Formatter, Serializer},
};

use crate::{Error, Result};

pub(super) const MAX_BODY_BYTES: usize = 16 * 1024 * 1024;

pub(super) enum Payload {
    Json(Value),
    Raw(Vec<u8>),
}

impl Payload {
    pub(super) fn prepare(self, get: bool) -> Result<(Vec<u8>, Option<String>)> {
        match self {
            Self::Raw(bytes) if !get && bytes.len() <= MAX_BODY_BYTES => Ok((bytes, None)),
            Self::Raw(_) => Err(Error::InvalidPayload),
            Self::Json(value) => {
                if !value.is_array() && !value.is_object() {
                    return Err(Error::InvalidPayload);
                }
                let bytes = encode_json(&value, MAX_BODY_BYTES)?;
                let query = if get {
                    Some(openrust::support::Arr::query(&value).map_err(|_| Error::InvalidPayload)?)
                } else {
                    None
                };
                Ok((bytes, query))
            }
        }
    }
}

pub(super) fn encode_json(value: &Value, limit: usize) -> Result<Vec<u8>> {
    let mut body = LimitedBody {
        bytes: Vec::new(),
        limit,
    };
    value
        .serialize(&mut Serializer::with_formatter(&mut body, PhpJson))
        .map_err(|_| Error::InvalidPayload)?;
    Ok(body.bytes)
}

struct LimitedBody {
    bytes: Vec<u8>,
    limit: usize,
}

impl Write for LimitedBody {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
            return Err(io::Error::other("Webhook 正文超过限制"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct PhpJson;

impl Formatter for PhpJson {
    fn write_string_fragment<W: ?Sized + Write>(
        &mut self,
        writer: &mut W,
        fragment: &str,
    ) -> io::Result<()> {
        let mut start = 0;
        for (index, character) in fragment.char_indices() {
            if character.is_ascii() && character != '/' {
                continue;
            }
            writer.write_all(&fragment.as_bytes()[start..index])?;
            if character == '/' {
                writer.write_all(br"\/")?;
            } else {
                for unit in character.encode_utf16(&mut [0; 2]) {
                    write!(writer, "\\u{unit:04x}")?;
                }
            }
            start = index + character.len_utf8();
        }
        writer.write_all(&fragment.as_bytes()[start..])
    }
}
