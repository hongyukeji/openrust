use std::io::Cursor;

use ::image::{ImageDecoder, ImageReader, metadata::Orientation};

use super::{Format, Limits, Result, codecs};

#[derive(Default)]
pub(super) struct Metadata {
    pub icc: Option<Vec<u8>>,
    pub exif: Option<Vec<u8>>,
    pub xmp: Option<Vec<u8>>,
}

impl Metadata {
    pub(super) fn read(bytes: &[u8], format: Format, limits: Limits) -> Result<Self> {
        match format {
            Format::Avif | Format::Heic => {
                use oxideav_heif::{HeifFile, meta::metadata_items, props::ItemProperties};
                let map = |error| codecs::codec_error(format, error);
                let file = HeifFile::parse_borrowed(bytes).map_err(map)?;
                let (Ok(meta), Ok(item)) = (file.meta(), file.primary_item()) else {
                    return Ok(Self::default());
                };
                let props = ItemProperties::resolve(meta, item.id).map_err(map)?;
                let (exif, xmp) = metadata_items(&file, meta, meta.metadata_of(item.id));
                Ok(Self {
                    icc: props.icc_profile().map(<[u8]>::to_vec),
                    exif,
                    xmp,
                })
            }
            Format::Jpeg | Format::Png | Format::Webp => {
                let mut reader = ImageReader::new(Cursor::new(bytes))
                    .with_guessed_format()
                    .map_err(super::Error::Io)?;
                reader.limits(codecs::decoder_limits(limits));
                let mut decoder = reader.into_decoder().map_err(codecs::decode_error)?;
                Ok(Self {
                    icc: decoder.icc_profile().map_err(codecs::decode_error)?,
                    exif: decoder.exif_metadata().map_err(codecs::decode_error)?,
                    xmp: decoder.xmp_metadata().map_err(codecs::decode_error)?,
                })
            }
            Format::Jxl => super::jxl_sequence::metadata(bytes, limits),
            _ => Ok(Self::default()),
        }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.icc.is_none() && self.exif.is_none() && self.xmp.is_none()
    }

    pub(super) fn normalize_orientation(&mut self) {
        if let Some(exif) = &mut self.exif {
            let _ = Orientation::remove_from_exif_chunk(exif);
        }
    }

    pub(super) fn webp(&self) -> oxideav_webp::Metadata {
        oxideav_webp::Metadata::new()
            .with_icc(self.icc.clone())
            .with_exif(self.exif.clone())
            .with_xmp(self.xmp.clone())
    }
}

pub(super) fn write_jpeg_xmp(bytes: &mut Vec<u8>, xmp: &[u8]) -> Result<()> {
    let namespace = b"http://ns.adobe.com/xap/1.0/\0";
    let size =
        u16::try_from(2 + namespace.len() + xmp.len()).map_err(|_| super::Error::InputTooLarge)?;
    let mut segment = vec![0xff, 0xe1];
    segment.extend_from_slice(&size.to_be_bytes());
    segment.extend_from_slice(namespace);
    segment.extend_from_slice(xmp);
    let offset = if bytes.get(2..4) == Some(&[0xff, 0xe0]) {
        4 + usize::from(u16::from_be_bytes([bytes[4], bytes[5]]))
    } else {
        2
    };
    bytes.splice(offset..offset, segment);
    Ok(())
}

pub(super) fn write_png_xmp(bytes: &mut Vec<u8>, xmp: &[u8]) -> Result<()> {
    let mut chunk = b"iTXtXML:com.adobe.xmp\0\0\0\0\0".to_vec();
    chunk.extend_from_slice(xmp);
    let crc = crc32fast::hash(&chunk);
    let size = u32::try_from(chunk.len() - 4).map_err(|_| super::Error::InputTooLarge)?;
    bytes.splice(
        33..33,
        size.to_be_bytes()
            .into_iter()
            .chain(chunk)
            .chain(crc.to_be_bytes()),
    );
    Ok(())
}
