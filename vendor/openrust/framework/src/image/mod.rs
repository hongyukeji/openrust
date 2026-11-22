//! Lazy image loading, transformations, and encoding.

mod animation;
mod codecs;
mod directory_dimensions;
mod facade;
mod heif_encoder;
mod image_pipeline;
mod instance;
mod jxl_sequence;
mod legacy_dimensions;
mod metadata;
mod native_driver;
mod sequence;
mod sequence_profiles;

pub(crate) use facade::ImageServiceProvider;
pub use facade::{ImageFacade, ImageManager};
pub use image_pipeline::{Background, Format, ImagePipeline, Transformation};
pub use instance::Image;

use axum::body::Bytes;
use std::{any::Any, fmt, sync::Arc};

/// 原生图片驱动交给转换回调的像素类型。
pub use ::image::DynamicImage as NativeImage;

/// 回调修改驱动的实际图片对象，保留流水线中的转换顺序。
pub type TransformationHandler =
    Arc<dyn Fn(&mut dyn Any, &Transformation) -> Result<()> + Send + Sync>;

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub max_input_bytes: usize,
    pub max_dimension: u32,
    pub max_pixels: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_input_bytes: 32 * 1024 * 1024,
            max_dimension: 16_384,
            max_pixels: 24_000_000,
        }
    }
}

impl Limits {
    pub fn dimensions(self, width: u32, height: u32) -> Result<()> {
        if width == 0 || height == 0 {
            return Err(Error::InvalidDimensions);
        }
        if width > self.max_dimension
            || height > self.max_dimension
            || u64::from(width) * u64::from(height) > self.max_pixels
        {
            return Err(Error::DimensionsTooLarge);
        }
        Ok(())
    }
}

/// The encoded bytes and metadata produced by a driver.
#[derive(Clone, Debug)]
pub struct EncodedImage {
    pub bytes: Bytes,
    pub format: Format,
    pub dimensions: (u32, u32),
}

#[allow(non_snake_case, reason = "Matches Laravel's image driver contract")]
pub trait Driver: Send + Sync + 'static {
    fn process(
        &self,
        contents: Bytes,
        pipeline: &ImagePipeline,
        limits: Limits,
    ) -> Result<EncodedImage>;
    fn dimensions(&self, contents: Bytes, limits: Limits) -> Result<(u32, u32)> {
        Ok(self
            .process(contents, &ImagePipeline::default(), limits)?
            .dimensions)
    }

    /// 按转换名称注册驱动处理器；没有可扩展后端的驱动须明确拒绝。
    fn transformUsing(
        &self,
        _transformation: &str,
        _callback: TransformationHandler,
    ) -> Result<()> {
        Err(Error::UnsupportedTransformationHandler)
    }

    fn dominantColor(
        &self,
        contents: Bytes,
        pipeline: &ImagePipeline,
        limits: Limits,
    ) -> Result<String>;
}

#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    UnsupportedFormat,
    WebpEncoding,
    UnknownDriver(String),
    InputTooLarge,
    DimensionsTooLarge,
    InvalidDimensions,
    AnimationTooLarge,
    InvalidAnimationTiming,
    InvalidTransformation,
    UnsupportedTransformationHandler,
    Missing,
    Encoding(::image::ImageError),
    Codec {
        format: Format,
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    Base64(base64::DecodeError),
    Io(std::io::Error),
    Filesystem(crate::filesystem::FilesystemError),
    Http(crate::http::client::Error),
    Unavailable(crate::contracts::container::BindingResolutionError),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedFormat => formatter.write_str("unsupported image format"),
            Self::WebpEncoding => formatter.write_str("could not encode WebP image"),
            Self::UnknownDriver(name) => write!(formatter, "unknown image driver {name}"),
            Self::InputTooLarge => formatter.write_str("image exceeds its input byte limit"),
            Self::DimensionsTooLarge => {
                formatter.write_str("image exceeds its dimension or pixel limit")
            }
            Self::AnimationTooLarge => {
                formatter.write_str("image exceeds its animation frame or pixel limit")
            }
            Self::InvalidAnimationTiming => {
                formatter.write_str("animation timing exceeds the target format's limit")
            }
            Self::InvalidDimensions => formatter.write_str("invalid image dimensions"),
            Self::InvalidTransformation => formatter.write_str("invalid image transformation"),
            Self::UnsupportedTransformationHandler => {
                formatter.write_str("当前图片驱动不支持注册转换处理器")
            }
            Self::Missing => formatter.write_str("image source was not found"),
            Self::Encoding(error) => error.fmt(formatter),
            Self::Codec { format, source } => {
                write!(formatter, "{} codec: {source}", format.extension())
            }
            Self::Base64(error) => error.fmt(formatter),
            Self::Io(error) => error.fmt(formatter),
            Self::Filesystem(error) => error.fmt(formatter),
            Self::Http(error) => error.fmt(formatter),
            Self::Unavailable(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Encoding(error) => Some(error),
            Self::Codec { source, .. } => Some(source.as_ref()),
            Self::Base64(error) => Some(error),
            Self::Io(error) => Some(error),
            Self::Filesystem(error) => Some(error),
            Self::Http(error) => Some(error),
            Self::Unavailable(error) => Some(error),
            _ => None,
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

pub(crate) use codecs::encoded_dimensions;
