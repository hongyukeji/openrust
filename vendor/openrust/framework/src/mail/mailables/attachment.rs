use std::path::{Path, PathBuf};

use percent_encoding::percent_decode_str;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncReadExt as _;
use url::Url;

use crate::{
    http::UploadedFile,
    mail::{MailResult, composition},
    support::facades::{Http, Storage},
};

use super::Attachment;

/// A source resolved into attachment bytes when a mailable is sent.
#[derive(Clone, Deserialize, Serialize)]
pub struct AttachmentSource {
    source: Source,
    name: Option<String>,
    mime: Option<String>,
}

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
enum Source {
    Path(PathBuf),
    Disk { disk: Option<String>, path: String },
    Data(Vec<u8>),
    Url(String),
}

#[allow(non_snake_case, reason = "Matches Laravel's attachment constructors")]
impl Attachment {
    pub fn fromPath(path: impl Into<PathBuf>) -> AttachmentSource {
        AttachmentSource {
            source: Source::Path(path.into()),
            name: None,
            mime: None,
        }
    }

    pub fn fromUrl(url: &str) -> MailResult<AttachmentSource> {
        Ok(AttachmentSource {
            source: Source::Url(parse_url(url)?.into()),
            name: None,
            mime: None,
        })
    }

    pub fn fromData(data: impl Into<Vec<u8>>, name: impl Into<String>) -> AttachmentSource {
        AttachmentSource {
            source: Source::Data(data.into()),
            name: Some(name.into()),
            mime: None,
        }
    }

    pub fn fromStorage(path: impl Into<String>) -> AttachmentSource {
        AttachmentSource::disk(None, path.into())
    }

    pub fn fromStorageDisk(disk: impl Into<String>, path: impl Into<String>) -> AttachmentSource {
        AttachmentSource::disk(Some(disk.into()), path.into())
    }

    pub fn fromCloudStorage(path: impl Into<String>) -> MailResult<AttachmentSource> {
        let disk = Storage::getDefaultCloudDriver().map_err(composition)?;
        Ok(Self::fromStorageDisk(disk, path))
    }

    pub fn fromUploadedFile(file: &UploadedFile) -> AttachmentSource {
        let mime = file
            .detected_type()
            .map(|(_, mime)| mime)
            .or_else(|| file.getClientMimeType());
        Self::fromData(file.get().to_vec(), file.getClientOriginalName()).with_metadata(None, mime)
    }
}

impl AttachmentSource {
    pub(crate) fn matches(&self, other: &Self) -> bool {
        if self.source != other.source || self.mime != other.mime {
            return false;
        }
        let default_name = match &self.source {
            Source::Disk { path, .. } => Path::new(path).file_name().and_then(|name| name.to_str()),
            _ => None,
        };
        self.name.as_deref().or(default_name) == other.name.as_deref().or(default_name)
    }

    pub(crate) fn with_metadata(mut self, name: Option<&str>, mime: Option<&str>) -> Self {
        if let Some(name) = name {
            self.name = Some(name.into());
        }
        if let Some(mime) = mime {
            self.mime = Some(mime.into());
        }
        self
    }

    pub(crate) fn matches_materialized(&self, attachment: &Attachment) -> bool {
        matches!(&self.source, Source::Data(bytes) if *bytes == attachment.bytes)
            && self.name.as_deref() == Some(attachment.name.as_str())
            && self.mime.as_deref().unwrap_or("application/octet-stream") == attachment.mime
    }

    fn disk(disk: Option<String>, path: String) -> Self {
        Self {
            source: Source::Disk { disk, path },
            name: None,
            mime: None,
        }
    }

    pub fn r#as(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    #[allow(non_snake_case, reason = "Matches Laravel's attachment MIME override")]
    pub fn withMime(mut self, mime: impl Into<String>) -> Self {
        self.mime = Some(mime.into());
        self
    }

    pub(crate) async fn resolve(self) -> MailResult<Attachment> {
        let (path, bytes, content_type) = match self.source {
            Source::Path(path) => {
                let bytes = tokio::fs::read(&path).await.map_err(|error| {
                    composition(format!(
                        "Cannot read mail attachment [{}]: {error}",
                        path.display()
                    ))
                })?;
                (Some(path), bytes, None)
            }
            Source::Disk { disk, path } => {
                let storage = Storage::disk(disk.as_deref()).map_err(composition)?;
                let (mut reader, content_type) = storage
                    .read_with_content_type(&path)
                    .await
                    .map_err(composition)?
                    .ok_or_else(|| {
                        composition(format!(
                            "Mail attachment [{path}] was not found on its storage disk."
                        ))
                    })?;
                let mut bytes = Vec::new();
                reader.read_to_end(&mut bytes).await.map_err(composition)?;
                (Some(PathBuf::from(path)), bytes, content_type)
            }
            Source::Data(bytes) => (None, bytes, None),
            Source::Url(url) => {
                let url = parse_url(&url)?;
                let response = Http::get(url.as_str()).await.map_err(composition)?;
                if !response.successful() {
                    return Err(composition(format!(
                        "Mail attachment URL returned HTTP {}.",
                        response.status().as_u16()
                    )));
                }
                let filename = url
                    .path_segments()
                    .and_then(|mut parts| parts.next_back())
                    .filter(|part| !part.is_empty())
                    .map(|part| {
                        PathBuf::from(percent_decode_str(part).decode_utf8_lossy().into_owned())
                    });
                let content_type = response
                    .header("content-type")
                    .and_then(|header| header.to_str().ok())
                    .map(str::to_owned);
                (
                    filename,
                    response.body().map_err(composition)?.to_vec(),
                    content_type,
                )
            }
        };
        let name = self
            .name
            .or_else(|| {
                path.as_deref()
                    .and_then(Path::file_name)
                    .map(|name| name.to_string_lossy().into_owned())
            })
            .filter(|name| !name.is_empty())
            .ok_or_else(|| composition("Mail attachment requires a filename."))?;
        let mime = self.mime.or(content_type).unwrap_or_else(|| {
            let Some(path) = path else {
                return "application/octet-stream".to_owned();
            };
            infer::get(&bytes).map_or_else(
                || {
                    mime_guess::from_path(path)
                        .first_or_octet_stream()
                        .essence_str()
                        .to_owned()
                },
                |kind| kind.mime_type().to_owned(),
            )
        });
        Ok(Attachment { name, mime, bytes })
    }
}

fn parse_url(value: &str) -> MailResult<Url> {
    let invalid_url = || composition("Mail attachment URLs must use the http or https scheme.");
    let (scheme, authority) = value.split_once("://").ok_or_else(invalid_url)?;
    if !(scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https"))
        || authority.starts_with(['/', '\\'])
    {
        return Err(invalid_url());
    }
    Url::parse(value)
        .ok()
        .filter(Url::has_host)
        .ok_or_else(invalid_url)
}
