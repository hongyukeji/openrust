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

pub(super) fn psd(bytes: &[u8]) -> Option<(u32, u32)> {
    let (height, rest) = bytes.get(14..)?.split_first_chunk()?;
    let width = rest.first_chunk()?;
    Some((u32::from_be_bytes(*width), u32::from_be_bytes(*height)))
}

pub(super) fn jpeg2000(bytes: &[u8]) -> Option<(u32, u32)> {
    // PHP reads Xsiz and Ysiz directly and only checks the marker and component count.
    if bytes.get(3)? != &0x51 {
        return None;
    }
    let (width, rest) = bytes.get(8..)?.split_first_chunk()?;
    let height = rest.first_chunk()?;
    let count = bytes.get(40..)?.first_chunk()?;
    (u16::from_be_bytes(*count) <= 256)
        .then(|| (u32::from_be_bytes(*width), u32::from_be_bytes(*height)))
}

pub(super) fn jp2(mut boxes: &[u8]) -> Option<(u32, u32)> {
    while let Some((header, content)) = boxes.split_first_chunk::<8>() {
        let (length, kind) = header.split_at(4);
        let length = u32::from_be_bytes(*length.first_chunk()?);
        if length == 1 {
            return None;
        }
        // getimagesize reads the first root codestream even if its box extends to EOF.
        if kind == b"jp2c" {
            return jpeg2000(content);
        }
        if length > i32::MAX as u32 {
            return None;
        }
        boxes = content.get(length.checked_sub(8)? as usize..)?;
    }
    None
}
