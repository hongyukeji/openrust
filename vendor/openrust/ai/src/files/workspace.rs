use std::sync::Arc;

use bytes::Bytes;
use openrust::http::{HttpFuture, Method, client::PendingRequest};
use serde::Deserialize;
use serde_json::json;

use crate::{AiError, AiResult, connection::Connection, text::validate};

use super::{
    FileInfo, FileProvider, FileUpload, OpenAiPage, ProviderFilePage, file_path, parse_timestamp,
    read_file_contents, validate_id,
};

#[derive(Clone, Copy)]
pub(crate) enum WorkspaceFileProtocol {
    Anthropic,
    OpenRouter,
}

pub(crate) struct WorkspaceFileProvider {
    pub(crate) connection: Arc<Connection>,
    pub(crate) protocol: WorkspaceFileProtocol,
}

impl WorkspaceFileProvider {
    fn request(&self) -> AiResult<PendingRequest> {
        let request = self.connection.request(None)?;
        Ok(match self.protocol {
            WorkspaceFileProtocol::Anthropic => request.with_header(
                "anthropic-beta".parse().expect("valid header name"),
                "files-api-2025-04-14".parse().expect("valid beta version"),
            ),
            WorkspaceFileProtocol::OpenRouter => request,
        })
    }
}

impl FileProvider for WorkspaceFileProvider {
    fn name(&self) -> &str {
        self.connection.name()
    }

    fn resource_scope(&self) -> Option<&Arc<()>> {
        Some(self.connection.resource_scope())
    }

    fn get_file<'a>(&'a self, id: &'a str) -> HttpFuture<'a, AiResult<FileInfo>> {
        Box::pin(async move {
            let file: Metadata = validate(self.request()?.get(&file_path(id)?).await)?
                .json()
                .map_err(|_| AiError::InvalidResponse)?;
            file.try_into()
        })
    }

    fn put_file(&self, file: FileUpload) -> HttpFuture<'_, AiResult<FileInfo>> {
        Box::pin(async move {
            if file.purpose != "user_data"
                || file.name.chars().count() > 255
                || file.name.contains(['<', '>', ':', '"', '|', '?', '*'])
            {
                return Err(AiError::InvalidFile);
            }
            let (part, _) = file.into_part()?;
            let file: Metadata = validate(
                self.request()?
                    .attach("file", part)
                    .post("files", &json!({}))
                    .await,
            )?
            .json()
            .map_err(|_| AiError::InvalidResponse)?;
            file.try_into()
        })
    }

    fn delete_file<'a>(&'a self, id: &'a str) -> HttpFuture<'a, AiResult<bool>> {
        Box::pin(async move {
            let result: DeletedFile =
                validate(self.request()?.send(Method::DELETE, &file_path(id)?).await)?
                    .json()
                    .map_err(|_| AiError::InvalidResponse)?;
            if result.id != id || result.kind != "file_deleted" {
                return Err(AiError::InvalidResponse);
            }
            Ok(true)
        })
    }

    fn file_contents<'a>(&'a self, id: &'a str) -> HttpFuture<'a, AiResult<Bytes>> {
        Box::pin(async move {
            let path = format!("{}/content", file_path(id)?);
            let response = validate(self.request()?.stream().get(&path).await)?;
            read_file_contents(response).await
        })
    }

    fn list_files<'a>(
        &'a self,
        after: Option<&'a str>,
        limit: u16,
    ) -> HttpFuture<'a, AiResult<ProviderFilePage>> {
        Box::pin(async move {
            let mut query = vec![("limit".into(), limit.to_string())];
            if let Some(cursor) = after {
                validate_id(cursor)?;
                query.push(("after_id".into(), cursor.into()));
            }
            let response = self
                .request()?
                .with_query_parameters(query)
                .get("files")
                .await;
            let page: OpenAiPage<Metadata> = validate(response)?
                .json()
                .map_err(|_| AiError::InvalidResponse)?;
            let (data, next_cursor) = page.decode(after, limit, |item| &item.id)?;
            Ok(ProviderFilePage {
                data: data
                    .into_iter()
                    .map(TryInto::try_into)
                    .collect::<AiResult<_>>()?,
                next_cursor,
            })
        })
    }
}

#[derive(Deserialize)]
struct Metadata {
    id: String,
    filename: Option<String>,
    mime_type: Option<String>,
    size_bytes: Option<u64>,
    created_at: Option<String>,
    expires_at: Option<String>,
    downloadable: Option<bool>,
}

impl TryFrom<Metadata> for FileInfo {
    type Error = AiError;

    fn try_from(file: Metadata) -> AiResult<Self> {
        Ok(Self {
            id: file.id,
            filename: file.filename,
            mime_type: file.mime_type,
            bytes: file.size_bytes,
            purpose: None,
            created_at: parse_timestamp(file.created_at)?,
            expires_at: parse_timestamp(file.expires_at)?,
            downloadable: file.downloadable,
            status: None,
        })
    }
}

#[derive(Deserialize)]
struct DeletedFile {
    id: String,
    #[serde(rename = "type")]
    kind: String,
}
