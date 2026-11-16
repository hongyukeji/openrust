use std::sync::Arc;

use ::image::{Delay, Frame, RgbaImage};
use oxideav_heif::{
    DecodeOptions, HeifFile,
    boxes::{find_box, iter_boxes, payload},
    props::Colr,
    sequence::{
        LoopBehaviour, MetadataTrackKind, Movie, SampleEntry, Track, parse_movie, sample_bytes,
    },
};

use super::{
    Error, Format, Limits, Result,
    animation::{self, Budget, Decoded},
    codecs,
    metadata::{FrameMetadata, Metadata},
};

pub(super) fn decode(bytes: &[u8], format: Format, limits: Limits) -> Result<Option<Decoded<'_>>> {
    let error = move |error| codecs::codec_error(format, error);
    let file = HeifFile::parse_borrowed(bytes).map_err(error)?;
    let Some(movie) = read_movie(&file, bytes, format)? else {
        return Ok(None);
    };
    let track = primary_track(&movie)?;

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
    let metadata = read_track_metadata(&file, &movie, track, format, limits)?;
    let options = DecodeOptions::default()
        .with_max_width(Some(limits.max_dimension))
        .with_max_height(Some(limits.max_dimension))
        .with_max_pixels(Some(limits.max_pixels))
        .with_max_bytes(Some(limits.max_input_bytes as u64));
    let decoded = oxideav_heif::decode_all_with(bytes, &options).map_err(error)?;
    let expected_track = track.track_id;
    let pictures: Vec<_> = decoded
        .into_iter()
        .filter(|frame| frame.track_id == Some(expected_track))
        .collect();
    if pictures.len() != track.samples.len() {
        return Err(Error::InvalidDimensions);
    }
    let first = pictures.first().ok_or(Error::InvalidDimensions)?;
    let dimensions = (first.image.width(), first.image.height());
    let frames = pictures.into_iter().map(move |picture| {
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
        metadata,
        plays,
    }))
}

fn primary_track(movie: &Movie) -> Result<&Track> {
    let mut tracks = movie
        .visual_tracks()
        .filter(|track| &track.handler != b"auxv" && track.aux_kind().is_none());
    let track = tracks.next().ok_or(Error::UnsupportedFormat)?;
    if tracks.next().is_some() {
        return Err(Error::UnsupportedFormat);
    }
    Ok(track)
}

fn read_movie(file: &HeifFile<&[u8]>, bytes: &[u8], format: Format) -> Result<Option<Movie>> {
    let error = |error| codecs::codec_error(format, error);
    let Some((_, body)) = find_box(bytes, b"moov").map_err(error)? else {
        return Ok(None);
    };
    check_sample_tables(body, format)?;
    parse_movie(file)
        .map_err(error)?
        .map(Some)
        .ok_or(Error::UnsupportedFormat)
}

