use std::{io::Cursor, sync::Arc};

use ::image::{ImageDecoder, ImageReader, metadata::Orientation};

use super::{Format, Limits, Result, codecs};

#[derive(Default)]
pub(super) struct Metadata {
    pub icc: Option<Vec<u8>>,
    pub exif: Option<Vec<u8>>,
    pub xmp: Option<Vec<u8>>,
    pub jpeg_profiles: Vec<u8>,
    pub frames: Vec<FrameMetadata>,
    pub track_exif: Option<Vec<u8>>,
    pub track_xmp: Option<Vec<u8>>,
}

#[derive(Default)]
pub(super) struct FrameMetadata {
    /// Outer option indicates that this sample description overrides the track ICC.
    pub icc: Option<Option<Arc<[u8]>>>,
    pub exif: Option<Vec<u8>>,
    pub xmp: Option<Vec<u8>>,
}

impl Metadata {
    pub(super) fn merge_track(&mut self, track: Self) {
        self.icc = track.icc.or(self.icc.take());
        if self.exif.is_some() {
            self.track_exif = track.exif;
        } else {
            self.exif = track.exif;
        }
        if self.xmp.is_some() {
            self.track_xmp = track.xmp;
        } else {
            self.xmp = track.xmp;
        }
        self.frames = track.frames;
    }

    pub(super) fn read(bytes: &[u8], format: Format, limits: Limits) -> Result<Self> {
        match format {
            Format::Avif | Format::Heic => {
                use oxideav_heif::{HeifFile, meta::metadata_items, props::ItemProperties};
                let map = |error| codecs::codec_error(format, error);
                let file = HeifFile::parse_borrowed(bytes).map_err(map)?;
                let mut metadata = Self::default();
                if let (Ok(meta), Ok(item)) = (file.meta(), file.primary_item()) {
                    let props = ItemProperties::resolve(meta, item.id).map_err(map)?;
                    metadata.icc = props.icc_profile().map(<[u8]>::to_vec);
                    (metadata.exif, metadata.xmp) =
                        metadata_items(&file, meta, meta.metadata_of(item.id));
                }
                Ok(metadata)
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
                    jpeg_profiles: if format == Format::Jpeg {
                        read_jpeg_profiles(bytes)
                    } else {
                        Vec::new()
                    },
                    ..Self::default()
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

pub(super) fn write_jpeg_profiles(bytes: &mut Vec<u8>, metadata: &Metadata) -> Result<()> {
    let mut profiles = Vec::new();
    if let Some(xmp) = &metadata.xmp {
        let namespace = b"http://ns.adobe.com/xap/1.0/\0";
        let size = u16::try_from(2 + namespace.len() + xmp.len())
            .map_err(|_| super::Error::InputTooLarge)?;
        profiles.extend_from_slice(&[0xff, 0xe1]);
        profiles.extend_from_slice(&size.to_be_bytes());
        profiles.extend_from_slice(namespace);
        profiles.extend_from_slice(xmp);
    }
    profiles.extend_from_slice(&metadata.jpeg_profiles);
    if !profiles.is_empty() {
        let offset = if bytes.get(2..4) == Some(&[0xff, 0xe0]) {
            4 + usize::from(u16::from_be_bytes([bytes[4], bytes[5]]))
        } else {
            2
        };
        bytes.splice(offset..offset, profiles);
    }
    Ok(())
}

fn read_jpeg_profiles(bytes: &[u8]) -> Vec<u8> {
    let mut profiles = Vec::new();
    let mut remaining = &bytes[2..];
    while let [0xff, marker, rest @ ..] = remaining {
        if *marker == 0xff {
            remaining = &remaining[1..];
            continue;
        }
        if matches!(marker, 0xda | 0xd9) {
            break;
        }
        if matches!(marker, 0x01 | 0xd0..=0xd7) {
            remaining = rest;
            continue;
        }
        let [high, low, ..] = rest else { break };
        let length = usize::from(u16::from_be_bytes([*high, *low]));
        let Some(payload) = rest.get(2..length) else {
            break;
        };
        if *marker == 0xfe || (*marker == 0xed && payload.starts_with(b"Photoshop 3.0\0")) {
            profiles.extend_from_slice(&remaining[..length + 2]);
        }
        remaining = &rest[length..];
    }
    profiles
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
