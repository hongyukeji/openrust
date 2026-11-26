use super::double_byte::{euc_cn, euc_kr};

pub(super) fn hz(mut bytes: &[u8]) -> bool {
    let mut chinese = false;
    while !bytes.is_empty() {
        bytes = match bytes {
            // mbstring accepts a trailing tilde in either HZ mode.
            [b'~'] => return true,
            [b'~', b'\n', rest @ ..] => rest,
            [b'~', b'{', rest @ ..] if !chinese => {
                chinese = true;
                rest
            }
            [b'~', b'}', rest @ ..] if chinese => {
                chinese = false;
                rest
            }
            [b'~', b'~', rest @ ..] if !chinese => rest,
            [b'~', ..] => return false,
            [0..=0x7f, rest @ ..] if !chinese => rest,
            [
                row @ (0x21..=0x29 | 0x30..=0x77),
                column @ 0x21..=0x7e,
                rest @ ..,
            ] if chinese && euc_cn(*row | 0x80, *column | 0x80) => rest,
            _ => return false,
        };
    }
    true
}

pub(super) fn iso_2022_kr(mut bytes: &[u8]) -> bool {
    let mut korean = false;
    while !bytes.is_empty() {
        bytes = match bytes {
            [0x1b, b'$', b')', b'C', rest @ ..] | [0x0f, rest @ ..] => {
                korean = false;
                rest
            }
            [0x0e, rest @ ..] => {
                korean = true;
                rest
            }
            [0x1b, ..] => return false,
            [0..=0x7f, rest @ ..] if !korean => rest,
            [row @ 0x21..=0x7e, column @ 0x21..=0x7e, rest @ ..]
                if korean
                    && (*row != 0x22 || *column <= 0x65)
                    && euc_kr(*row | 0x80, *column | 0x80) =>
            {
                rest
            }
            _ => return false,
        };
    }
    true
}
