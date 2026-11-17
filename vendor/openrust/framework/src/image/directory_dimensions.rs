pub(super) fn tiff(bytes: &[u8], little_endian: bool) -> Option<(u32, u32)> {
    let short = if little_endian {
        u16::from_le_bytes
    } else {
        u16::from_be_bytes
    };
    let long = if little_endian {
        u32::from_le_bytes
    } else {
        u32::from_be_bytes
    };
    let offset = long(*bytes.get(4..)?.first_chunk()?) as usize;
    let (count, directory) = bytes.get(offset..)?.split_first_chunk()?;
    let length = usize::from(short(*count)) * 12;
    // The first directory must include the link to the next directory.
    let entries = &directory.get(..length + 4)?[..length];
    let (mut width, mut height) = (0, 0);
    for entry in entries.as_chunks::<12>().0 {
        let dimension = match short([entry[0], entry[1]]) {
            0x0100 | 0xa002 => &mut width,
            0x0101 | 0xa003 => &mut height,
            _ => continue,
        };
        let value = [entry[8], entry[9]];
        *dimension = match short([entry[2], entry[3]]) {
            1 | 6 => u32::from(value[0]),
            3 => u32::from(short(value)),
            8 => (short(value) as i16) as u32,
            4 | 9 => long([entry[8], entry[9], entry[10], entry[11]]),
            _ => continue,
        };
    }
    (width != 0 && height != 0).then_some((width, height))
}

pub(super) fn ico(bytes: &[u8]) -> Option<(u32, u32)> {
    let (count, directory) = bytes.get(4..)?.split_first_chunk()?;
    let count = u16::from_le_bytes(*count);
    if !(1..=255).contains(&count) {
        return None;
    }
    // PHP selects the last complete entry with the highest bit depth.
    let (width, height) = directory
        .as_chunks::<16>()
        .0
        .iter()
        .take(usize::from(count))
        .max_by_key(|entry| u16::from_le_bytes([entry[6], entry[7]]))
        .map_or((0, 0), |entry| (entry[0], entry[1]));
    let dimension = |value| if value == 0 { 256 } else { u32::from(value) };
    Some((dimension(width), dimension(height)))
}
