use std::io::Cursor;

use ::image::{
    AnimationDecoder, Delay, DynamicImage, Frame, ImageDecoder,
    codecs::{gif::GifDecoder, png::PngDecoder, webp::WebPDecoder},
    metadata::{LoopCount, Orientation},
};
use oxideav_webp::{build, container::fourcc};

use super::{Error, Format, Limits, Result, codecs};

pub(super) const MAX_FRAMES: usize = 1024;
const MAX_PIXELS: u64 = 96_000_000;

pub(super) struct Budget {
    frames: usize,
    remaining: u64,
}

impl Budget {
    pub(super) fn new(limits: Limits) -> Self {
        Self {
            frames: 0,
            remaining: limits.max_pixels.saturating_mul(4).min(MAX_PIXELS),
        }
    }

    pub(super) fn add(&mut self, (width, height): (u32, u32)) -> Result<()> {
        if self.frames == MAX_FRAMES {
            return Err(Error::AnimationTooLarge);
        }
        self.remaining = self
            .remaining
            .checked_sub(u64::from(width) * u64::from(height))
            .ok_or(Error::AnimationTooLarge)?;
        self.frames += 1;
        Ok(())
    }

    pub(super) fn is_empty(&self) -> bool {
        self.frames == 0
    }
}

pub(super) struct Decoded<'a> {
    pub frames: Box<dyn Iterator<Item = Result<Frame>> + 'a>,
    pub dimensions: (u32, u32),
    pub orientation: Option<Orientation>,
    /// Total plays, with zero representing infinite playback.
    pub plays: u32,
}

pub(super) fn decode(
    contents: &[u8],
    format: Format,
    limits: Limits,
    oriented: bool,
) -> Result<Option<Decoded<'_>>> {
    let reader = Cursor::new(contents);
    let decoder_limits = codecs::decoder_limits(limits);
    let decoded = match format {
        Format::Gif => {
            let mut decoder = GifDecoder::new(reader).map_err(codecs::decode_error)?;
            decoder
                .set_limits(decoder_limits)
                .map_err(codecs::decode_error)?;
            // The image wrapper merges GIF's missing loop extension with infinite looping.
            let header = gif::DecodeOptions::new()
                .read_info(contents)
                .map_err(|error| codecs::codec_error(format, error))?;
            let plays = match header.repeat() {
                gif::Repeat::Finite(repeats) => u32::from(repeats) + 1,
                gif::Repeat::Infinite => 0,
            };
            from_decoder(decoder, limits, oriented, Some(plays))?
        }
        Format::Png => {
            let mut decoder =
                PngDecoder::with_limits(reader, decoder_limits).map_err(codecs::decode_error)?;
            if !decoder.is_apng().map_err(codecs::decode_error)? {
                return Ok(None);
            }
            let dimensions = decoder.dimensions();
            limits.dimensions(dimensions.0, dimensions.1)?;
            let orientation = oriented
                .then(|| decoder.orientation().map_err(codecs::decode_error))
                .transpose()?;
            let decoder = decoder.apng().map_err(codecs::decode_error)?;
            Decoded {
                plays: total_plays(decoder.loop_count()),
                dimensions,
                orientation,
                frames: Box::new(
                    decoder
                        .into_frames()
                        .map(|frame| frame.map_err(codecs::decode_error)),
                ),
            }
        }
        Format::Webp => {
            let mut decoder = WebPDecoder::new(reader).map_err(codecs::decode_error)?;
            if !decoder.has_animation() {
                return Ok(None);
            }
            decoder
                .set_limits(decoder_limits)
                .map_err(codecs::decode_error)?;
            from_decoder(decoder, limits, oriented, None)?
        }
        Format::Avif | Format::Heic => return super::sequence::decode(contents, format, limits),
        Format::Jxl => return super::jxl_sequence::decode(contents, limits),
        _ => return Ok(None),
    };
    Ok(Some(decoded))
}

fn from_decoder<'a>(
    mut decoder: impl ImageDecoder + AnimationDecoder<'a>,
    limits: Limits,
    oriented: bool,
    plays: Option<u32>,
) -> Result<Decoded<'a>> {
    let dimensions = decoder.dimensions();
    limits.dimensions(dimensions.0, dimensions.1)?;
    let orientation = oriented
        .then(|| decoder.orientation().map_err(codecs::decode_error))
        .transpose()?;
    Ok(Decoded {
        plays: plays.unwrap_or_else(|| total_plays(decoder.loop_count())),
        dimensions,
        orientation,
        frames: Box::new(
            decoder
                .into_frames()
                .map(|frame| frame.map_err(codecs::decode_error)),
        ),
    })
}

