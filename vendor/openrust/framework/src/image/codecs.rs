use std::io::Cursor;

use ::image::{
    DynamicImage, ImageDecoder as _, ImageFormat, ImageReader, RgbaImage, metadata::Orientation,
};

use super::{
    Error, Format, Limits, Result,
    metadata::{self, Metadata},
};

pub(super) fn identify(bytes: &[u8]) -> Result<Format> {
    if bytes.starts_with(&[0xff, 0x0a]) || bytes.starts_with(b"\0\0\0\x0cJXL \r\n\x87\n") {
        return Ok(Format::Jxl);
    }
    if let Ok(header) = oxideav_heif::boxes::parse_box_header(bytes, 0)
        && header.box_type == *b"ftyp"
        && let Ok(brands) = oxideav_heif::ftyp::FileType::parse(
            header.box_type,
            oxideav_heif::boxes::payload(bytes, &header),
        )
    {
        let class = brands.classify();
        if class.av1 {
            return Ok(Format::Avif);
        }
        if class.hevc || brands.major_brand == *b"mif1" {
            return Ok(Format::Heic);
        }
    }
    Format::try_from(::image::guess_format(bytes).map_err(decode_error)?)
}

/// Reads image metadata without decoding pixels or applying EXIF orientation.
pub(crate) fn encoded_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    match bytes.first_chunk::<4>()? {
        b"II*\0" => return super::directory_dimensions::tiff(bytes, true),
        b"MM\0*" => return super::directory_dimensions::tiff(bytes, false),
        b"\0\0\x01\0" => return super::directory_dimensions::ico(bytes),
        _ => {}
    }
    let dimensions = match identify(bytes).ok()? {
        Format::Avif | Format::Heic => {
            use oxideav_heif::{HeifFile, props::ItemProperties};
            let file = HeifFile::parse_borrowed(bytes).ok()?;
            let meta = file.meta().ok()?;
            let props = ItemProperties::resolve(meta, file.primary_item().ok()?.id).ok()?;
            let size = props.ispe()?;
            (size.width, size.height)
        }
        Format::Jxl => return None,
        _ => ImageReader::new(Cursor::new(bytes))
            .with_guessed_format()
            .ok()?
            .into_dimensions()
            .ok()?,
    };
    (dimensions.0 > 0 && dimensions.1 > 0).then_some(dimensions)
}

pub(super) fn decode(
    contents: &[u8],
    format: Format,
    limits: Limits,
    oriented: bool,
) -> Result<(DynamicImage, Option<Orientation>)> {
    let decoder_limits = decoder_limits(limits);
    let (image, orientation) = match format {
        Format::Avif => (decode_avif(contents, limits)?, None),
        Format::Heic => (decode_heic(contents, limits)?, None),
        Format::Jxl => {
            let mut decoder = jxl_oxide::integration::JxlDecoder::new(Cursor::new(contents))
                .map_err(decode_error)?;
            decoder.set_limits(decoder_limits).map_err(decode_error)?;
            // JPEG XL's decoder already applies the codestream orientation.
            decode_native(decoder, limits, false)?
        }
        _ => {
            let mut reader = ImageReader::new(Cursor::new(contents))
                .with_guessed_format()
                .map_err(Error::Io)?;
            reader.limits(decoder_limits);
            decode_native(
                reader.into_decoder().map_err(decode_error)?,
                limits,
                oriented,
            )?
        }
    };
    Ok((image, orientation))
}

fn decode_native(
    mut decoder: impl ::image::ImageDecoder,
    limits: Limits,
    oriented: bool,
) -> Result<(DynamicImage, Option<Orientation>)> {
    let (width, height) = decoder.dimensions();
    limits.dimensions(width, height)?;
    let orientation = oriented
        .then(|| decoder.orientation().map_err(decode_error))
        .transpose()?;
    Ok((
        DynamicImage::from_decoder(decoder).map_err(decode_error)?,
        orientation,
    ))
}

pub(super) fn decoder_limits(limits: Limits) -> ::image::Limits {
    let mut decoder = ::image::Limits::default();
    decoder.max_image_width = Some(limits.max_dimension);
    decoder.max_image_height = Some(limits.max_dimension);
    decoder.max_alloc = Some(limits.max_pixels.saturating_mul(12));
    decoder
}

