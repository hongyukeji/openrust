use ::image::Frame;
use oxideav_heif::{
    HeifFile, HeifWriter, SequenceAlphaTrack, SequenceWriter, props::Property,
    sequence::LoopBehaviour, writer::SequenceSample,
};

use super::{Error, Format, Result, animation, codecs, metadata::Metadata};

pub(super) fn encode(
    frames: Vec<Frame>,
    quality: u8,
    plays: u32,
    metadata: &Metadata,
) -> Result<Vec<u8>> {
    let error = |error| codecs::codec_error(Format::Avif, error);
    let (width, height) = frames[0].buffer().dimensions();
    let width = u16::try_from(width).map_err(|_| Error::DimensionsTooLarge)?;
    let height = u16::try_from(height).map_err(|_| Error::DimensionsTooLarge)?;
    let has_alpha = frames
        .iter()
        .any(|f| f.buffer().pixels().any(|p| p[3] != 255));
    let encoder = ravif::Encoder::new()
        .with_speed(8)
        .with_quality(f32::from(quality))
        .with_num_threads(Some(1));
    let mut sequence: Option<SequenceWriter> = None;
    let mut duration = 0u64;
    for frame in frames {
        let delay = u32::try_from(animation::ticks(frame.delay(), 1))
            .map_err(|_| Error::InvalidAnimationTiming)?;
        duration += u64::from(delay);
        let pixels = frame.buffer();
        let still = encoder
            .encode_raw_planes_8_bit(
                usize::from(width),
                usize::from(height),
                pixels.pixels().map(|p| [p[1], p[2], p[0]]),
                has_alpha.then(|| pixels.pixels().map(|p| p[3])),
                ravif::PixelRange::Full,
                ravif::MatrixCoefficients::Identity,
            )
            .map_err(|error| codecs::codec_error(Format::Avif, error))?;
        let file = HeifFile::parse_borrowed(&still.avif_file).map_err(error)?;
        let meta = file.meta().map_err(error)?;
        let id = file.primary_item().map_err(error)?.id;
        let (config, mut properties) = properties_of(&file, id)?;
        add_icc(&mut properties, metadata);
        let data = file.item_data_owned(id).map_err(error)?;
        let alpha = meta
            .auxiliaries_of(id)
            .into_iter()
            .next()
            .map(|id| -> Result<_> {
                let (config, properties) = properties_of(&file, id)?;
                let data = file.item_data_owned(id).map_err(error)?;
                Ok((config, properties, data))
            })
            .transpose()?;
        if has_alpha && alpha.is_none() {
            return Err(Error::UnsupportedFormat);
        }
        let writer = sequence.get_or_insert_with(|| {
            let mut writer = SequenceWriter::new(*b"av01", config.clone(), width, height, 1000);
            writer.entry_properties = entry_properties(&properties);
            let mut cover = HeifWriter::new();
            let master = cover.add_coded_item(*b"av01", Vec::new(), properties);
            cover.set_primary(master);
            add_metadata(&mut cover, master, metadata);
            if let Some((config, properties, data)) = &alpha {
                let id = cover.add_coded_item(*b"av01", data.clone(), properties.clone());
                cover.add_reference(*b"auxl", id, vec![master]);
                writer.alpha = Some(SequenceAlphaTrack::new(
                    *b"av01",
                    config.clone(),
                    width,
                    height,
                    oxideav_heif::props::AUX_URN_ALPHA.to_owned(),
                    Vec::new(),
                    entry_properties(properties),
                ));
            }
            writer.exif = metadata.exif.clone();
            writer.xmp = metadata.xmp.clone();
            writer.still = Some(cover);
            writer.cover_sample = Some(0);
            writer
        });
        if writer.config != config {
            return Err(Error::UnsupportedFormat);
        }
        writer.push_sample(data, delay, true);
        if let Some((config, _, data)) = alpha {
            let track = writer.alpha.as_mut().ok_or(Error::UnsupportedFormat)?;
            if track.config != config {
                return Err(Error::UnsupportedFormat);
            }
            track.samples.push(SequenceSample::new(data, delay, true));
        }
    }
    if plays != 0
        && (duration.saturating_mul(u64::from(plays)) >= u64::from(u32::MAX)
            || duration == 0 && plays > 1)
    {
        return Err(Error::InvalidAnimationTiming);
    }
    let loops = if plays == 0 {
        LoopBehaviour::Forever
    } else {
        LoopBehaviour::Times(u64::from(plays))
    };
    sequence
        .ok_or(Error::InvalidDimensions)?
        .with_looping(loops)
        .write_to_vec()
        .map_err(error)
}

fn properties_of(file: &HeifFile<&[u8]>, id: u32) -> Result<(Property, Vec<(Property, bool)>)> {
    let error = |error| codecs::codec_error(Format::Avif, error);
    let meta = file.meta().map_err(error)?;
    let mut config = None;
    let mut properties = Vec::new();
    for (_, raw, essential) in meta.properties_of(id) {
        let property = Property::parse(raw).map_err(error)?;
        if matches!(property, Property::Av1C(_)) {
            config = Some(property.clone());
        }
        properties.push((property, essential));
    }
    Ok((config.ok_or(Error::UnsupportedFormat)?, properties))
}

fn entry_properties(properties: &[(Property, bool)]) -> Vec<Property> {
    properties
        .iter()
        .filter_map(|(property, _)| {
            matches!(property, Property::Pixi(_) | Property::Colr(_)).then_some(property.clone())
        })
        .collect()
}

pub(super) fn embed_still(bytes: Vec<u8>, metadata: &Metadata) -> Result<Vec<u8>> {
    if metadata.is_empty() {
        return Ok(bytes);
    }
    let error = |error| codecs::codec_error(Format::Avif, error);
    let file = HeifFile::parse_borrowed(&bytes).map_err(error)?;
    let meta = file.meta().map_err(error)?;
    let id = file.primary_item().map_err(error)?.id;
    let mut writer = HeifWriter::new();
    let (_, mut properties) = properties_of(&file, id)?;
    add_icc(&mut properties, metadata);
    let primary = writer.add_coded_item(
        *b"av01",
        file.item_data_owned(id).map_err(error)?,
        properties,
    );
    writer.set_primary(primary);
    for id in meta.auxiliaries_of(id) {
        let (_, properties) = properties_of(&file, id)?;
        let child = writer.add_coded_item(
            *b"av01",
            file.item_data_owned(id).map_err(error)?,
            properties,
        );
        writer.add_reference(*b"auxl", child, vec![primary]);
    }
    add_metadata(&mut writer, primary, metadata);
    writer.write_to_vec().map_err(error)
}

fn add_icc(properties: &mut Vec<(Property, bool)>, metadata: &Metadata) {
    if let Some(profile) = &metadata.icc {
        properties.push((
            Property::Colr(oxideav_heif::props::Colr::Icc {
                restricted: false,
                profile: profile.clone(),
            }),
            false,
        ));
    }
}

fn add_metadata(writer: &mut HeifWriter, id: u32, metadata: &Metadata) {
    if let Some(exif) = &metadata.exif {
        writer.add_exif(id, exif);
    }
    if let Some(xmp) = &metadata.xmp {
        writer.add_xmp_bytes(id, xmp.clone());
    }
}
