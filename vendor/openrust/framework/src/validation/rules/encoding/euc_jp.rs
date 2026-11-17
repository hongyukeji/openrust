#[derive(Clone, Copy)]
pub(super) enum Variant {
    Standard,
    Windows,
    Cp51932,
    Jis2004,
}

impl Variant {
    pub(super) fn check(self, mut bytes: &[u8]) -> bool {
        while !bytes.is_empty() {
            bytes = match bytes {
                [0..=0x7f, rest @ ..] | [0x8e, 0xa1..=0xdf, rest @ ..] => rest,
                [0x8f, row, column, rest @ ..] if self.plane_two(*row, *column) => rest,
                [row, column, rest @ ..] if self.plane_one(*row, *column) => rest,
                _ => return false,
            };
        }
        true
    }

    fn plane_one(self, row: u8, column: u8) -> bool {
        match self {
            Self::Standard => jis_0208(row, column),
            Self::Windows => {
                jis_0208(row, column)
                    || vendor_row(row, column)
                    || matches!(row, 0xf5..=0xfe) && full_row(column)
            }
            Self::Cp51932 => {
                jis_0208(row, column)
                    || vendor_row(row, column)
                    || match row {
                        0xf9..=0xfb => full_row(column),
                        0xfc => matches!(column, 0xa1..=0xee | 0xf1..=0xfe),
                        _ => false,
                    }
            }
            Self::Jis2004 => match row {
                0xa1..=0xa3 | 0xa5..=0xa7 | 0xa9..=0xab | 0xae..=0xfe => full_row(column),
                0xa4 => matches!(column, 0xa1..=0xfb),
                0xa8 => matches!(column, 0xa1..=0xde | 0xe7..=0xfc),
                0xac => matches!(column, 0xa1..=0xf3 | 0xfd..=0xfe),
                0xad => {
                    matches!(column, 0xa1..=0xd7 | 0xdf..=0xef | 0xf3 | 0xf8..=0xf9 | 0xfd..=0xfe)
                }
                _ => false,
            },
        }
    }

    fn plane_two(self, row: u8, column: u8) -> bool {
        match self {
            Self::Standard => jis_0212(row, column),
            Self::Windows => {
                jis_0212(row, column)
                    || matches!(row, 0xf4..=0xfe) && full_row(column)
                    || row == 0xf3 && matches!(column, 0xf3..=0xfe)
            }
            Self::Cp51932 => false,
            Self::Jis2004 => match row {
                0xa1 | 0xa3..=0xa5 | 0xa8 | 0xac..=0xaf | 0xee..=0xfd => full_row(column),
                0xfe => matches!(column, 0xa1..=0xf6),
                _ => false,
            },
        }
    }
}

fn full_row(column: u8) -> bool {
    matches!(column, 0xa1..=0xfe)
}

fn vendor_row(row: u8, column: u8) -> bool {
    row == 0xad && matches!(column, 0xa1..=0xbe | 0xc0..=0xd6 | 0xdf..=0xfc)
}

fn jis_0208(row: u8, column: u8) -> bool {
    match row {
        0xa1 | 0xb0..=0xce | 0xd0..=0xf3 => full_row(column),
        0xa2 => {
            matches!(column, 0xa1..=0xae | 0xba..=0xc1 | 0xca..=0xd0 | 0xdc..=0xea | 0xf2..=0xf9 | 0xfe)
        }
        0xa3 => matches!(column, 0xb0..=0xb9 | 0xc1..=0xda | 0xe1..=0xfa),
        0xa4 => matches!(column, 0xa1..=0xf3),
        0xa5 => matches!(column, 0xa1..=0xf6),
        0xa6 => matches!(column, 0xa1..=0xb8 | 0xc1..=0xd8),
        0xa7 => matches!(column, 0xa1..=0xc1 | 0xd1..=0xf1),
        0xa8 => matches!(column, 0xa1..=0xc0),
        0xcf => matches!(column, 0xa1..=0xd3),
        0xf4 => matches!(column, 0xa1..=0xa6),
        _ => false,
    }
}

fn jis_0212(row: u8, column: u8) -> bool {
    match row {
        0xa2 => matches!(column, 0xaf..=0xb9 | 0xc2..=0xc4 | 0xeb..=0xf1),
        0xa6 => matches!(column, 0xe1..=0xe5 | 0xe7 | 0xe9..=0xea | 0xec | 0xf1..=0xfc),
        0xa7 => matches!(column, 0xc2..=0xce | 0xf2..=0xfe),
        0xa9 => {
            matches!(column, 0xa1..=0xa2 | 0xa4 | 0xa6 | 0xa8..=0xa9 | 0xab..=0xad | 0xaf..=0xb0 | 0xc1..=0xd0)
        }
        0xaa => matches!(column, 0xa1..=0xb8 | 0xba..=0xf7),
        0xab => matches!(column, 0xa1..=0xbb | 0xbd..=0xc3 | 0xc5..=0xf7),
        0xb0..=0xec => full_row(column),
        0xed => matches!(column, 0xa1..=0xe3),
        _ => false,
    }
}
