use std::{
    sync::Arc,
    time::{Duration, SystemTime},
};

use openrust::filesystem::{FilesystemManager, TemporaryUrlOptions};
use serde_json::Value;
use url::Url;

use super::field;
use crate::{ModerationError, ModerationResult, StoredMedia};

pub(super) struct VersionedMediaLinks {
    storage: Arc<FilesystemManager>,
    disk: String,
    origin: url::Origin,
}

impl VersionedMediaLinks {
    pub(super) fn new(storage: Arc<FilesystemManager>, profile: &Value) -> ModerationResult<Self> {
        let disk = field(profile, "media_disk")?
            .ok_or(ModerationError::InvalidConfiguration("media_disk"))?;
        let origin = field(profile, "media_origin")?
            .and_then(|value| Url::parse(value).ok())
            .filter(|url| {
                url.scheme() == "https"
                    && url.path() == "/"
                    && url.query().is_none()
                    && url.fragment().is_none()
                    && url.username().is_empty()
                    && url.password().is_none()
                    && url.port().is_none()
            })
            .ok_or(ModerationError::InvalidConfiguration("media_origin"))?;
        Ok(Self {
            storage,
            disk: disk.to_owned(),
            origin: origin.origin(),
        })
    }

    pub(super) async fn create(&self, source: &StoredMedia) -> ModerationResult<Url> {
        // S3 的 null 版本可能被覆盖，不能用来固定送审对象。
        if source.disk() != self.disk || source.version() == "null" {
            return Err(ModerationError::InvalidInput("versioned media"));
        }
        let disk = self
            .storage
            .disk(Some(&self.disk))
            .map_err(|_| ModerationError::MediaUnavailable)?;
        let temporary = disk
            .temporary_url_with(
                source.key(),
                SystemTime::now() + Duration::from_secs(300),
                TemporaryUrlOptions {
                    version_id: Some(source.version().to_owned()),
                    ..Default::default()
                },
            )
            .await
            .map_err(|_| ModerationError::MediaUnavailable)?;
        let url = Url::parse(&temporary).map_err(|_| ModerationError::MediaUnavailable)?;
        if url.origin() != self.origin
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
        {
            return Err(ModerationError::MediaUnavailable);
        }
        let mut versions = url.query_pairs().filter(|(name, _)| name == "versionId");
        let version_matches = versions
            .next()
            .is_some_and(|(_, value)| value == source.version())
            && versions.next().is_none();
        let signed = url.query_pairs().any(|(name, value)| {
            name == "X-Amz-Signature"
                && value.len() == 64
                && value.bytes().all(|byte| byte.is_ascii_hexdigit())
        });
        if !version_matches || !signed {
            return Err(ModerationError::MediaUnavailable);
        }
        Ok(url)
    }
}
