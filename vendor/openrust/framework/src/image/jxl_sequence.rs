use ::image::{Frame, Rgba, RgbaImage};
use jxl_oxide::{AllocTracker, InitializeResult, JxlImage, PixelFormat};

use super::{
    Error, Format, Limits, Result,
    animation::{self, Budget, Decoded},
    codecs,
};

pub(super) fn decode(contents: &[u8], limits: Limits) -> Result<Option<Decoded<'_>>> {
    let error = |source| Error::Codec {
        format: Format::Jxl,
        source,
    };
    let (mut image, mut remaining) = initialize(contents, limits)?;
    let Some(timing) = image.image_header().metadata.animation.as_ref() else {
        return Ok(None);
    };
    let (numerator, denominator, plays) = (
        timing.tps_numerator,
        timing.tps_denominator,
        timing.num_loops,
    );
    let dimensions = (image.width(), image.height());
    limits.dimensions(dimensions.0, dimensions.1)?;
    let pixel_format = image.pixel_format();
    if pixel_format.has_black() {
        return Err(Error::UnsupportedFormat);
    }
    let mut budget = Budget::new(limits);
    let mut accounted = 0;
    while !image.is_loading_done() && !remaining.is_empty() {
        let consumed = image.feed_bytes(remaining).map_err(error)?;
        let frames = image.num_loaded_keyframes();
        for _ in accounted..frames {
            budget.add(dimensions)?;
        }
        accounted = frames;
        if consumed == 0 {
            break;
        }
        remaining = &remaining[consumed..];
    }
    image.finalize().map_err(error)?;
    if !image.is_loading_done() {
        return Err(Error::InvalidDimensions);
    }
    let count = image.num_loaded_keyframes();
    for _ in accounted..count {
        budget.add(dimensions)?;
    }
    let frames = (0..count).map(move |index| {
        let render = image.render_frame(index).map_err(error)?;
        let mut stream = render.stream();
        let (width, height) = dimensions;
        let mut samples = vec![0u8; width as usize * height as usize * pixel_format.channels()];
        if stream.write_to_buffer(&mut samples) != samples.len() {
            return Err(Error::InvalidDimensions);
        }
        let mut buffer = RgbaImage::new(width, height);
        for (output, input) in buffer
            .pixels_mut()
            .zip(samples.chunks_exact(pixel_format.channels()))
        {
            *output = Rgba(match pixel_format {
                PixelFormat::Gray => [input[0], input[0], input[0], 255],
                PixelFormat::Graya => [input[0], input[0], input[0], input[1]],
                PixelFormat::Rgb => [input[0], input[1], input[2], 255],
                PixelFormat::Rgba => [input[0], input[1], input[2], input[3]],
                _ => return Err(Error::UnsupportedFormat),
            });
        }
        let delay = animation::rational_delay(render.duration(), numerator, denominator)?;
        Ok(Frame::from_parts(buffer, 0, 0, delay))
    });
    Ok(Some(Decoded {
        frames: Box::new(frames),
        dimensions,
        orientation: None,
        plays,
    }))
}

fn initialize(contents: &[u8], limits: Limits) -> Result<(JxlImage, &[u8])> {
    let error = |source| Error::Codec {
        format: Format::Jxl,
        source,
    };
    let memory = usize::try_from(limits.max_pixels.saturating_mul(48)).unwrap_or(usize::MAX);
    let mut pending = JxlImage::builder()
        .alloc_tracker(AllocTracker::with_limit(memory))
        .build_uninit();
    let mut remaining = contents;
    let mut size = 4096;
    let image = loop {
        let chunk = &remaining[..remaining.len().min(size)];
        let consumed = pending.feed_bytes(chunk).map_err(error)?;
        remaining = &remaining[consumed..];
        match pending.try_init().map_err(error)? {
            InitializeResult::Initialized(image) => break image,
            InitializeResult::NeedMoreData(next) if !remaining.is_empty() && !chunk.is_empty() => {
                if consumed < chunk.len() {
                    size = size.saturating_add(4096);
                    if size > remaining.len() && consumed == 0 && chunk.len() == remaining.len() {
                        return Err(Error::InvalidDimensions);
                    }
                }
                pending = next;
            }
            _ => return Err(Error::InvalidDimensions),
        }
    };
    Ok((image, remaining))
}

pub(super) fn metadata(contents: &[u8], limits: Limits) -> Result<super::metadata::Metadata> {
    use oxideav_jpegxl::container::{JxlFile, MetadataKind};
    let (image, _) = initialize(contents, limits)?;
    let mut metadata = super::metadata::Metadata {
        icc: image.original_icc().map(<[u8]>::to_vec),
        ..Default::default()
    };
    if contents.starts_with(&[0xff, 0x0a]) {
        return Ok(metadata);
    }
    let error = |error| codecs::codec_error(Format::Jxl, error);
    let file = JxlFile::parse(contents).map_err(error)?;
    for item in file.metadata {
        match item.kind {
            MetadataKind::Exif if metadata.exif.is_none() => {
                let bytes = item.content(limits.max_input_bytes).map_err(error)?;
                metadata.exif = oxideav_heif::meta::exif_payload(&bytes).ok();
            }
            MetadataKind::Xml if metadata.xmp.is_none() => {
                metadata.xmp = Some(
                    item.content(limits.max_input_bytes)
                        .map_err(error)?
                        .into_owned(),
                );
            }
            _ => {}
        }
    }
    Ok(metadata)
}

pub(super) fn encode(
    frames: Vec<Frame>,
    quality: u8,
    plays: u32,
    metadata: &super::metadata::Metadata,
) -> Result<Vec<u8>> {
    use oxideav_jpegxl::encoder::{
        AnimationSpec, ColourSpec, EncodeSpec, FrameInput, encode_codestream,
    };
    let (width, height) = frames[0].buffer().dimensions();
    let spec = EncodeSpec {
        width,
        height,
        bits: 8,
        grey: false,
        alpha: true,
        colour: match &metadata.icc {
            Some(icc) => ColourSpec::Icc(icc.clone()),
            None => ColourSpec::Enumerated {
                primaries: 1,
                transfer: 13,
            },
        },
        animation: Some(AnimationSpec {
            tps_numerator: 1000,
            tps_denominator: 1,
            num_loops: plays,
        }),
        quality,
        group_size_shift: 1,
    };
    let frames = frames
        .into_iter()
        .map(|frame| {
            // Zero-tick JPEG XL frames are references and do not display.
            let duration = u32::try_from(animation::ticks(frame.delay(), 1).max(1))
                .map_err(|_| Error::InvalidAnimationTiming)?;
            let mut channels: Vec<_> = (0..4)
                .map(|_| Vec::with_capacity(width as usize * height as usize))
                .collect();
            for pixel in frame.buffer().pixels() {
                for (channel, sample) in channels.iter_mut().zip(pixel.0) {
                    channel.push(i32::from(sample));
                }
            }
            Ok(FrameInput { channels, duration })
        })
        .collect::<Result<Vec<_>>>()?;
    let error = |error| codecs::codec_error(Format::Jxl, error);
    let bytes = encode_codestream(&spec, &frames).map_err(error)?;
    if metadata.exif.is_some() || metadata.xmp.is_some() {
        oxideav_jpegxl::encoder::wrap_container(
            &bytes,
            metadata.exif.as_deref(),
            metadata.xmp.as_deref(),
        )
        .map_err(error)
    } else {
        Ok(bytes)
    }
}
