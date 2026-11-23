//! JSON options relevant to Scope's two valid UTF-8 string fields.
use std::fmt::Write as _;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScopeJsonOptions(u32);
impl ScopeJsonOptions {
    pub const JSON_HEX_TAG: Self = Self(1);
    pub const JSON_HEX_AMP: Self = Self(2);
    pub const JSON_HEX_APOS: Self = Self(4);
    pub const JSON_HEX_QUOT: Self = Self(8);
    pub const JSON_NUMERIC_CHECK: Self = Self(32);
    pub const JSON_UNESCAPED_SLASHES: Self = Self(64);
    pub const JSON_PRETTY_PRINT: Self = Self(128);
    pub const JSON_UNESCAPED_UNICODE: Self = Self(256);
    pub const JSON_PRESERVE_ZERO_FRACTION: Self = Self(1024);
    pub const JSON_UNESCAPED_LINE_TERMINATORS: Self = Self(2048);
    /// PHP option bits with no effect on an object of valid strings are accepted too.
    pub const fn from_bits(bits: u32) -> Self {
        Self(bits)
    }
    const fn has(self, flag: Self) -> bool {
        self.0 & flag.0 != 0
    }
    pub(crate) fn encode(self, scope: &crate::Scope) -> String {
        let id = self.string(&scope.id);
        let description = self.string(&scope.description);
        if self.has(Self::JSON_PRETTY_PRINT) {
            format!("{{\n    \"id\": {id},\n    \"description\": {description}\n}}")
        } else {
            format!("{{\"id\":{id},\"description\":{description}}}")
        }
    }
    fn string(self, text: &str) -> String {
        if self.has(Self::JSON_NUMERIC_CHECK)
            && let Some(number) = openrust::support::php_numeric_json_string(
                text,
                self.has(Self::JSON_PRESERVE_ZERO_FRACTION),
            )
        {
            return number;
        }
        let mut output = String::from("\"");
        for c in text.chars() {
            let hex = match c {
                '<' | '>' => self.has(Self::JSON_HEX_TAG),
                '&' => self.has(Self::JSON_HEX_AMP),
                '\'' => self.has(Self::JSON_HEX_APOS),
                '"' => self.has(Self::JSON_HEX_QUOT),
                _ => false,
            };
            if hex {
                write!(output, "\\u{:04X}", u32::from(c)).expect("String write");
                continue;
            }
            match c {
                '"' => output.push_str("\\\""),
                '\\' => output.push_str("\\\\"),
                '/' if !self.has(Self::JSON_UNESCAPED_SLASHES) => output.push_str("\\/"),
                '\u{8}' => output.push_str("\\b"),
                '\u{c}' => output.push_str("\\f"),
                '\n' => output.push_str("\\n"),
                '\r' => output.push_str("\\r"),
                '\t' => output.push_str("\\t"),
                c if c.is_control() && u32::from(c) < 32 => {
                    write!(output, "\\u{:04x}", u32::from(c)).expect("String write");
                }
                c if !c.is_ascii()
                    && (!self.has(Self::JSON_UNESCAPED_UNICODE)
                        || matches!(c, '\u{2028}' | '\u{2029}')
                            && !self.has(Self::JSON_UNESCAPED_LINE_TERMINATORS)) =>
                {
                    for unit in c.encode_utf16(&mut [0; 2]) {
                        write!(output, "\\u{unit:04x}").expect("String write");
                    }
                }
                c => output.push(c),
            }
        }
        output.push('"');
        output
    }
}
impl std::ops::BitOr for ScopeJsonOptions {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}
