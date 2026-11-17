use encoding_rs::{DecoderResult, Encoding};

use crate::validation::{RuleFailure, ValidationError, ValidationResult, Value};

/// Validates byte sequences without replacement or text normalization.
#[derive(Clone)]
pub(in crate::validation) struct EncodingRule {
    name: String,
    codec: Codec,
}

#[derive(Clone, Copy)]
enum Codec {
    Ascii,
    Latin1,
    Utf8,
    Utf16,
    Ucs2(bool),
    Utf32(Option<bool>),
    Decoder(&'static Encoding),
}

impl EncodingRule {
    pub fn parse(name: &str) -> ValidationResult<Self> {
        let codec = match name.to_ascii_lowercase().as_str() {
            "ascii" | "us-ascii" => Codec::Ascii,
            "iso-8859-1" => Codec::Latin1,
            "utf-8" => Codec::Utf8,
            "utf-16" => Codec::Utf16,
            "utf-32" => Codec::Utf32(None),
            "utf-32be" => Codec::Utf32(Some(false)),
            "utf-32le" => Codec::Utf32(Some(true)),
            "sjis" | "sjis-win" => Codec::Decoder(encoding_rs::SHIFT_JIS),
            "euc-jp" | "euc-jp-win" => Codec::Decoder(encoding_rs::EUC_JP),
            "euc-cn" | "cp936" => Codec::Decoder(encoding_rs::GBK),
            "cp949" | "uhc" => Codec::Decoder(encoding_rs::EUC_KR),
            "cp950" => Codec::Decoder(encoding_rs::BIG5),
            "ucs-2" | "ucs-2be" => Codec::Ucs2(false),
            "ucs-2le" => Codec::Ucs2(true),
            _ => Codec::Decoder(
                Encoding::for_label_no_replacement(name.as_bytes())
                    .ok_or(ValidationError::InvalidRule)?,
            ),
        };
        Ok(Self {
            name: name.to_owned(),
            codec,
        })
    }

    pub fn bytes(&self, value: &[u8]) -> bool {
        match self.codec {
            Codec::Ascii => value.is_ascii(),
            Codec::Latin1 => true,
            Codec::Utf8 => std::str::from_utf8(value).is_ok(),
            Codec::Utf16 => match value.strip_prefix(b"\xff\xfe") {
                Some(value) => decode(encoding_rs::UTF_16LE, value),
                None => decode(encoding_rs::UTF_16BE, value),
            },
            Codec::Ucs2(little_endian) => {
                let (units, remainder) = value.as_chunks::<2>();
                remainder.is_empty()
                    && units.iter().all(|unit| {
                        let codepoint = if little_endian {
                            u16::from_le_bytes(*unit)
                        } else {
                            u16::from_be_bytes(*unit)
                        };
                        !(0xD800..=0xDFFF).contains(&codepoint)
                    })
            }
            Codec::Utf32(little_endian) => {
                let (value, little_endian) = match little_endian {
                    Some(order) => (value, order),
                    None => value
                        .strip_prefix(b"\xff\xfe\x00\x00")
                        .map_or((value, false), |value| (value, true)),
                };
                let (units, remainder) = value.as_chunks::<4>();
                remainder.is_empty()
                    && units.iter().all(|unit| {
                        let codepoint = if little_endian {
                            u32::from_le_bytes(*unit)
                        } else {
                            u32::from_be_bytes(*unit)
                        };
                        char::from_u32(codepoint).is_some()
                    })
            }
            Codec::Decoder(encoding) => decode(encoding, value),
        }
    }

    pub fn value(&self, value: &Value) -> bool {
        match value {
            Value::String(value) => self.bytes(value.as_bytes()),
            Value::Array(values) => values.iter().all(|value| self.value(value)),
            Value::Object(values) => values
                .iter()
                .all(|(key, value)| self.bytes(key.as_bytes()) && self.value(value)),
            _ => true,
        }
    }

    pub fn failure(&self) -> RuleFailure {
        let mut failure = RuleFailure::new("validation.encoding");
        failure
            .replacements
            .insert("encoding".into(), self.name.clone());
        failure
    }
}

fn decode(encoding: &'static Encoding, mut remaining: &[u8]) -> bool {
    let mut decoder = encoding.new_decoder_without_bom_handling();
    let mut buffer = [0; 4096];
    loop {
        match decoder.decode_to_utf8_without_replacement(remaining, &mut buffer, true) {
            (DecoderResult::InputEmpty, _, _) => return true,
            (DecoderResult::OutputFull, consumed, _) => remaining = &remaining[consumed..],
            (DecoderResult::Malformed(..), _, _) => return false,
        }
    }
}
