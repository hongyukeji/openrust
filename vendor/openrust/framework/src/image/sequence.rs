use std::time::Duration;

use ::image::{Delay, Frame, RgbaImage};
use oxideav_heif::{
    DecodeOptions, EncodeOptions, HeifFile, HeifImage, StillCodec,
    boxes::{find_box, iter_boxes, payload},
    sequence::{LoopBehaviour, parse_movie},
};

use super::{
    Error, Format, Limits, Result,
    animation::{self, Budget, Decoded},
    codecs,
};

pub(super) fn decode(bytes: &[u8], format: Format, limits: Limits) -> Result<Option<Decoded<'_>>> {
    let error = move |error| codecs::codec_error(format, error);
    // Still images stay on their existing codec paths.
    let Some((_, movie_bytes)) = find_box(bytes, b"moov").map_err(error)? else {
        return Ok(None);
    };
    check_sample_tables(movie_bytes, format)?;
    let file = HeifFile::parse_borrowed(bytes).map_err(error)?;
    let movie = parse_movie(&file)
        .map_err(error)?
        .ok_or(Error::UnsupportedFormat)?;
    let mut tracks = movie
        .visual_tracks()
        .filter(|track| &track.handler != b"auxv" && track.aux_kind().is_none());
    let track = tracks.next().ok_or(Error::UnsupportedFormat)?;
    if tracks.next().is_some() {
        return Err(Error::UnsupportedFormat);
    }

    let mut budget = Budget::new(limits);
    for track in movie.visual_tracks() {
        let entry = track.primary_entry().ok_or(Error::UnsupportedFormat)?;
        let dimensions = (u32::from(entry.width), u32::from(entry.height));
        limits.dimensions(dimensions.0, dimensions.1)?;
        for _ in &track.samples {
            budget.add(dimensions)?;
        }
    }
    // The backend visits still items as well as timed tracks. Bound the cover work.
    if let Ok(meta) = file.meta() {
        let primary = file.primary_item().map_err(error)?;
        if meta
            .display_order()
            .iter()
            .any(|alternatives| alternatives.as_slice() != [primary.id])
        {
            return Err(Error::UnsupportedFormat);
        }
    }
    let plays = match track.loop_behaviour() {
        LoopBehaviour::Forever => 0,
        LoopBehaviour::Times(count) => {
            u32::try_from(count).map_err(|_| Error::InvalidAnimationTiming)?
        }
        _ => return Err(Error::InvalidAnimationTiming),
    };
    let options = DecodeOptions::default()
        .with_max_width(Some(limits.max_dimension))
        .with_max_height(Some(limits.max_dimension))
        .with_max_pixels(Some(limits.max_pixels))
        .with_max_bytes(Some(limits.max_input_bytes as u64));
    let decoded = oxideav_heif::decode_all_with(bytes, &options).map_err(error)?;
    let expected_track = track.track_id;
    let mut pictures = decoded
        .into_iter()
        .filter(move |frame| frame.track_id == Some(expected_track))
        .peekable();
    let first = pictures.peek().ok_or(Error::InvalidDimensions)?;
    let dimensions = (first.image.width(), first.image.height());
    let frames = pictures.map(move |picture| {
        let pixels = picture.image.try_to_rgba8().map_err(error)?;
        let width = picture.image.width();
        let height = picture.image.height();
        if (width, height) != dimensions {
            return Err(Error::InvalidDimensions);
        }
        let image = RgbaImage::from_raw(width, height, pixels).ok_or(Error::InvalidDimensions)?;
        let delay = picture.delay.ok_or(Error::InvalidAnimationTiming)?;
        Ok(Frame::from_parts(
            image,
            0,
            0,
            Delay::from_saturating_duration(delay),
        ))
    });
    Ok(Some(Decoded {
        frames: Box::new(frames),
        dimensions,
        orientation: None,
        plays,
    }))
}