fn read_track_metadata(
    file: &HeifFile<&[u8]>,
    movie: &Movie,
    track: &Track,
    format: Format,
    limits: Limits,
) -> Result<Metadata> {
    let primary = track.primary_entry().ok_or(Error::UnsupportedFormat)?;
    let (exif, xmp) = track.sequence_metadata(file);
    let mut metadata = Metadata {
        icc: icc(primary).map(<[u8]>::to_vec),
        exif,
        xmp,
        ..Metadata::default()
    };
    // The backend attaches a copy of the track metadata to each decoded picture.
    let shared = [
        metadata.icc.as_ref(),
        metadata.exif.as_ref(),
        metadata.xmp.as_ref(),
    ]
    .into_iter()
    .flatten()
    .map(Vec::len)
    .sum::<usize>();
    let mut remaining = limits
        .max_input_bytes
        .checked_sub(shared.saturating_mul(track.samples.len()))
        .ok_or(Error::InputTooLarge)?;

    let error = |error| codecs::codec_error(format, error);
    let timed: Vec<_> = movie
        .metadata_tracks_of(track.track_id)
        .into_iter()
        .filter_map(|track| track.metadata_kind().map(|kind| (track, kind)))
        .collect();
    let mut pictures: Vec<_> = track.samples.iter().collect();
    pictures.sort_by_key(|sample| sample.pts());
    let mut profiles = vec![None; track.sample_entries.len()];
    for (index, picture) in pictures.iter().enumerate() {
        let description = picture
            .description_index
            .checked_sub(1)
            .ok_or(Error::UnsupportedFormat)? as usize;
        let entry = track
            .sample_entries
            .get(description)
            .ok_or(Error::UnsupportedFormat)?;
        let profile = icc(entry);
        let mut frame = FrameMetadata::default();
        // The backend uses one visual configuration for the whole track.
        if description != 0 && !same_configuration(entry, primary) {
            return Err(Error::UnsupportedFormat);
        }
        if profile != icc(primary) {
            frame.icc = Some(
                profiles[description]
                    .get_or_insert_with(|| profile.map(Arc::from))
                    .clone(),
            );
        }
        for &(timeline, kind) in &timed {
            let Some(at) = timeline.sample_index_at(picture.pts(), track.timescale) else {
                continue;
            };
            // Prefer one-to-one alignment for parallel timelines with the same
            // timestamp (zero-duration samples may share a presentation time).
            let backend_sample = &timeline.samples[at];
            let aligned = timeline.timescale == track.timescale
                && timeline.samples.len() == pictures.len()
                && timeline.samples[index].dts == picture.pts()
                && timeline.samples[index].duration == picture.duration;
            let sample = if aligned {
                &timeline.samples[index]
            } else {
                backend_sample
            };
            remaining = remaining
                .checked_sub(sample.size.max(backend_sample.size) as usize)
                .ok_or(Error::InputTooLarge)?;
            if !aligned
                && u128::from(picture.pts()) * u128::from(timeline.timescale.max(1))
                    >= (u128::from(sample.dts) + u128::from(sample.duration))
                        * u128::from(track.timescale.max(1))
            {
                continue;
            }
            let data = sample_bytes(file, sample).map_err(error)?;
            if data.is_empty() {
                continue;
            }
            match kind {
                MetadataTrackKind::Exif if data.len() > 4 => {
                    frame.exif = Some(oxideav_heif::meta::exif_payload(data).map_err(error)?);
                }
                MetadataTrackKind::Xmp => frame.xmp = Some(data.to_vec()),
                _ => {}
            }
        }
        metadata.frames.push(frame);
    }
    // A cover-only profile must not become the default for unprofiled samples
    // of a track that explicitly changes profiles.
    if metadata.icc.is_none() && metadata.frames.iter().any(|frame| frame.icc.is_some()) {
        for frame in &mut metadata.frames {
            frame.icc.get_or_insert(None);
        }
    }
    if metadata
        .frames
        .iter()
        .all(|f| f.icc.is_none() && f.exif.is_none() && f.xmp.is_none())
    {
        metadata.frames.clear();
    }
    Ok(metadata)
}

fn icc(entry: &SampleEntry) -> Option<&[u8]> {
    entry.colr.iter().find_map(|color| match color {
        Colr::Icc { profile, .. } => Some(profile.as_slice()),
        _ => None,
    })
}

fn same_configuration(entry: &SampleEntry, primary: &SampleEntry) -> bool {
    entry.entry_type == primary.entry_type
        && entry.data_reference_index == primary.data_reference_index
        && entry.width == primary.width
        && entry.height == primary.height
        && entry.hvcc == primary.hvcc
        && entry.av1c == primary.av1c
        && entry.avcc == primary.avcc
        && entry.lhvc == primary.lhvc
        && entry.vvcc == primary.vvcc
        && entry.clap == primary.clap
        && entry
            .colr
            .iter()
            .filter(|c| !c.is_icc())
            .eq(primary.colr.iter().filter(|c| !c.is_icc()))
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
