use std::path::Path;

use bytes::Bytes;
use tokio::io::{AsyncRead, AsyncReadExt};

use openrust::{
    filesystem::{Storage, Visibility, WriteOptions},
    support::Str,
};

use crate::{AiError, AiResult};

pub(crate) async fn write(
    bytes: &[u8],
    extension: &str,
    directory: &str,
    name: Option<&str>,
    disk: Option<&str>,
    visibility: Option<Visibility>,
) -> AiResult<String> {
    let name = match name {
        Some(name)
            if !name.trim().is_empty()
                && !name.contains(['/', '\\'])
                && !matches!(name, "." | "..")
                && !name.chars().any(char::is_control) =>
        {
            name.to_owned()
        }
        Some(_) => return Err(AiError::InvalidConfiguration("media.filename")),
        None => format!("{}.{}", Str::ulid(), extension),
    };
    let directory = directory.trim_matches('/');
    let path = if directory.is_empty() {
        name
    } else {
        format!("{directory}/{name}")
    };
    let disk = Storage::disk(disk).map_err(|_| AiError::MediaStorageUnavailable)?;
    let stored = disk
        .put_with_options(&path, bytes, WriteOptions { visibility })
        .await
        .map_err(|_| AiError::MediaStorageUnavailable)?;
    if !stored {
        return Err(AiError::MediaStorageUnavailable);
    }
    Ok(path)
}

pub(crate) async fn read(path: &str, disk: Option<&str>, limit: usize) -> AiResult<Option<Bytes>> {
    let disk = Storage::disk(disk).map_err(|_| AiError::MediaStorageUnavailable)?;
    let Some(reader) = disk
        .read_stream(path)
        .await
        .map_err(|_| AiError::MediaStorageUnavailable)?
    else {
        return Ok(None);
    };
    read_limited(reader, limit).await.map(Some)
}

pub(crate) async fn read_path(path: &Path, limit: usize) -> AiResult<Bytes> {
    let metadata = tokio::fs::metadata(path)
        .await
        .map_err(|_| AiError::MediaStorageUnavailable)?;
    if !metadata.is_file() {
        return Err(AiError::MediaStorageUnavailable);
    }
    let file = tokio::fs::File::open(path)
        .await
        .map_err(|_| AiError::MediaStorageUnavailable)?;
    read_limited(file, limit).await
}

pub(crate) async fn read_limited(reader: impl AsyncRead, limit: usize) -> AiResult<Bytes> {
    let mut content = Vec::new();
    let mut reader = std::pin::pin!(reader.take(limit as u64));
    reader
        .read_to_end(&mut content)
        .await
        .map_err(|_| AiError::MediaStorageUnavailable)?;
    if content.len() >= limit {
        return Err(AiError::MediaTooLarge);
    }
    Ok(content.into())
}