fn decode_avif(contents: &[u8], limits: Limits) -> Result<DynamicImage> {
    let backend = imgalifier_avif::Av1Backend {
        max_pixels: u32::try_from(limits.max_pixels).unwrap_or(u32::MAX),
        ..Default::default()
    };
    let decoder = imgalifier_avif::Decoder::new(
        contents,
        backend,
        imgalifier_avif::Limits {
            max_item_bytes: limits.max_input_bytes,
            max_pixels: limits.max_pixels,
            max_threads: 1,
            ..Default::default()
        },
    )
    .map_err(avif_error)?;
    let info = decoder.info().map_err(avif_error)?;
    limits.dimensions(info.width, info.height)?;
    let image = decoder.decode_primary().map_err(avif_error)?;
    rgba_image(image.width, image.height, image.data, limits)
}

fn decode_heic(contents: &[u8], limits: Limits) -> Result<DynamicImage> {
    let options = oxideav_heif::DecodeOptions::default()
        .with_max_width(Some(limits.max_dimension))
        .with_max_height(Some(limits.max_dimension))
        .with_max_pixels(Some(limits.max_pixels))
        .with_max_bytes(Some(limits.max_input_bytes as u64));
    let image = oxideav_heif::decode_with(contents, &options).map_err(heic_error)?;
    limits.dimensions(image.width(), image.height())?;
    let pixels = image.try_to_rgba8().map_err(heic_error)?;
    rgba_image(image.width(), image.height(), pixels, limits)
}

fn rgba_image(width: u32, height: u32, pixels: Vec<u8>, limits: Limits) -> Result<DynamicImage> {
    limits.dimensions(width, height)?;
    RgbaImage::from_raw(width, height, pixels)
        .map(DynamicImage::ImageRgba8)
        .ok_or(Error::InvalidDimensions)
}

pub(super) fn encode(
    image: DynamicImage,
    format: Format,
    quality: u8,
    metadata: &Metadata,
) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    match format {
        Format::Webp => return encode_webp(&image.into_rgba8(), quality, metadata),
        Format::Heic => {
            let qp = ((100 - u32::from(quality)) * 51 / 100) as u8;
            let options = oxideav_heif::EncodeOptions::default()
                .with_qp(qp)
                .with_icc_profile(metadata.icc.clone())
                .with_exif(metadata.exif.clone())
                .with_xmp(
                    metadata
                        .xmp
                        .as_ref()
                        .and_then(|xmp| String::from_utf8(xmp.clone()).ok()),
                );
            return if image.color().has_alpha() {
                let rgba = image.into_rgba8();
                let (width, height) = rgba.dimensions();
                oxideav_heif::encode_rgba8(width, height, rgba.as_raw(), &options)
            } else {
                let rgb = image.into_rgb8();
                let (width, height) = rgb.dimensions();
                oxideav_heif::encode_rgb8(width, height, rgb.as_raw(), &options)
            }
            .map_err(heic_error);
        }
        Format::Jxl => return encode_jxl(image, quality, metadata),
        Format::Jpeg => {
            let mut encoder =
                ::image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, quality);
            set_metadata(&mut encoder, format, metadata)?;
            image.write_with_encoder(encoder).map_err(Error::Encoding)?;
            metadata::write_jpeg_profiles(&mut bytes, metadata)?;
            return Ok(bytes);
        }
        Format::Avif => {
            let encoder =
                ::image::codecs::avif::AvifEncoder::new_with_speed_quality(&mut bytes, 8, quality)
                    .with_num_threads(Some(1));
            image.write_with_encoder(encoder).map_err(Error::Encoding)?;
            return super::heif_encoder::embed_still(bytes, metadata);
        }
        Format::Png => {
            let mut encoder = ::image::codecs::png::PngEncoder::new(&mut bytes);
            set_metadata(&mut encoder, format, metadata)?;
            image.write_with_encoder(encoder).map_err(Error::Encoding)?;
            if let Some(xmp) = &metadata.xmp {
                metadata::write_png_xmp(&mut bytes, xmp)?;
            }
            return Ok(bytes);
        }
        Format::Gif => image.write_to(&mut Cursor::new(&mut bytes), ImageFormat::Gif),
        Format::Bmp => image.write_to(&mut Cursor::new(&mut bytes), ImageFormat::Bmp),
    }
    .map_err(Error::Encoding)?;
    Ok(bytes)
}

