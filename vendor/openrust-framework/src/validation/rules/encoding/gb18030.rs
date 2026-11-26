use super::double_byte::gbk;

pub(super) fn check(mut bytes: &[u8]) -> bool {
    while !bytes.is_empty() {
        bytes = match bytes {
            [0..=0x7f, rest @ ..] => rest,
            [lead, trail, rest @ ..] if gbk(*lead, *trail) => rest,
            [
                first @ 0x81..=0xfe,
                second @ b'0'..=b'9',
                third @ 0x81..=0xfe,
                fourth @ b'0'..=b'9',
                rest @ ..,
            ] if valid_pointer(*first, *second, *third, *fourth) => rest,
            _ => return false,
        };
    }
    true
}

fn valid_pointer(first: u8, second: u8, third: u8, fourth: u8) -> bool {
    let pointer = ((u32::from(first - 0x81) * 10 + u32::from(second - b'0')) * 126
        + u32::from(third - 0x81))
        * 10
        + u32::from(fourth - b'0');
    // Both mbstring versions accept the same BMP and supplementary ranges.
    matches!(pointer, 0..=39_419 | 189_000..=1_237_575)
}