fn total_plays(loops: LoopCount) -> u32 {
    match loops {
        LoopCount::Finite(plays) => plays.get(),
        LoopCount::Infinite => 0,
    }
}

pub(super) fn supports_animation(format: Format) -> bool {
    matches!(
        format,
        Format::Gif | Format::Webp | Format::Png | Format::Avif | Format::Heic | Format::Jxl
    )
}

pub(super) fn encode(
    frames: Vec<Frame>,
    original: Format,
    format: Format,
    quality: u8,
    plays: u32,
) -> Result<Vec<u8>> {
    let animated = original != Format::Gif
        || frames.len() > 1
        || plays != 1
        || frames[0].delay().numer_denom_ms().0 != 0;
    match format {
        Format::Gif => encode_gif(frames, plays),
        Format::Png if animated => encode_apng(frames, plays),
        Format::Webp if animated => encode_webp(frames, plays, quality),
        Format::Avif if animated => super::avif_sequence::encode(frames, quality, plays),
        Format::Heic if animated => super::sequence::encode(frames, format, quality, plays),
        Format::Jxl if animated => super::jxl_sequence::encode(frames, quality, plays),
        _ => {
            let frame = frames.into_iter().next().ok_or(Error::InvalidDimensions)?;
            codecs::encode(
                DynamicImage::ImageRgba8(frame.into_buffer()),
                format,
                quality,
            )
        }
    }
}

fn encode_gif(frames: Vec<Frame>, plays: u32) -> Result<Vec<u8>> {
    let (width, height) = frames[0].buffer().dimensions();
    let width = u16::try_from(width).map_err(|_| Error::DimensionsTooLarge)?;
    let height = u16::try_from(height).map_err(|_| Error::DimensionsTooLarge)?;
    let error = |error| codecs::codec_error(Format::Gif, error);
    let mut gif = gif::Encoder::new(Vec::new(), width, height, &[]).map_err(error)?;
    match plays {
        0 => gif.set_repeat(gif::Repeat::Infinite).map_err(error)?,
        1 => {}
        count => {
            let repeats = u16::try_from(count - 1).map_err(|_| Error::InvalidAnimationTiming)?;
            gif.set_repeat(gif::Repeat::Finite(repeats))
                .map_err(error)?;
        }
    }
    for frame in frames {
        let delay =
            u16::try_from(ticks(frame.delay(), 10)).map_err(|_| Error::InvalidAnimationTiming)?;
        let mut pixels = frame.into_buffer().into_raw();
        let mut frame = gif::Frame::from_rgba_speed(width, height, &mut pixels, 1);
        frame.delay = delay;
        // Decoders see each composited frame as its own full-canvas image.
        frame.dispose = gif::DisposalMethod::Background;
        gif.write_frame(&frame).map_err(error)?;
    }
    gif.into_inner().map_err(error)
}

fn encode_apng(frames: Vec<Frame>, plays: u32) -> Result<Vec<u8>> {
    let (width, height) = frames[0].buffer().dimensions();
    let mut bytes = Vec::new();
    let mut png = png::Encoder::new(&mut bytes, width, height);
    png.set_color(png::ColorType::Rgba);
    png.set_depth(png::BitDepth::Eight);
    let error = |error| codecs::codec_error(Format::Png, error);
    png.set_animated(frames.len() as u32, plays)
        .map_err(error)?;
    let mut writer = png.write_header().map_err(error)?;
    for frame in frames {
        let (numerator, denominator) = png_delay(frame.delay())?;
        writer
            .set_frame_delay(numerator, denominator)
            .map_err(error)?;
        writer
            .write_image_data(frame.buffer().as_raw())
            .map_err(error)?;
    }
    writer.finish().map_err(error)?;
    Ok(bytes)
}

