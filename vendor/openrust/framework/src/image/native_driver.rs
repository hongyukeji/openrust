use ::image::{
    DynamicImage, GenericImageView as _, Rgba, RgbaImage, imageops, metadata::Orientation,
};
use axum::body::Bytes;

use super::{
    Background, Driver, EncodedImage, Error, Format, ImagePipeline, Limits, Result, Transformation,
    animation, codecs,
};

pub(super) struct NativeDriver;

impl Driver for NativeDriver {
    fn process(
        &self,
        contents: Bytes,
        pipeline: &ImagePipeline,
        limits: Limits,
    ) -> Result<EncodedImage> {
        let original = codecs::identify(&contents)?;
        let format = pipeline.format.unwrap_or(original);
        let passthrough = pipeline.transformations.is_empty()
            && pipeline.format.is_none()
            && pipeline.quality.is_none();
        let oriented = needs_orientation(pipeline);
        let quality = pipeline.quality.unwrap_or(70);

        let (bytes, dimensions) =
            if let Some(animation) = animation::decode(&contents, original, limits, oriented)? {
                let plays = animation.plays;
                let dimensions = animation.dimensions;
                let frames = transform_frames(animation, pipeline, limits, format, passthrough)?;
                if passthrough {
                    (contents.clone(), dimensions)
                } else {
                    let dimensions = frames[0].buffer().dimensions();
                    (
                        animation::encode(frames, original, format, quality, plays)?.into(),
                        dimensions,
                    )
                }
            } else {
                let (image, orientation) = codecs::decode(&contents, original, limits, oriented)?;
                let image = apply(image, orientation, pipeline, limits)?;
                let dimensions = image.dimensions();
                let bytes = if passthrough {
                    contents
                } else {
                    codecs::encode(image, format, quality)?.into()
                };
                (bytes, dimensions)
            };
        Ok(EncodedImage {
            bytes,
            format,
            dimensions,
        })
    }

    fn dominantColor(
        &self,
        contents: Bytes,
        pipeline: &ImagePipeline,
        limits: Limits,
    ) -> Result<String> {
        let format = codecs::identify(&contents)?;
        let oriented = needs_orientation(pipeline);
        let (image, orientation) =
            if let Some(mut animation) = animation::decode(&contents, format, limits, oriented)? {
                let frame = animation
                    .frames
                    .next()
                    .ok_or(Error::InvalidDimensions)?
                    .map_err(codecs::decode_error)?;
                (
                    DynamicImage::ImageRgba8(frame.into_buffer()),
                    animation.orientation,
                )
            } else {
                codecs::decode(&contents, format, limits, oriented)?
            };
        let image = apply(image, orientation, pipeline, limits)?;
        let [red, green, blue, _] = average(&image).0;
        Ok(format!("#{red:02x}{green:02x}{blue:02x}"))
    }
}

fn needs_orientation(pipeline: &ImagePipeline) -> bool {
    pipeline
        .transformations
        .iter()
        .any(|step| matches!(step, Transformation::Orient))
}

fn transform_frames(
    animation: animation::Decoded<'_>,
    pipeline: &ImagePipeline,
    limits: Limits,
    format: Format,
    passthrough: bool,
) -> Result<Vec<::image::Frame>> {
    let mut input = animation::Budget::new(limits);
    let mut output = animation::Budget::new(limits);
    let mut rendered = Vec::new();
    let preserve = animation::supports_animation(format);
    for frame in animation.frames {
        input.add(animation.dimensions)?;
        let frame = frame.map_err(codecs::decode_error)?;
        if !passthrough && (preserve || rendered.is_empty()) {
            let delay = frame.delay();
            let image = apply(
                DynamicImage::ImageRgba8(frame.into_buffer()),
                animation.orientation,
                pipeline,
                limits,
            )?;
            output.add(image.dimensions())?;
            rendered.push(::image::Frame::from_parts(image.into_rgba8(), 0, 0, delay));
        }
    }
    if input.is_empty() {
        return Err(Error::InvalidDimensions);
    }
    Ok(rendered)
}

fn apply(
    mut image: DynamicImage,
    mut orientation: Option<Orientation>,
    pipeline: &ImagePipeline,
    limits: Limits,
) -> Result<DynamicImage> {
    for step in &pipeline.transformations {
        if matches!(step, Transformation::Orient) {
            if let Some(orientation) = orientation.take() {
                image.apply_orientation(orientation);
            }
        } else {
            image = transform(image, step, limits)?;
        }
    }
    Ok(image)
}

