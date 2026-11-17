mod double_byte;
mod euc_jp;
mod euc_tw;
mod gb18030;
mod iso_2022_jp;
mod shift_jis;
mod shifted;
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
    RejectBytes(fn(u8) -> bool),
    Base64,
    Utf7(utf7::Variant),
    Utf8,
    Utf16,
    Ucs2,
    Ucs4(bool),
    ShiftJis(shift_jis::Variant),
    Cp936,
    Gb18030,
    EucJp(euc_jp::Variant),
    EucTw,
    Hz,
    Iso2022Jp(iso_2022_jp::Variant),
    Iso2022Kr,
    DoubleByte(fn(u8, u8) -> bool),
    Utf32(Option<bool>),
    Decoder(&'static Encoding),
}

impl EncodingRule {
    pub fn parse(name: &str) -> ValidationResult<Self> {
        let label = name.to_ascii_lowercase();
        let codec = match label.as_str() {
            "ascii" | "7bit" => Codec::Ascii,
            // These single-byte encodings map every byte. The legacy transfer decoders
            // also accept arbitrary input, including bytes they discard while decoding.
            "8bit" | "quoted-printable" | "html-entities" | "uuencode" | "cp850" | "cp866"
            | "koi8-r" | "koi8-u" | "windows-1252" | "iso-8859-1" | "iso-8859-2" | "iso-8859-4"
            | "iso-8859-5" | "iso-8859-9" | "iso-8859-10" | "iso-8859-13" | "iso-8859-14"
            | "iso-8859-15" | "iso-8859-16" => Codec::AnyByte,
            // Match PHP mbstring's undefined positions rather than the WHATWG tables.
            "windows-1251" => Codec::RejectBytes(|byte| byte == 0x98),
            "windows-1254" => {
                Codec::RejectBytes(|byte| matches!(byte, 0x81 | 0x8d..=0x90 | 0x9d..=0x9e))
            }
            "armscii-8" => Codec::RejectBytes(|byte| matches!(byte, 0xa1 | 0xff)),
            "iso-8859-3" => Codec::RejectBytes(|byte| {
                matches!(byte, 0xa5 | 0xae | 0xbe | 0xc3 | 0xd0 | 0xe3 | 0xf0)
            }),
            "iso-8859-6" => Codec::RejectBytes(|byte| {
                matches!(
                    byte,
                    0xa1..=0xa3 | 0xa5..=0xab | 0xae..=0xba | 0xbc..=0xbe | 0xc0
                        | 0xdb..=0xdf | 0xf3..=0xff
                )
            }),
            "iso-8859-7" => Codec::RejectBytes(|byte| matches!(byte, 0xae | 0xd2 | 0xff)),
            "iso-8859-8" => {
                Codec::RejectBytes(|byte| matches!(byte, 0xa1 | 0xbf..=0xde | 0xfb | 0xfc | 0xff))
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
            "sjis" => Codec::ShiftJis(shift_jis::Variant::Standard),
            "cp932" | "sjis-win" | "sjis-mobile#docomo" | "sjis-mobile#kddi" => {
                Codec::ShiftJis(shift_jis::Variant::Windows)
            }
            "sjis-mac" => Codec::ShiftJis(shift_jis::Variant::Mac),
            "sjis-2004" => Codec::ShiftJis(shift_jis::Variant::Jis2004),
            "sjis-mobile#softbank" => Codec::ShiftJis(shift_jis::Variant::SoftBank),
            "euc-jp" => Codec::EucJp(euc_jp::Variant::Standard),
            "eucjp-win" => Codec::EucJp(euc_jp::Variant::Windows),
            "cp51932" => Codec::EucJp(euc_jp::Variant::Cp51932),
            "euc-jp-2004" => Codec::EucJp(euc_jp::Variant::Jis2004),
            "euc-cn" => Codec::DoubleByte(double_byte::euc_cn),
            "euc-tw" => Codec::EucTw,
            "hz" => Codec::Hz,
            "cp936" => Codec::Cp936,
            "gb18030" | "gb18030-2022" => Codec::Gb18030,
            "big-5" => Codec::DoubleByte(double_byte::big5),
            "cp950" => Codec::DoubleByte(double_byte::cp950),
            "euc-kr" => Codec::DoubleByte(double_byte::euc_kr),
            "uhc" => Codec::DoubleByte(double_byte::uhc),
            "jis" => Codec::Iso2022Jp(iso_2022_jp::Variant::Jis),
            "iso-2022-jp" => Codec::Iso2022Jp(iso_2022_jp::Variant::Standard),
            "iso-2022-jp-ms" => Codec::Iso2022Jp(iso_2022_jp::Variant::Microsoft),
            "iso-2022-jp-2004" => Codec::Iso2022Jp(iso_2022_jp::Variant::Jis2004),
            "iso-2022-jp-mobile#kddi" => Codec::Iso2022Jp(iso_2022_jp::Variant::Kddi),
            "cp50220" | "cp50221" | "cp50222" => Codec::Iso2022Jp(iso_2022_jp::Variant::Cp5022x),
            "iso-2022-kr" => Codec::Iso2022Kr,
            "utf-16be" | "utf-16le" => Codec::Decoder(
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
            Codec::RejectBytes(rejects) => !value.iter().copied().any(rejects),
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
            Codec::ShiftJis(variant) => variant.check(value),
            Codec::Cp936 => valid_pairs(value, |lead, trail| {
                if matches!(lead, 0x80 | 0xff) {
                    1
                } else if trail.is_some_and(|trail| double_byte::gbk(lead, trail)) {
                    2
                } else {
                    0
                }
            }),
            Codec::Gb18030 => gb18030::check(value),
            Codec::EucJp(variant) => variant.check(value),
            Codec::EucTw => euc_tw::check(value),
            Codec::Hz => shifted::hz(value),
            Codec::Iso2022Jp(variant) => variant.check(value),
            Codec::Iso2022Kr => shifted::iso_2022_kr(value),
            Codec::DoubleByte(accepts) => valid_pairs(value, |lead, trail| {
                if trail.is_some_and(|trail| accepts(lead, trail)) {
                    2
                } else {
                    0
                }
            }),
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
