use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tokio::io::AsyncReadExt as _;

use crate::{
    mail::{MailResult, composition},
    support::facades::Storage,
};

use super::Attachment;

/// A source resolved into attachment bytes when a mailable is sent.
#[derive(Clone, Deserialize, Serialize)]
pub struct AttachmentSource {
    source: Source,
    name: Option<String>,
    mime: Option<String>,
}

#[derive(Clone, Deserialize, Serialize)]
enum Source {
    Path(PathBuf),
    Disk { disk: Option<String>, path: String },
    Data(Vec<u8>),
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
}

impl AttachmentSource {
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