fn transform(image: DynamicImage, step: &Transformation, limits: Limits) -> Result<DynamicImage> {
    use Transformation as T;
    let (old_width, old_height) = image.dimensions();
    let filter = imageops::FilterType::Lanczos3;
    let result = match *step {
        T::Resize(width, height) => {
            let (width, height) = match (width, height) {
                (Some(width), Some(height)) => (width, height),
                (Some(width), None) => (width, proportional(old_height, old_width, width)?),
                (None, Some(height)) => (proportional(old_width, old_height, height)?, height),
                (None, None) => return Ok(image),
            };
            limits.dimensions(width, height)?;
            image.resize_exact(width, height, filter)
        }
        T::Scale(width, height) => {
            let width = width.unwrap_or(old_width).min(old_width);
            let height = height.unwrap_or(old_height).min(old_height);
            limits.dimensions(width, height)?;
            image.resize(width, height, filter)
        }
        T::Cover(width, height) => {
            limits.dimensions(width, height)?;
            let (crop_width, crop_height) = fit(old_width, old_height, width, height);
            image
                .crop_imm(
                    (old_width - crop_width) / 2,
                    (old_height - crop_height) / 2,
                    crop_width,
                    crop_height,
                )
                .resize_exact(width, height, filter)
        }
        T::Contain(width, height, background) => {
            limits.dimensions(width, height)?;
            let fill = color(&image, background);
            let (scaled_width, scaled_height) = fit(width, height, old_width, old_height);
            let image = image
                .resize_exact(scaled_width, scaled_height, filter)
                .into_rgba8();
            let mut canvas = RgbaImage::from_pixel(width, height, fill);
            imageops::overlay(
                &mut canvas,
                &image,
                i64::from((width - scaled_width) / 2),
                i64::from((height - scaled_height) / 2),
            );
            DynamicImage::ImageRgba8(canvas)
        }
        T::Crop {
            width,
            height,
            x,
            y,
        } => {
            limits.dimensions(width, height)?;
            if width > old_width.saturating_sub(x) || height > old_height.saturating_sub(y) {
                return Err(Error::InvalidDimensions);
            }
            image.crop_imm(x, y, width, height)
        }
        T::Rotate(angle, background) => rotate(image, angle, background, limits)?,
        T::Blur(amount) if amount > 0 => image.blur(f32::from(amount.min(100)) / 10.0),
        T::Sharpen(amount) if amount > 0 => image.unsharpen(f32::from(amount.min(100)) / 10.0, 0),
        T::Grayscale => image.grayscale(),
        T::FlipHorizontally => image.fliph(),
        T::FlipVertically => image.flipv(),
        T::Custom { .. } => return Err(Error::InvalidTransformation),
        _ => image,
    };
    Ok(result)
}

fn proportional(size: u32, reference: u32, target: u32) -> Result<u32> {
    let product = u64::from(size) * u64::from(target);
    let rounded = (product + u64::from(reference / 2)) / u64::from(reference);
    u32::try_from(rounded.max(1)).map_err(|_| Error::DimensionsTooLarge)
}

fn fit(width: u32, height: u32, aspect_width: u32, aspect_height: u32) -> (u32, u32) {
    let width = u64::from(width);
    let height = u64::from(height);
    let aspect_width = u64::from(aspect_width);
    let aspect_height = u64::from(aspect_height);
    if width * aspect_height > height * aspect_width {
        (
            ((height * aspect_width / aspect_height).max(1)) as u32,
            height as u32,
        )
    } else {
        (
            width as u32,
            ((width * aspect_height / aspect_width).max(1)) as u32,
        )
    }
}

fn rotate(
    image: DynamicImage,
    angle: f64,
    background: Background,
    limits: Limits,
) -> Result<DynamicImage> {
    if !angle.is_finite() {
        return Err(Error::InvalidTransformation);
    }
    let angle = angle.rem_euclid(360.0);
    let (width, height) = image.dimensions();
    match angle {
        0.0 => return Ok(image),
        90.0 | 270.0 => {
            limits.dimensions(height, width)?;
            return Ok(if angle == 90.0 {
                image.rotate90()
            } else {
                image.rotate270()
            });
        }
        180.0 => return Ok(image.rotate180()),
        _ => {}
    }
    let (sin, cos) = angle.to_radians().sin_cos();
    let w = f64::from(width);
    let h = f64::from(height);
    let rotated_width = (w * cos.abs() + h * sin.abs()).ceil() as u32;
    let rotated_height = (w * sin.abs() + h * cos.abs()).ceil() as u32;
    limits.dimensions(rotated_width, rotated_height)?;
    let fill = color(&image, background);
    let pixels = image.into_rgba8();
    let rotated = RgbaImage::from_fn(rotated_width, rotated_height, |x, y| {
        let dx = f64::from(x) + 0.5 - f64::from(rotated_width) / 2.0;
        let dy = f64::from(y) + 0.5 - f64::from(rotated_height) / 2.0;
        let sx = cos * dx + sin * dy + w / 2.0;
        let sy = -sin * dx + cos * dy + h / 2.0;
        if (0.0..w).contains(&sx) && (0.0..h).contains(&sy) {
            *pixels.get_pixel(sx as u32, sy as u32)
        } else {
            fill
        }
    });
    Ok(DynamicImage::ImageRgba8(rotated))
}

fn color(image: &DynamicImage, background: Background) -> Rgba<u8> {
    match background {
        Background::Transparent => Rgba([0, 0, 0, 0]),
        Background::Color(color) => Rgba(color),
        Background::Dominant => average(image),
    }
}

fn average(image: &DynamicImage) -> Rgba<u8> {
    let mut sums = [0_u64; 3];
    let mut weight = 0_u64;
    for (_, _, pixel) in image.pixels() {
        let alpha = u64::from(pixel[3]);
        for (sum, channel) in sums.iter_mut().zip(pixel.0) {
            *sum += u64::from(channel) * alpha;
        }
        weight += alpha;
    }
    if weight == 0 {
        return Rgba([0, 0, 0, 0]);
    }
    let [red, green, blue] = sums.map(|sum| ((sum + weight / 2) / weight) as u8);
    Rgba([red, green, blue, 255])
}
