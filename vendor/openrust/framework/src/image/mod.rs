//! Lazy image loading, transformations, and encoding.

mod facade;
mod image_pipeline;
mod instance;
mod native_driver;

pub(crate) use facade::ImageServiceProvider;
pub use facade::{ImageFacade, ImageManager};
pub use image_pipeline::{Background, Format, ImagePipeline, Transformation};
pub use instance::Image;

use axum::body::Bytes;
use std::fmt;

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
    InvalidTransformation,
    Missing,
    Encoding(::image::ImageError),
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
            Self::InvalidDimensions => formatter.write_str("invalid image dimensions"),
            Self::InvalidTransformation => formatter.write_str("invalid image transformation"),
            Self::Missing => formatter.write_str("image source was not found"),
            Self::Encoding(error) => error.fmt(formatter),
            Self::Base64(error) => error.fmt(formatter),
            Self::Io(error) => error.fmt(formatter),
            Self::Filesystem(error) => error.fmt(formatter),
            Self::Http(error) => error.fmt(formatter),
            Self::Unavailable(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;