fn set_metadata(
    encoder: &mut impl ::image::ImageEncoder,
    format: Format,
    metadata: &Metadata,
) -> Result<()> {
    if let Some(profile) = &metadata.icc {
        encoder
            .set_icc_profile(profile.clone())
            .map_err(|error| codec_error(format, error))?;
    }
    if let Some(exif) = &metadata.exif {
        encoder
            .set_exif_metadata(exif.clone())
            .map_err(|error| codec_error(format, error))?;
    }
    Ok(())
}

fn encode_jxl(image: DynamicImage, quality: u8, metadata: &Metadata) -> Result<Vec<u8>> {
    use ::image::ColorType;

    let width = image.width() as usize;
    let height = image.height() as usize;
    let mut options = jixel::EncodeConfig::default()
        .with_quality(f32::from(quality))
        .with_lossless(quality == 100)
        .with_function_fields(false)
        .with_num_threads(1);
    if let Some(icc) = &metadata.icc {
        options = options.with_icc_profile(icc.clone());
    }
    if let Some(exif) = &metadata.exif {
        options = options.with_exif(exif.clone());
    }
    if let Some(xmp) = &metadata.xmp {
        options = options.with_xmp(xmp.clone());
    }
    match image.color() {
        ColorType::L16 | ColorType::Rgb16 => {
            jixel::encode_image_16bit(image.into_rgb16().as_raw(), width, height, &options)
        }
        ColorType::La16 | ColorType::Rgba16 => jixel::encode_image_with_alpha_16bit(
            image.into_rgba16().as_raw(),
            width,
            height,
            &options,
        ),
        color if color.has_alpha() => {
            jixel::encode_image_with_alpha(image.into_rgba8().as_raw(), width, height, &options)
        }
        _ => jixel::encode_image(image.into_rgb8().as_raw(), width, height, &options),
    }
    .map_err(|error| codec_error(Format::Jxl, error))
}

pub(super) fn encode_webp(image: &RgbaImage, quality: u8, metadata: &Metadata) -> Result<Vec<u8>> {
    let (width, height) = image.dimensions();
    let mut options = oxideav_webp::EncodeOptions::default();
    if quality < 100 {
        options = options.with_quality(f32::from(quality));
    }
    let error = |error| codec_error(Format::Webp, error);
    if metadata.icc.is_none() && metadata.exif.is_none() && metadata.xmp.is_none() {
        return oxideav_webp::encode_rgba8(width, height, image.as_raw(), &options).map_err(error);
    }
    let image = oxideav_webp::WebpImage::from_rgba8(width, height, image.as_raw().clone())
        .map_err(error)?
        .with_metadata(metadata.webp());
    oxideav_webp::encode(&image, &options).map_err(error)
}

pub(super) fn decode_error(error: ::image::ImageError) -> Error {
    match error {
        ::image::ImageError::Limits(_) => Error::DimensionsTooLarge,
        ::image::ImageError::Unsupported(_) => Error::UnsupportedFormat,
        error => Error::Encoding(error),
    }
}

fn avif_error(error: imgalifier_avif::Error) -> Error {
    match error {
        imgalifier_avif::Error::LimitExceeded(_) => Error::DimensionsTooLarge,
        error => codec_error(Format::Avif, error),
    }
}

fn heic_error(error: oxideav_heif::Error) -> Error {
    match error {
        oxideav_heif::Error::LimitExceeded(_) => Error::DimensionsTooLarge,
        error => codec_error(Format::Heic, error),
    }
}

pub(super) fn codec_error(
    format: Format,
    source: impl std::error::Error + Send + Sync + 'static,
) -> Error {
    Error::Codec {
        format,
        source: Box::new(source),
    }
}
