use std::{path::Path, sync::Arc};

use bytes::{Bytes, BytesMut};
use futures_util::StreamExt;
use openrust::{
    filesystem::Visibility,
    http::{HttpFuture, Method, UploadedFile, client::Part},
};
use serde::{Deserialize, Serialize};

use crate::{
    Ai, AiError, AiResult,
    connection::Connection,
    media_storage,
    text::{transport_error, validate},
};

const FILE_LIMIT: usize = 100 * 1024 * 1024;

pub struct Files;

impl Files {
    pub async fn get(id: &str, provider: Option<&str>) -> AiResult<FileResponse> {
        validate_id(id)?;
        let provider = Ai::file_provider(provider)?;
        let file = provider.get_file(id).await?;
        if file.id != id {
            return Err(AiError::InvalidResponse);
        }
        FileResponse::new(file, provider)
    }

    pub async fn put(file: FileUpload, provider: Option<&str>) -> AiResult<FileResponse> {
        let provider = Ai::file_provider(provider)?;
        let info = provider.put_file(file).await?;
        FileResponse::new(info, provider)
    }

    pub async fn put_from_path(
        path: impl AsRef<Path>,
        provider: Option<&str>,
    ) -> AiResult<FileResponse> {
        Self::put(FileUpload::from_path(path).await?, provider).await
    }

    pub async fn put_from_storage(
        path: &str,
        disk: Option<&str>,
        provider: Option<&str>,
    ) -> AiResult<FileResponse> {
        Self::put(FileUpload::from_storage(path, disk).await?, provider).await
    }

    pub async fn delete(id: &str, provider: Option<&str>) -> AiResult<bool> {
        validate_id(id)?;
        Ai::file_provider(provider)?.delete_file(id).await
    }

    pub async fn list(
        after: Option<&str>,
        limit: u16,
        provider: Option<&str>,
    ) -> AiResult<FilePage> {
        if let Some(cursor) = after {
            validate_id(cursor)?;
        }
        if !(1..=100).contains(&limit) {
            return Err(AiError::InvalidConfiguration("files.limit"));
        }
        let provider = Ai::file_provider(provider)?;
        let page = provider.list_files(after, limit).await?;
        let files = page
            .data
            .into_iter()
            .map(|file| FileResponse::new(file, provider.clone()))
            .collect::<AiResult<_>>()?;
        if let Some(cursor) = &page.next_cursor {
            validate_id(cursor).map_err(|_| AiError::InvalidResponse)?;
            if Some(cursor.as_str()) == after {
                return Err(AiError::InvalidResponse);
            }
        }
        Ok(FilePage {
            files,
            next_cursor: page.next_cursor,
        })
    }
}

pub struct FileUpload {
    content: Bytes,
    name: String,
    mime_type: String,
    purpose: String,
}

impl FileUpload {
    pub fn from_bytes(content: impl Into<Bytes>, name: impl Into<String>) -> AiResult<Self> {
        let content = content.into();
        let name = name.into();
        if content.is_empty() || !valid_filename(&name) {
            return Err(AiError::InvalidFile);
        }
        if content.len() >= FILE_LIMIT {
            return Err(AiError::MediaTooLarge);
        }
        Ok(Self {
            content,
            name,
            mime_type: "application/octet-stream".into(),
            purpose: "user_data".into(),
        })
    }

    pub fn from_upload(file: &UploadedFile) -> AiResult<Self> {
        Self::from_bytes(file.get(), file.get_client_original_name())
    }

    pub async fn from_path(path: impl AsRef<Path>) -> AiResult<Self> {
        let path = path.as_ref();
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or(AiError::InvalidFile)?;
        let file = tokio::fs::File::open(path)
            .await
            .map_err(|_| AiError::MediaStorageUnavailable)?;
        if !file
            .metadata()
            .await
            .map_err(|_| AiError::MediaStorageUnavailable)?
            .is_file()
        {
            return Err(AiError::InvalidFile);
        }
        Self::from_bytes(media_storage::read_limited(file, FILE_LIMIT).await?, name)
    }

    pub async fn from_storage(path: &str, disk: Option<&str>) -> AiResult<Self> {
        let name = path.rsplit('/').next().ok_or(AiError::InvalidFile)?;
        let content = media_storage::read(path, disk, FILE_LIMIT)
            .await?
            .ok_or(AiError::InvalidFile)?;
        Self::from_bytes(content, name)
    }

