use flate2::{Decompress, FlushDecompress, Status};

pub(super) fn iff(bytes: &[u8]) -> Option<(u32, u32)> {
    let (form, mut chunks) = bytes.get(8..)?.split_first_chunk::<4>()?;
    if !matches!(form, b"ILBM" | b"PBM ") {
        return None;
    }
    while let Some((header, body)) = chunks.split_first_chunk::<8>() {
        let length = u32::from_be_bytes(*header[4..].first_chunk()?);
        if length >= i32::MAX as u32 {
            return None;
        }
        let aligned = length + length % 2;
        if &header[..4] == b"BMHD" {
            if aligned < 9 {
                return None;
            }
            let bitmap = body.get(..9)?;
            let width = i16::from_be_bytes([bitmap[0], bitmap[1]]);
            let height = i16::from_be_bytes([bitmap[2], bitmap[3]]);
            if width > 0 && height > 0 && (1..=32).contains(&bitmap[8]) {
                return Some((width as u32, height as u32));
            }
            chunks = &body[9..];
        } else {
            chunks = body.get(aligned as usize..)?;
        }
    }
    None
}

pub(super) fn wbmp(bytes: &[u8]) -> Option<(u32, u32)> {
    let mut bytes = bytes.strip_prefix(&[0])?.iter().copied();
    bytes.find(|byte| byte & 0x80 == 0)?;
    Some((wbmp_dimension(&mut bytes)?, wbmp_dimension(&mut bytes)?))
}

fn wbmp_dimension(bytes: &mut impl Iterator<Item = u8>) -> Option<u32> {
    let mut dimension = 0;
    for byte in bytes {
        dimension = (dimension << 7) | u32::from(byte & 0x7f);
        if dimension > 2048 {
            return None;
        }
        if byte & 0x80 == 0 {
            return (dimension != 0).then_some(dimension);
        }
    }
    None
}

pub(super) fn xbm(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 12 {
        return None;
    }
    let (mut width, mut height) = (0, 0);
    for line in bytes.split(|byte| *byte == b'\n') {
        let line = line.split(|byte| *byte == 0).next()?;
        let Some(define) = line.strip_prefix(b"#define") else {
            continue;
        };
        let mut fields = define
            .split(|byte| byte.is_ascii_whitespace() || *byte == 0x0b)
            .filter(|field| !field.is_empty());
        let Some(name) = fields.next() else { continue };
        let dimension = match name.rsplit(|byte| *byte == b'_').next() {
            Some(b"width") => &mut width,
            Some(b"height") => &mut height,
            _ => continue,
        };
        let Some(value) = fields.next().and_then(xbm_dimension) else {
            continue;
        };
        *dimension = value;
        if width != 0 && height != 0 {
            return Some((width, height));
        }
    }
    None
}

fn xbm_dimension(bytes: &[u8]) -> Option<u32> {
    let digits = usize::from(matches!(bytes.first(), Some(b'+' | b'-')));
    let length = bytes[digits..]
        .iter()
        .take_while(|byte| byte.is_ascii_digit())
        .count();
    if length == 0 {
        return None;
    }
    let number = std::str::from_utf8(&bytes[..digits + length]).ok()?;
    let limit = if number.starts_with('-') {
        i64::MIN
    } else {
        i64::MAX
    };
    Some(number.parse::<i64>().unwrap_or(limit) as u32)
}

pub(super) fn swf(bytes: &[u8]) -> Option<(u32, u32)> {
    Some(swf_rectangle(bytes.get(8..40)?))
}

pub(super) fn swc(bytes: &[u8]) -> Option<(u32, u32)> {
    let compressed = bytes.get(8..)?;
    if compressed.len() < 64 {
        return None;
    }
    let mut decoder = Decompress::new(true);
    let mut rectangle = [0; 17];
    let mut output = [0; 8192];
    let limit = compressed.len().saturating_mul(1 << 15) as u64;
    loop {
        let input_start = decoder.total_in();
        let output_start = decoder.total_out();
        let status = decoder
            .decompress(
                &compressed[input_start as usize..],
                &mut output,
                FlushDecompress::None,
            )
            .ok()?;
        let output_end = decoder.total_out();
        if output_end > limit {
            return None;
        }
        if output_start < rectangle.len() as u64 {
            let start = output_start as usize;
            let end = output_end.min(rectangle.len() as u64) as usize;
            rectangle[start..end].copy_from_slice(&output[..end - start]);
        }
        if status == Status::StreamEnd {
            return Some(swf_rectangle(&rectangle));
        }
        if input_start == decoder.total_in() && output_start == output_end {
            return None;
        }
    }
}

fn swf_rectangle(bytes: &[u8]) -> (u32, u32) {
    let field_bits = usize::from(bytes[0] >> 3);
    let coordinate = |field| {
        let start = 5 + field * field_bits;
        (start..start + field_bits).fold(0_u64, |value, bit| {
            let next = (bytes[bit / 8] >> (7 - bit % 8)) & 1;
            (value << 1) | u64::from(next)
        })
    };
    // PHP subtracts the unsigned fields before converting twips to pixels.
    let size = |min, max| (coordinate(max).wrapping_sub(coordinate(min)) / 20) as u32;
    (size(0, 1), size(2, 3))
}