fn check_sample_tables(movie: &[u8], format: Format) -> Result<()> {
    let error = |error| codecs::codec_error(format, error);
    for track in iter_boxes(movie) {
        let track = track.map_err(error)?;
        if &track.box_type != b"trak" {
            continue;
        }
        let mut table = payload(movie, &track);
        for name in [b"mdia", b"minf", b"stbl"] {
            table = find_box(table, name)
                .map_err(error)?
                .ok_or(Error::UnsupportedFormat)?
                .1;
        }
        for table_header in iter_boxes(table) {
            let table_header = table_header.map_err(error)?;
            let table = payload(table, &table_header);
            let count = match &table_header.box_type {
                b"stsz" | b"stz2" => u64::from(read_u32(table, 8)?),
                b"stts" | b"ctts" => {
                    let count = read_u32(table, 4)? as usize;
                    let entries = table.get(8..).ok_or(Error::UnsupportedFormat)?;
                    if entries.len() / 8 != count || entries.len() % 8 != 0 {
                        return Err(Error::UnsupportedFormat);
                    }
                    let mut total = 0u64;
                    for entry in entries.chunks_exact(8) {
                        total += u64::from(read_u32(entry, 0)?);
                        if total > animation::MAX_FRAMES as u64 {
                            return Err(Error::AnimationTooLarge);
                        }
                    }
                    total
                }
                _ => continue,
            };
            if count > animation::MAX_FRAMES as u64 {
                return Err(Error::AnimationTooLarge);
            }
        }
    }
    Ok(())
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32> {
    let value = bytes
        .get(offset..offset + 4)
        .ok_or(Error::UnsupportedFormat)?;
    Ok(u32::from_be_bytes(
        value.try_into().expect("slice has four bytes"),
    ))
}

pub(super) fn encode(
    frames: Vec<Frame>,
    format: Format,
    quality: u8,
    plays: u32,
    metadata: &super::metadata::Metadata,
) -> Result<Vec<u8>> {
    let error = move |error| codecs::codec_error(format, error);
    let options = EncodeOptions::default()
        .with_codec(StillCodec::Hevc)
        .with_qp(((100 - u32::from(quality)) * 51 / 100) as u8)
        .with_icc_profile(metadata.icc.clone())
        .with_exif(metadata.exif.clone())
        .with_xmp(
            metadata
                .xmp
                .as_ref()
                .and_then(|xmp| String::from_utf8(xmp.clone()).ok()),
        );
    let mut segment = 0u64;
    let has_alpha = frames
        .iter()
        .any(|frame| frame.buffer().pixels().any(|pixel| pixel[3] != 255));
    let mut frames = frames
        .into_iter()
        .map(|frame| {
            let ticks = animation::ticks(frame.delay(), 1);
            segment += ticks;
            let buffer = frame.into_buffer();
            let (width, height) = buffer.dimensions();
            let image = if has_alpha {
                HeifImage::from_rgba8(width, height, buffer.into_raw())
            } else {
                let rgb = ::image::DynamicImage::ImageRgba8(buffer).into_rgb8();
                HeifImage::from_rgb8(width, height, rgb.into_raw())
            }
            .map_err(error)?;
            Ok(oxideav_heif::Frame::new(
                image,
                Some(Duration::from_millis(ticks)),
                None,
                None,
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    if !metadata.is_empty() {
        frames.push(oxideav_heif::Frame::new(
            frames[0].image.clone(),
            None,
            None,
            None,
        ));
    }
    let mut bytes = oxideav_heif::encode_all(&frames, &options).map_err(error)?;
    write_play_count(&mut bytes, segment, plays, format)?;
    Ok(bytes)
}

fn write_play_count(bytes: &mut Vec<u8>, segment: u64, plays: u32, format: Format) -> Result<()> {
    let error = move |error| codecs::codec_error(format, error);
    let (header, movie) = find_box(bytes, b"moov")
        .map_err(error)?
        .ok_or(Error::UnsupportedFormat)?;
    let segment = u32::try_from(segment).map_err(|_| Error::InvalidAnimationTiming)?;
    let duration = if plays == 0 {
        u32::MAX
    } else {
        segment
            .checked_mul(plays)
            .filter(|&duration| duration < u32::MAX)
            .ok_or(Error::InvalidAnimationTiming)?
    };
    if segment == 0 && plays > 1 {
        return Err(Error::InvalidAnimationTiming);
    }
    // Write the replacement movie after the media. The free box retains all original sample offsets.
    let mut replacement = Vec::new();
    for child in iter_boxes(movie) {
        let child = child.map_err(error)?;
        let mut body = payload(movie, &child).to_vec();
        match &child.box_type {
            b"mvhd" => set_duration(&mut body, 16, duration)?,
            b"trak" => {
                let (track, _) = find_box(&body, b"tkhd")
                    .map_err(error)?
                    .ok_or(Error::UnsupportedFormat)?;
                set_duration(&mut body[track.payload_start..track.end()], 20, duration)?;
                let mut edit = vec![0, 0, 0, u8::from(plays != 1)];
                edit.extend_from_slice(&1u32.to_be_bytes());
                edit.extend_from_slice(&segment.to_be_bytes());
                edit.extend_from_slice(&0u32.to_be_bytes());
                edit.extend_from_slice(&0x10000u32.to_be_bytes());
                let mut edits = Vec::new();
                push_box(&mut edits, b"elst", &edit)?;
                push_box(&mut body, b"edts", &edits)?;
            }
            _ => {}
        }
        push_box(&mut replacement, &child.box_type, &body)?;
    }
    bytes[header.start + 4..header.start + 8].copy_from_slice(b"free");
    push_box(bytes, b"moov", &replacement)
}

fn set_duration(bytes: &mut [u8], offset: usize, duration: u32) -> Result<()> {
    // SequenceWriter emits version-zero movie and track headers.
    if bytes.first() != Some(&0) {
        return Err(Error::UnsupportedFormat);
    }
    bytes
        .get_mut(offset..offset + 4)
        .ok_or(Error::InvalidDimensions)?
        .copy_from_slice(&duration.to_be_bytes());
    Ok(())
}

fn push_box(output: &mut Vec<u8>, name: &[u8; 4], body: &[u8]) -> Result<()> {
    let size = u32::try_from(body.len() + 8).map_err(|_| Error::AnimationTooLarge)?;
    output.extend_from_slice(&size.to_be_bytes());
    output.extend_from_slice(name);
    output.extend_from_slice(body);
    Ok(())
}