    pub fn as_name(mut self, name: impl Into<String>) -> AiResult<Self> {
        let name = name.into();
        if !valid_filename(&name) {
            return Err(AiError::InvalidFile);
        }
        self.name = name;
        Ok(self)
    }

    pub fn with_mime_type(mut self, mime_type: impl Into<String>) -> AiResult<Self> {
        let mime_type = mime_type.into();
        Part::text("")
            .mime_str(&mime_type)
            .map_err(|_| AiError::InvalidFile)?;
        self.mime_type = mime_type;
        Ok(self)
    }

    pub fn purpose(mut self, purpose: impl Into<String>) -> AiResult<Self> {
        let purpose = purpose.into();
        validate_id(&purpose)?;
        self.purpose = purpose;
        Ok(self)
    }

    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn mime_type(&self) -> &str {
        &self.mime_type
    }
    pub fn content(&self) -> &[u8] {
        &self.content
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FileInfo {
    pub id: String,
    pub filename: Option<String>,
    pub bytes: Option<u64>,
    pub purpose: Option<String>,
    pub created_at: Option<u64>,
    pub expires_at: Option<u64>,
    pub status: Option<String>,
}

pub struct FileResponse {
    file: FileInfo,
    provider: Arc<dyn FileProvider>,
}

impl FileResponse {
    fn new(file: FileInfo, provider: Arc<dyn FileProvider>) -> AiResult<Self> {
        validate_id(&file.id).map_err(|_| AiError::InvalidResponse)?;
        Ok(Self { file, provider })
    }

    pub fn id(&self) -> &str {
        &self.file.id
    }

    pub fn info(&self) -> &FileInfo {
        &self.file
    }

    pub fn provider(&self) -> &str {
        self.provider.name()
    }

    pub(crate) fn uses_provider(&self, provider: &dyn FileProvider) -> bool {
        same_resource_scope(self.provider.as_ref(), provider)
    }

    pub async fn refresh(&self) -> AiResult<Self> {
        let file = self.provider.get_file(self.id()).await?;
        if file.id != self.id() {
            return Err(AiError::InvalidResponse);
        }
        Self::new(file, self.provider.clone())
    }

    pub async fn contents(&self) -> AiResult<Bytes> {
        validate_id(self.id())?;
        self.provider.file_contents(self.id()).await
    }

    pub async fn store_as(
        &self,
        directory: &str,
        name: &str,
        disk: Option<&str>,
    ) -> AiResult<String> {
        let contents = self.contents().await?;
        media_storage::write(
            &contents,
            "",
            directory,
            Some(name),
            disk,
            Some(Visibility::Private),
        )
        .await
    }

    pub async fn delete(&self) -> AiResult<bool> {
        validate_id(self.id())?;
        self.provider.delete_file(self.id()).await
    }
}

pub struct FilePage {
    pub files: Vec<FileResponse>,
    pub next_cursor: Option<String>,
}

pub struct ProviderFilePage {
    pub data: Vec<FileInfo>,
    pub next_cursor: Option<String>,
}

pub trait FileProvider: Send + Sync {
    fn name(&self) -> &str;
    fn resource_scope(&self) -> Option<&Arc<()>> {
        None
    }

    fn get_file<'a>(&'a self, id: &'a str) -> HttpFuture<'a, AiResult<FileInfo>>;
    fn put_file(&self, file: FileUpload) -> HttpFuture<'_, AiResult<FileInfo>>;
    fn delete_file<'a>(&'a self, id: &'a str) -> HttpFuture<'a, AiResult<bool>>;
    fn file_contents<'a>(&'a self, _id: &'a str) -> HttpFuture<'a, AiResult<Bytes>> {
        Box::pin(async { Err(AiError::EndpointNotSupported) })
    }
    fn list_files<'a>(
        &'a self,
        _after: Option<&'a str>,
        _limit: u16,
    ) -> HttpFuture<'a, AiResult<ProviderFilePage>> {
        Box::pin(async { Err(AiError::EndpointNotSupported) })
    }
}

pub(crate) struct OpenAiFileProvider(pub(crate) Arc<Connection>);

impl FileProvider for OpenAiFileProvider {
    fn name(&self) -> &str {
        self.0.name()
    }

    fn resource_scope(&self) -> Option<&Arc<()>> {
        Some(self.0.resource_scope())
    }

    fn get_file<'a>(&'a self, id: &'a str) -> HttpFuture<'a, AiResult<FileInfo>> {
        Box::pin(async move {
            let path = file_path(id)?;
            validate(self.0.request(None)?.get(&path).await)?
                .json()
                .map_err(|_| AiError::InvalidResponse)
        })
    }

