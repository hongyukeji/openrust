#[derive(Clone, Copy)]
pub(super) enum Variant {
    Standard,
    Windows,
    Mac,
    Jis2004,
    SoftBank,
}

impl Variant {
    pub(super) fn check(self, mut bytes: &[u8]) -> bool {
        while !bytes.is_empty() {
            bytes = match bytes {
                [0x1b, ..] if matches!(self, Self::SoftBank) => {
                    let Some(rest) = after_softbank_escape(bytes) else {
                        return false;
                    };
                    rest
                }
                [0..=0x7f | 0xa1..=0xdf, rest @ ..] => rest,
                [0x80 | 0xa0 | 0xfd..=0xff, rest @ ..] if matches!(self, Self::Mac) => rest,
                [lead, trail, rest @ ..] if self.accepts_pair(*lead, *trail) => rest,
                _ => return false,
            };
        }
        true
    }

    fn accepts_pair(self, lead: u8, trail: u8) -> bool {
        match self {
            Self::Standard => jis_0208(lead, trail),
            Self::Windows | Self::SoftBank => {
                jis_0208(lead, trail) || windows_extension(lead, trail)
            }
            Self::Mac => jis_0208(lead, trail) || mac_extension(lead, trail),
            Self::Jis2004 => match lead {
                0x81 | 0x83 | 0x85 | 0x88..=0x9f | 0xe0..=0xfb => full_row(trail),
                0x82 => matches!(trail, 0x40..=0x7e | 0x80..=0xf9),
                0x84 => matches!(trail, 0x40..=0x7e | 0x80..=0xdc | 0xe5..=0xfa),
                0x86 => matches!(trail, 0x40..=0x7e | 0x80..=0xf1 | 0xfb..=0xfc),
                0x87 => {
                    matches!(trail, 0x40..=0x76 | 0x7e | 0x80..=0x8f | 0x93 | 0x98..=0x99 | 0x9d..=0xfc)
                }
                0xfc => matches!(trail, 0x40..=0x7e | 0x80..=0xf4),
                _ => false,
            },
        }
    }
}

fn full_row(trail: u8) -> bool {
    matches!(trail, 0x40..=0x7e | 0x80..=0xfc)
}

fn jis_0208(lead: u8, trail: u8) -> bool {
    match lead {
        0x81 => {
            matches!(trail, 0x40..=0x7e | 0x80..=0xac | 0xb8..=0xbf | 0xc8..=0xce | 0xda..=0xe8 | 0xf0..=0xf7 | 0xfc)
        }
        0x82 => matches!(trail, 0x4f..=0x58 | 0x60..=0x79 | 0x81..=0x9a | 0x9f..=0xf1),
        0x83 => matches!(trail, 0x40..=0x7e | 0x80..=0x96 | 0x9f..=0xb6 | 0xbf..=0xd6),
        0x84 => matches!(trail, 0x40..=0x60 | 0x70..=0x7e | 0x80..=0x91 | 0x9f..=0xbe),
        0x88 => matches!(trail, 0x9f..=0xfc),
        0x89..=0x97 | 0x99..=0x9f | 0xe0..=0xe9 => full_row(trail),
        0x98 => matches!(trail, 0x40..=0x72 | 0x9f..=0xfc),
        0xea => matches!(trail, 0x40..=0x7e | 0x80..=0xa4),
        _ => false,
    }
}

fn windows_extension(lead: u8, trail: u8) -> bool {
    match lead {
        0x87 => matches!(trail, 0x40..=0x5d | 0x5f..=0x75 | 0x7e | 0x80..=0x9c),
        0xed | 0xf0..=0xfb => full_row(trail),
        0xee => matches!(trail, 0x40..=0x7e | 0x80..=0xec | 0xef..=0xfc),
        0xfc => matches!(trail, 0x40..=0x4b),
        _ => false,
    }
}

fn mac_extension(lead: u8, trail: u8) -> bool {
    match lead {
        0x85 => {
            matches!(trail, 0x40..=0x53 | 0x5e..=0x71 | 0x7c..=0x7e | 0x80..=0x85 | 0x91..=0x9a | 0x9f..=0xad | 0xb3..=0xc1 | 0xdb..=0xf4)
        }
        0x86 => matches!(trail, 0x40..=0x5d | 0x9b..=0xa6 | 0xb3..=0xb5 | 0xc7..=0xd6),
        0x87 => {
            matches!(trail, 0x40..=0x58 | 0x91..=0xb5 | 0xbd..=0xc1 | 0xe5..=0xe8 | 0xfa..=0xfc)
        }
        0x88 => matches!(trail, 0x40..=0x42 | 0x54..=0x55 | 0x68 | 0x6a..=0x6d),
        0xeb => {
            matches!(trail, 0x41..=0x42 | 0x50..=0x51 | 0x5b..=0x5d | 0x60..=0x64 | 0x69..=0x7a | 0x81)
        }
        0xec => matches!(
            trail,
            0x9f | 0xa1 | 0xa3 | 0xa5 | 0xa7 | 0xc1 | 0xe1 | 0xe3 | 0xe5 | 0xec
        ),
        0xed => matches!(
            trail,
            0x40 | 0x42 | 0x44 | 0x46 | 0x48 | 0x62 | 0x83 | 0x85 | 0x87 | 0x8e | 0x95..=0x96
        ),
        _ => false,
    }
}

fn after_softbank_escape(bytes: &[u8]) -> Option<&[u8]> {
    let [0x1b, b'$', set, body @ ..] = bytes else {
        return None;
    };
    let assigned = match set {
        b'E' | b'G' => 0x22..=0x7a,
        b'F' => 0x23..=0x7a,
        b'O' => 0x23..=0x6d,
        b'P' => 0x22..=0x6c,
        b'Q' => 0x23..=0x5e,
        _ => return None,
    };
    for (index, byte) in body.iter().enumerate() {
        if *byte == 0x0f {
            return Some(&body[index + 1..]);
        }
        if !assigned.contains(byte) {
            return None;
        }
    }
    // A complete selector and assigned bytes may end without shift-in in mbstring.
    Some(&[])
}
