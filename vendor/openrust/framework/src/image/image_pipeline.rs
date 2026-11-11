use super::{Error, Result};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Format {
    Jpeg,
    Png,
    Gif,
    Webp,
    Avif,
    Bmp,
}

impl Format {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Jpeg => "jpg",
            Self::Png => "png",
            Self::Gif => "gif",
            Self::Webp => "webp",
            Self::Avif => "avif",
            Self::Bmp => "bmp",
        }
    }

    pub fn mime_type(self) -> &'static str {
        self.native().to_mime_type()
    }

    pub(super) fn native(self) -> ::image::ImageFormat {
        match self {
            Self::Jpeg => ::image::ImageFormat::Jpeg,
            Self::Png => ::image::ImageFormat::Png,
            Self::Gif => ::image::ImageFormat::Gif,
            Self::Webp => ::image::ImageFormat::WebP,
            Self::Avif => ::image::ImageFormat::Avif,
            Self::Bmp => ::image::ImageFormat::Bmp,
        }
    }
}

impl TryFrom<::image::ImageFormat> for Format {
    type Error = Error;

    fn try_from(value: ::image::ImageFormat) -> Result<Self> {
        match value {
            ::image::ImageFormat::Jpeg => Ok(Self::Jpeg),
            ::image::ImageFormat::Png => Ok(Self::Png),
            ::image::ImageFormat::Gif => Ok(Self::Gif),
            ::image::ImageFormat::WebP => Ok(Self::Webp),
            ::image::ImageFormat::Avif => Ok(Self::Avif),
            ::image::ImageFormat::Bmp => Ok(Self::Bmp),
            _ => Err(Error::UnsupportedFormat),
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub enum Background {
    #[default]
    Transparent,
    Color([u8; 4]),
    Dominant,
}

#[derive(Clone, Debug)]
pub enum Transformation {
    Resize(Option<u32>, Option<u32>),
    Scale(Option<u32>, Option<u32>),
    Cover(u32, u32),
    Contain(u32, u32, Background),
    Crop {
        width: u32,
        height: u32,
        x: u32,
        y: u32,
    },
    Orient,
    Rotate(f64, Background),
    Blur(u8),
    Sharpen(u8),
    Grayscale,
    FlipHorizontally,
    FlipVertically,
    Custom {
        name: String,
        options: serde_json::Value,
    },
}

#[derive(Clone, Debug, Default)]
pub struct ImagePipeline {
    pub transformations: Vec<Transformation>,
    pub format: Option<Format>,
    pub quality: Option<u8>,
}