    fn put_file(&self, file: FileUpload) -> HttpFuture<'_, AiResult<FileInfo>> {
        Box::pin(async move {
            let part = Part::stream_with_length(file.content.clone(), file.content.len() as u64)
                .file_name(file.name)
                .mime_str(&file.mime_type)
                .map_err(|_| AiError::InvalidFile)?;
            let response = self
                .0
                .request(None)?
                .attach("file", part)
                .post("files", &serde_json::json!({"purpose": file.purpose}))
                .await;
            validate(response)?
                .json()
                .map_err(|_| AiError::InvalidResponse)
        })
    }

    fn delete_file<'a>(&'a self, id: &'a str) -> HttpFuture<'a, AiResult<bool>> {
        Box::pin(async move {
            let path = file_path(id)?;
            let response = self.0.request(None)?.send(Method::DELETE, &path).await;
            let response: Deleted = validate(response)?
                .json()
                .map_err(|_| AiError::InvalidResponse)?;
            if response.id != id {
                return Err(AiError::InvalidResponse);
            }
            Ok(response.deleted)
        })
    }

    fn file_contents<'a>(&'a self, id: &'a str) -> HttpFuture<'a, AiResult<Bytes>> {
        Box::pin(async move {
            let path = format!("{}/content", file_path(id)?);
            let response = self.0.request(None)?.stream().get(&path).await;
            let mut stream = validate(response)?.into_stream().map_err(transport_error)?;
            let mut content = BytesMut::new();
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.map_err(transport_error)?;
                if content.len().saturating_add(chunk.len()) >= FILE_LIMIT {
                    return Err(AiError::MediaTooLarge);
                }
                content.extend_from_slice(&chunk);
            }
            Ok(content.freeze())
        })
    }

    fn list_files<'a>(
        &'a self,
        after: Option<&'a str>,
        limit: u16,
    ) -> HttpFuture<'a, AiResult<ProviderFilePage>> {
        Box::pin(async move {
            let mut query = vec![("limit".into(), limit.to_string())];
            if let Some(after) = after {
                validate_id(after)?;
                query.push(("after".into(), after.into()));
            }
            let response = self
                .0
                .request(None)?
                .with_query_parameters(query)
                .get("files")
                .await;
            let page: OpenAiPage<FileInfo> = validate(response)?
                .json()
                .map_err(|_| AiError::InvalidResponse)?;
            let (data, next_cursor) = page.decode(after, limit, |file| &file.id)?;
            Ok(ProviderFilePage { data, next_cursor })
        })
    }
}

#[derive(Deserialize)]
pub(crate) struct OpenAiPage<T> {
    data: Vec<T>,
    has_more: bool,
    last_id: Option<String>,
}

impl<T> OpenAiPage<T> {
    pub(crate) fn decode(
        self,
        after: Option<&str>,
        limit: u16,
        id: impl Fn(&T) -> &str,
    ) -> AiResult<(Vec<T>, Option<String>)> {
        if self.data.len() > usize::from(limit) {
            return Err(AiError::InvalidResponse);
        }
        let cursor = if self.has_more {
            let cursor = self.last_id.ok_or(AiError::InvalidResponse)?;
            validate_id(&cursor).map_err(|_| AiError::InvalidResponse)?;
            if self.data.last().map(id) != Some(&cursor) || after == Some(&cursor) {
                return Err(AiError::InvalidResponse);
            }
            Some(cursor)
        } else {
            None
        };
        Ok((self.data, cursor))
    }
}

pub(crate) fn same_resource_scope(first: &dyn FileProvider, second: &dyn FileProvider) -> bool {
    first
        .resource_scope()
        .zip(second.resource_scope())
        .is_some_and(|(first, second)| Arc::ptr_eq(first, second))
}

#[derive(Deserialize)]
pub(crate) struct Deleted {
    pub(crate) id: String,
    pub(crate) deleted: bool,
}

fn file_path(id: &str) -> AiResult<String> {
    validate_id(id)?;
    Ok(format!("files/{id}"))
}

fn valid_filename(name: &str) -> bool {
    !name.trim().is_empty()
        && !matches!(name, "." | "..")
        && !name.contains(['/', '\\'])
        && !name.chars().any(char::is_control)
}

pub(crate) fn validate_id(id: &str) -> AiResult<()> {
    if id.is_empty()
        || id.len() > 255
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err(AiError::InvalidFile);
    }
    Ok(())
}
