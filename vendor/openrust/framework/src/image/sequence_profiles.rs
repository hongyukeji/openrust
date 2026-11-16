use std::collections::HashMap;

use oxideav_heif::{
    HeifFile,
    boxes::{BoxHeader, find_box, iter_boxes, payload},
    sequence::{Sample, parse_movie},
};

use super::{Error, Format, Result, codecs, metadata::Metadata};

pub(super) fn write(bytes: &mut Vec<u8>, metadata: &Metadata, format: Format) -> Result<()> {
    let map = |error| codecs::codec_error(format, error);
    let file = HeifFile::parse_borrowed(bytes.as_slice()).map_err(map)?;
    let movie = parse_movie(&file)
        .map_err(map)?
        .ok_or(Error::UnsupportedFormat)?;
    // SequenceWriter assigns track 1 to the main image sequence and writes it first.
    let track = movie.track(1).ok_or(Error::UnsupportedFormat)?;
    let (header, body) = child(bytes, b"moov", format)?;
    let mut replacement = body.to_vec();
    change_child(&mut replacement, b"trak", format, |track_box| {
        change_child(track_box, b"mdia", format, |media| {
            change_child(media, b"minf", format, |info| {
                change_child(info, b"stbl", format, |table| {
                    write_table(table, &track.samples, metadata, format)
                })
            })
        })
    })?;
    // Keep the original media and item offsets, including those of the metadata tracks.
    bytes[header.start + 4..header.start + 8].copy_from_slice(b"free");
    push_box(bytes, b"moov", &replacement)
}

fn write_table(
    table: &mut Vec<u8>,
    samples: &[Sample],
    metadata: &Metadata,
    format: Format,
) -> Result<()> {
    if samples.len() != metadata.frames.len() {
        return Err(Error::InvalidDimensions);
    }
    let (_, descriptions) = child(table, b"stsd", format)?;
    let description = descriptions.get(8..).ok_or(Error::UnsupportedFormat)?;
    let mut entries = description.to_vec();
    let mut indexes = HashMap::from([(metadata.icc.as_deref(), 1u32)]);
    let mut runs = Vec::<(u32, u32)>::new();
    let mut offsets = Vec::with_capacity(samples.len());
    for (sample, frame) in samples.iter().zip(&metadata.frames) {
        let profile = match &frame.icc {
            Some(override_profile) => override_profile.as_deref(),
            None => metadata.icc.as_deref(),
        };
        let next = u32::try_from(indexes.len() + 1).map_err(|_| Error::AnimationTooLarge)?;
        let index = match indexes.entry(profile) {
            std::collections::hash_map::Entry::Occupied(entry) => *entry.get(),
            std::collections::hash_map::Entry::Vacant(entry) => {
                add_description(&mut entries, description, profile, format)?;
                *entry.insert(next)
            }
        };
        // Use one chunk per sample, so a switch of description needs no sample data relocation.
        offsets.push(sample.offset);
        let chunk = offsets.len() as u32;
        if runs.last().is_none_or(|&(_, previous)| previous != index) {
            runs.push((chunk, index));
        }
    }
    change_child(table, b"stsd", format, |box_body| {
        *box_body = counted(indexes.len());
        box_body.extend(entries);
        Ok(())
    })?;
    change_child(table, b"stsc", format, |box_body| {
        *box_body = counted(runs.len());
        for (first_chunk, index) in runs {
            box_body.extend(first_chunk.to_be_bytes());
            box_body.extend(1u32.to_be_bytes()); // samples per chunk
            box_body.extend(index.to_be_bytes());
        }
        Ok(())
    })?;
    // The original writer selects the offset width using the first sample;
    // later samples in the new table can cross that width's boundary.
    let previous = if find_box(table, b"stco")
        .map_err(|e| codecs::codec_error(format, e))?
        .is_some()
    {
        b"stco"
    } else {
        b"co64"
    };
    let (header, _) = child(table, previous, format)?;
    let use_32 = offsets.iter().all(|&offset| u32::try_from(offset).is_ok());
    let mut body = counted(offsets.len());
    for offset in offsets {
        if use_32 {
            body.extend((offset as u32).to_be_bytes());
        } else {
            body.extend(offset.to_be_bytes());
        }
    }
    let mut replacement = Vec::new();
    push_box(
        &mut replacement,
        if use_32 { b"stco" } else { b"co64" },
        &body,
    )?;
    table.splice(header.start..header.end(), replacement);
    Ok(())
}

fn add_description(
    entries: &mut Vec<u8>,
    description: &[u8],
    profile: Option<&[u8]>,
    format: Format,
) -> Result<()> {
    let map = |error| codecs::codec_error(format, error);
    let header = iter_boxes(description)
        .next()
        .ok_or(Error::UnsupportedFormat)?
        .map_err(map)?;
    let entry = payload(description, &header);
    // ISO/IEC 14496-12 VisualSampleEntry's fixed-size header.
    let (header_bytes, properties) = entry.split_at_checked(78).ok_or(Error::UnsupportedFormat)?;
    let mut body = header_bytes.to_vec();
    for property in iter_boxes(properties) {
        let property = property.map_err(map)?;
        let is_icc = &property.box_type == b"colr"
            && matches!(
                payload(properties, &property).get(..4),
                Some(b"prof" | b"rICC")
            );
        if !is_icc {
            body.extend_from_slice(&properties[property.start..property.end()]);
        }
    }
    if let Some(profile) = profile {
        let mut color = b"prof".to_vec();
        color.extend_from_slice(profile);
        push_box(&mut body, b"colr", &color)?;
    }
    push_box(entries, &header.box_type, &body)
}

fn child<'a>(parent: &'a [u8], name: &[u8; 4], format: Format) -> Result<(BoxHeader, &'a [u8])> {
    find_box(parent, name)
        .map_err(|error| codecs::codec_error(format, error))?
        .ok_or(Error::UnsupportedFormat)
}

fn change_child(
    parent: &mut Vec<u8>,
    name: &[u8; 4],
    format: Format,
    change: impl FnOnce(&mut Vec<u8>) -> Result<()>,
) -> Result<()> {
    let (header, body) = child(parent, name, format)?;
    let mut body = body.to_vec();
    change(&mut body)?;
    let mut replacement = Vec::new();
    push_box(&mut replacement, name, &body)?;
    parent.splice(header.start..header.end(), replacement);
    Ok(())
}

fn counted(count: usize) -> Vec<u8> {
    let mut body = vec![0; 4]; // full box version and flags
    body.extend((count as u32).to_be_bytes());
    body
}

fn push_box(output: &mut Vec<u8>, kind: &[u8; 4], body: &[u8]) -> Result<()> {
    let size = u32::try_from(body.len() + 8).map_err(|_| Error::AnimationTooLarge)?;
    output.extend(size.to_be_bytes());
    output.extend(kind);
    output.extend(body);
    Ok(())
}
