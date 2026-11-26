/// The standard and IMAP forms use different shift markers and end conditions.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Variant {
    Standard,
    Imap,
}

impl Variant {
    pub(super) fn check(self, bytes: &[u8]) -> bool {
        let shift = if self == Self::Imap { b'&' } else { b'+' };
        let mut input = bytes.iter().copied().peekable();
        while let Some(byte) = input.next() {
            if byte != shift {
                if !self.direct(byte) {
                    return false;
                }
                continue;
            }

            let Some(first) = input.next() else {
                // PHP accepts a trailing shift marker in standard UTF-7.
                return self == Self::Standard;
            };
            if first == b'-' {
                continue;
            }
            let Some(mut digit) = self.digit(first) else {
                return false;
            };

            let mut buffer = 0_u32;
            let mut bits = 0;
            let mut high_surrogate = false;
            loop {
                buffer = (buffer << 6) | u32::from(digit);
                bits += 6;
                if bits >= 16 {
                    bits -= 16;
                    let unit = (buffer >> bits) as u16;
                    buffer &= (1 << bits) - 1;
                    if high_surrogate != matches!(unit, 0xDC00..=0xDFFF)
                        || self == Self::Imap
                            && matches!(unit, 0x20..=0x7E)
                            && unit != u16::from(b'&')
                    {
                        return false;
                    }
                    high_surrogate = matches!(unit, 0xD800..=0xDBFF);
                }
                let Some(next) = input.peek().copied().and_then(|byte| self.digit(byte)) else {
                    break;
                };
                input.next();
                digit = next;
            }
            // A complete UTF-16 unit leaves at most four unused zero bits.
            if bits >= 6 || buffer != 0 || high_surrogate {
                return false;
            }
            match input.next() {
                Some(b'-') => {}
                Some(byte) if self == Self::Standard && self.direct(byte) => {}
                None if self == Self::Standard => return true,
                _ => return false,
            }
        }
        true
    }

    fn direct(self, byte: u8) -> bool {
        match self {
            Self::Standard => {
                matches!(byte, b'\0' | b'\t' | b'\r' | b'\n' | b' '..=b'}') && byte != b'\\'
            }
            Self::Imap => matches!(byte, b' '..=b'~'),
        }
    }

    fn digit(self, byte: u8) -> Option<u8> {
        Some(match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' if self == Self::Standard => 63,
            b',' if self == Self::Imap => 63,
            _ => return None,
        })
    }
}