fn png_delay(delay: Delay) -> Result<(u16, u16)> {
    let (numerator, denominator) = delay.numer_denom_ms();
    let (numerator, denominator) = (u64::from(numerator), u64::from(denominator) * 1000);
    let gcd = |mut a, mut b| {
        while b != 0 {
            (a, b) = (b, a % b);
        }
        a
    };
    let gcd = gcd(numerator, denominator);
    if let (Ok(numerator), Ok(denominator)) = (
        u16::try_from(numerator / gcd),
        u16::try_from(denominator / gcd),
    ) {
        return Ok((numerator, denominator));
    }
    // Keep the finest available denominator when the exact ratio cannot be stored.
    let max = u64::from(u16::MAX);
    let frequency = (max * denominator / numerator.max(1)).clamp(1, max);
    let value = ((numerator * frequency + denominator / 2) / denominator).max(1);
    Ok((
        u16::try_from(value).map_err(|_| Error::InvalidAnimationTiming)?,
        frequency as u16,
    ))
}

pub(super) fn rational_delay(ticks: u32, numerator: u32, denominator: u32) -> Result<Delay> {
    if numerator == 0 || denominator == 0 {
        return Err(Error::InvalidAnimationTiming);
    }
    let n = u64::from(ticks) * u64::from(denominator) * 1000;
    let mut a = n;
    let mut b = u64::from(numerator);
    while b != 0 {
        (a, b) = (b, a % b);
    }
    let ticks = u32::try_from(n / a).map_err(|_| Error::InvalidAnimationTiming)?;
    Ok(Delay::from_numer_denom_ms(ticks, numerator / a as u32))
}

pub(super) fn ticks(delay: Delay, milliseconds_per_tick: u64) -> u64 {
    let (numerator, denominator) = delay.numer_denom_ms();
    let denominator = u64::from(denominator) * milliseconds_per_tick;
    if numerator == 0 {
        0
    } else {
        ((u64::from(numerator) + denominator / 2) / denominator).max(1)
    }
}

fn encode_webp(frames: Vec<Frame>, plays: u32, quality: u8) -> Result<Vec<u8>> {
    let (width, height) = frames[0].buffer().dimensions();
    let has_alpha = frames
        .iter()
        .any(|frame| frame.buffer().pixels().any(|pixel| pixel[3] != 255));
    let flags = build::Vp8xFlags {
        has_animation: true,
        has_alpha,
        ..Default::default()
    };
    let canvas = build::build_vp8x_chunk(width, height, flags)
        .map_err(|error| codecs::codec_error(Format::Webp, error))?;
    let loop_count = u16::try_from(plays).map_err(|_| Error::InvalidAnimationTiming)?;
    let mut anim = [0; 6];
    anim[4..].copy_from_slice(&loop_count.to_le_bytes());

    let mut file = b"RIFF\0\0\0\0WEBP".to_vec();
    let mut chunk = |name, payload: &[u8]| {
        let bytes = build::build_chunk(name, payload)
            .map_err(|error| codecs::codec_error(Format::Webp, error))?;
        file.extend(bytes);
        Ok::<_, Error>(())
    };
    chunk(fourcc::VP8X, &canvas)?;
    chunk(fourcc::ANIM, &anim)?;
    for frame in frames {
        let delay = ticks(frame.delay(), 1);
        if delay > 0xff_ffff {
            return Err(Error::InvalidAnimationTiming);
        }
        let encoded = codecs::encode_webp(frame.buffer(), quality)?;
        let webp = oxideav_webp::container::parse(&encoded)
            .map_err(|error| codecs::codec_error(Format::Webp, error))?;
        // RFC 9649 ANMF takes uint24 canvas offsets, dimensions minus one and time.
        // The no-blend flag replaces all pixels, including transparency.
        let mut payload = vec![0; 6];
        payload.extend_from_slice(&(width - 1).to_le_bytes()[..3]);
        payload.extend_from_slice(&(height - 1).to_le_bytes()[..3]);
        payload.extend_from_slice(&(delay as u32).to_le_bytes()[..3]);
        payload.push(0b10);
        for bitstream in webp
            .chunks
            .iter()
            .filter(|c| matches!(c.fourcc, fourcc::ALPH | fourcc::VP8 | fourcc::VP8L))
        {
            payload.extend_from_slice(
                &encoded[bitstream.payload_start - 8
                    ..bitstream.payload_end + (bitstream.size % 2) as usize],
            );
        }
        chunk(fourcc::ANMF, &payload)?;
    }
    let size = u32::try_from(file.len() - 8).map_err(|_| Error::AnimationTooLarge)?;
    file[4..8].copy_from_slice(&size.to_le_bytes());
    Ok(file)
}
