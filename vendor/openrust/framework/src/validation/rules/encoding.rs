mod utf7;

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
    AnyByte,
    Base64,
    Utf7(utf7::Variant),
    Utf8,
    Utf16,
    Ucs2,
    Ucs4(bool),
    ShiftJis { windows: bool },
    EucCn,
    Utf32(Option<bool>),
    Decoder(&'static Encoding),
}

impl EncodingRule {
    pub fn parse(name: &str) -> ValidationResult<Self> {
        let label = name.to_ascii_lowercase();
        let codec = match label.as_str() {
            "ascii" | "7bit" => Codec::Ascii,
            // mbstring's legacy transfer decoders accept input without reporting malformed bytes.
            "iso-8859-1" | "8bit" | "quoted-printable" | "html-entities" | "uuencode" => {
                Codec::AnyByte
            }
            "base64" => Codec::Base64,
            "utf-7" => Codec::Utf7(utf7::Variant::Standard),
            "utf7-imap" => Codec::Utf7(utf7::Variant::Imap),
            "utf-8" => Codec::Utf8,
            "utf-16" => Codec::Utf16,
            "utf-32" => Codec::Utf32(None),
            "utf-32be" => Codec::Utf32(Some(false)),
            "utf-32le" => Codec::Utf32(Some(true)),
            "ucs-2" | "ucs-2be" | "ucs-2le" => Codec::Ucs2,
            "ucs-4" | "ucs-4be" => Codec::Ucs4(false),
            "ucs-4le" => Codec::Ucs4(true),
            "sjis" => Codec::ShiftJis { windows: false },
            "cp932" | "sjis-win" => Codec::ShiftJis { windows: true },
            "eucjp-win" => Codec::Decoder(encoding_rs::EUC_JP),
            "euc-cn" => Codec::EucCn,
            "cp936" => Codec::Decoder(encoding_rs::GBK),
            "uhc" => Codec::Decoder(encoding_rs::EUC_KR),
            "big-5" | "cp950" => Codec::Decoder(encoding_rs::BIG5),
            "utf-16be" | "utf-16le" | "euc-jp" | "iso-2022-jp" | "gb18030" | "euc-kr"
            | "windows-1251" | "windows-1252" | "windows-1254" | "iso-8859-2" | "iso-8859-3"
            | "iso-8859-4" | "iso-8859-5" | "iso-8859-6" | "iso-8859-7" | "iso-8859-8"
            | "iso-8859-9" | "iso-8859-10" | "iso-8859-13" | "iso-8859-14" | "iso-8859-15"
            | "iso-8859-16" | "cp866" | "koi8-r" | "koi8-u" => Codec::Decoder(
                Encoding::for_label_no_replacement(label.as_bytes())
                    .ok_or(ValidationError::InvalidRule)?,
            ),
            _ => return Err(ValidationError::InvalidRule),
        };
        Ok(Self {
            name: name.to_owned(),
            codec,
        })
    }

    pub fn bytes(&self, value: &[u8]) -> bool {
        match self.codec {
            Codec::Ascii => value.is_ascii(),
            Codec::AnyByte => true,
            Codec::Base64 => value.iter().all(|byte| {
                byte.is_ascii_alphanumeric()
                    || matches!(byte, b'+' | b'/' | b'=' | b' ' | b'\t' | b'\r' | b'\n')
            }),
            Codec::Utf7(variant) => variant.check(value),
            Codec::Utf8 => std::str::from_utf8(value).is_ok(),
            Codec::Utf16 => match value.strip_prefix(b"\xff\xfe") {
                Some(value) => decode(encoding_rs::UTF_16LE, value),
                None => decode(encoding_rs::UTF_16BE, value),
            },
            Codec::Ucs2 => value.len().is_multiple_of(2),
            Codec::Ucs4(little_endian) => {
                fixed_width_4(value, little_endian, |unit| unit != u32::MAX)
            }
            Codec::ShiftJis { windows } => {
                valid_pairs(value, |lead, trail| {
                    let standard = matches!(lead, 0x81..=0x86 | 0x88..=0x9F | 0xE0..=0xEA);
                    let extension = windows && matches!(lead, 0x87 | 0xEB..=0xFC);
                    if matches!(lead, 0xA1..=0xDF) {
                        1
                    } else if (standard || extension)
                        && matches!(trail, Some(0x40..=0x7E | 0x80..=0xFC))
                    {
                        2
                    } else {
                        0
                    }
                }) && decode(encoding_rs::SHIFT_JIS, value)
            }
            Codec::EucCn => {
                valid_pairs(value, |lead, trail| {
                    if matches!(lead, 0xA1..=0xF7) && matches!(trail, Some(0xA1..=0xFE)) {
                        2
                    } else {
                        0
                    }
                }) && decode(encoding_rs::GBK, value)
            }
            Codec::Utf32(little_endian) => {
                let (value, little_endian) = match little_endian {
                    Some(order) => (value, order),
                    None => value
                        .strip_prefix(b"\xff\xfe\x00\x00")
                        .map_or((value, false), |value| (value, true)),
                };
                fixed_width_4(value, little_endian, |unit| char::from_u32(unit).is_some())
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

fn valid_pairs(mut bytes: &[u8], width: impl Fn(u8, Option<u8>) -> usize) -> bool {
    while let Some((&lead, tail)) = bytes.split_first() {
        let consumed = if lead.is_ascii() {
            1
        } else {
            width(lead, tail.first().copied())
        };
        if consumed == 0 {
            return false;
        }
        bytes = &bytes[consumed..];
    }
    true
}

fn fixed_width_4(value: &[u8], little_endian: bool, valid: impl Fn(u32) -> bool) -> bool {
    let (units, remainder) = value.as_chunks::<4>();
    remainder.is_empty()
        && units.iter().all(|unit| {
            let unit = if little_endian {
                u32::from_le_bytes(*unit)
            } else {
                u32::from_be_bytes(*unit)
            };
            valid(unit)
        })
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
