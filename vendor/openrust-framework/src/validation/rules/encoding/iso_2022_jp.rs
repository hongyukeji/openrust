use super::euc_jp;

#[derive(Clone, Copy)]
pub(super) enum Variant {
    Jis,
    Standard,
    Microsoft,
    Jis2004,
    Kddi,
    Cp5022x,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Ascii,
    Roman,
    Kana,
    ShiftedKana,
    Jis0208,
    Jis0212,
    Jis0213PlaneOne,
    Jis0213PlaneTwo,
    UserDefined,
}

impl Variant {
    pub(super) fn check(self, mut bytes: &[u8]) -> bool {
        let mut state = State::Ascii;
        while let Some((&byte, rest)) = bytes.split_first() {
            bytes = rest;
            match byte {
                0x1b => {
                    if state == State::ShiftedKana {
                        return false;
                    }
                    let Some((next, rest)) = self.designation(bytes) else {
                        return false;
                    };
                    state = next;
                    bytes = rest;
                }
                0x0e | 0x0f => match self {
                    Self::Standard => return false,
                    Self::Jis => {
                        state = match (byte, state) {
                            (0x0e, State::Ascii) => State::ShiftedKana,
                            (0x0f, State::ShiftedKana) => State::Ascii,
                            _ => return false,
                        };
                    }
                    Self::Cp5022x => {
                        state = if byte == 0x0e {
                            State::Kana
                        } else {
                            State::Ascii
                        };
                    }
                    _ => {}
                },
                row if self.is_double_byte(state, row) => {
                    let Some((&column @ 0x21..=0x7e, rest)) = bytes.split_first() else {
                        return false;
                    };
                    if !self.accepts_pair(state, row, column) {
                        return false;
                    }
                    bytes = rest;
                }
                0..=0x7f => {}
                0xa1..=0xdf if !matches!(self, Self::Standard | Self::Jis2004) => {}
                _ => return false,
            }
        }
        !matches!(self, Self::Standard | Self::Jis) || state == State::Ascii
    }

    fn designation(self, bytes: &[u8]) -> Option<(State, &[u8])> {
        let state = match bytes {
            [b'(', b'B', rest @ ..] => (State::Ascii, rest),
            [b'(', b'J', rest @ ..] => match self {
                Self::Jis | Self::Standard | Self::Cp5022x => (State::Roman, rest),
                Self::Microsoft | Self::Kddi => (State::Ascii, rest),
                Self::Jis2004 => return None,
            },
            [b'(', b'H', rest @ ..] if matches!(self, Self::Jis | Self::Cp5022x) => {
                (State::Ascii, rest)
            }
            [b'(', b'I', rest @ ..] if !matches!(self, Self::Standard | Self::Jis2004) => {
                (State::Kana, rest)
            }
            [b'$', b'B', rest @ ..] => (State::Jis0208, rest),
            [b'$', b'@', rest @ ..] if !matches!(self, Self::Jis2004) => (State::Jis0208, rest),
            [b'$', b'(', b'@' | b'B', rest @ ..]
                if !matches!(self, Self::Standard | Self::Jis2004) =>
            {
                (State::Jis0208, rest)
            }
            [b'$', b'(', b'D', rest @ ..] if matches!(self, Self::Jis | Self::Cp5022x) => {
                (State::Jis0212, rest)
            }
            [b'$', b'(', b'Q', rest @ ..] if matches!(self, Self::Jis2004) => {
                (State::Jis0213PlaneOne, rest)
            }
            [b'$', b'(', b'P', rest @ ..] if matches!(self, Self::Jis2004) => {
                (State::Jis0213PlaneTwo, rest)
            }
            [b'$', b'(', b'?', rest @ ..] if matches!(self, Self::Microsoft) => {
                (State::UserDefined, rest)
            }
            _ => return None,
        };
        Some(state)
    }

    fn is_double_byte(self, state: State, row: u8) -> bool {
        matches!(
            state,
            State::Jis0208
                | State::Jis0212
                | State::Jis0213PlaneOne
                | State::Jis0213PlaneTwo
                | State::UserDefined
        ) && (0x21..=match self {
            Self::Cp5022x => 0x97,
            Self::Microsoft | Self::Kddi => 0x7f,
            _ => 0x7e,
        })
            .contains(&row)
    }

    fn accepts_pair(self, state: State, row: u8, column: u8) -> bool {
        let euc_row = row | 0x80;
        let euc_column = column | 0x80;
        match state {
            State::Jis0208 => {
                euc_jp::Variant::Standard.plane_one(euc_row, euc_column)
                    || match self {
                        Self::Microsoft => windows_extension(row, column),
                        Self::Kddi => {
                            euc_jp::vendor_row(euc_row, euc_column) || matches!(row, 0x75..=0x78)
                        }
                        Self::Cp5022x => {
                            windows_extension(row, column)
                                || matches!(row, 0x7f..=0x96)
                                || row == 0x97 && column <= 0x2c
                        }
                        _ => false,
                    }
            }
            State::Jis0212 => euc_jp::Variant::Standard.plane_two(euc_row, euc_column),
            State::Jis0213PlaneOne => euc_jp::Variant::Jis2004.plane_one(euc_row, euc_column),
            State::Jis0213PlaneTwo => euc_jp::Variant::Jis2004.plane_two(euc_row, euc_column),
            State::UserDefined => matches!(row, 0x21..=0x34),
            _ => false,
        }
    }
}

fn windows_extension(row: u8, column: u8) -> bool {
    euc_jp::vendor_row(row | 0x80, column | 0x80)
        || matches!(row, 0x79..=0x7b)
        || row == 0x7c && !matches!(column, 0x6f | 0x70)
}
