use std::{borrow::Cow, sync::Arc};

use openrust::http::{HttpFuture, Method, client::PendingRequest};
use serde::Deserialize;
use serde_json::json;
use url::Url;

use crate::{AiError, AiResult, connection::Connection, text::validate};

use super::{FileInfo, FileProvider, FileUpload, ProviderFilePage, parse_timestamp};

pub(crate) struct GeminiFileProvider(pub(crate) Arc<Connection>);

impl FileProvider for GeminiFileProvider {
    fn name(&self) -> &str {
        self.0.name()
    }

    fn resource_scope(&self) -> Option<&Arc<()>> {
        Some(self.0.resource_scope())
    }

    fn canonical_id<'a>(&self, id: &'a str) -> AiResult<Cow<'a, str>> {
        let name = id.strip_prefix("files/").unwrap_or(id);
        validate_gemini_id(name)?;
        Ok(if name.len() == id.len() {
            format!("files/{name}").into()
        } else {
            id.into()
        })
    }

    fn validate_cursor(&self, cursor: &str) -> AiResult<()> {
        if cursor.is_empty()
            || cursor.len() > 4096
            || !cursor.bytes().all(|byte| byte.is_ascii_graphic())
        {
            return Err(AiError::InvalidFile);
        }
        Ok(())
    }

    fn get_file<'a>(&'a self, id: &'a str) -> HttpFuture<'a, AiResult<FileInfo>> {
        Box::pin(async move {
            let path = self.canonical_id(id)?;
            let file: Metadata = validate(self.0.request(None)?.get(&path).await)?
                .json()
                .map_err(|_| AiError::InvalidResponse)?;
            file.try_into()
        })
    }

    fn put_file(&self, file: FileUpload) -> HttpFuture<'_, AiResult<FileInfo>> {
        Box::pin(async move {
            if file
                .purpose
                .as_deref()
                .is_some_and(|purpose| purpose != "user_data")
                || file.name.chars().count() > 512
            {
                return Err(AiError::InvalidFile);
            }
            let base = Url::parse(self.0.base_url()).map_err(|_| AiError::InvalidResponse)?;
            let upload = upload_endpoint(&base)?;
            let request =
                upload_header(self.0.request(None)?, "x-goog-upload-protocol", "resumable")?;
            let request = upload_header(request, "x-goog-upload-command", "start")?;
            let request = upload_header(
                request,
                "x-goog-upload-header-content-length",
                &file.content.len().to_string(),
            )?;
            let request = upload_header(
                request,
                "x-goog-upload-header-content-type",
                &file.mime_type,
            )?;
            let response = validate(
                request
                    .post(upload.as_str(), &json!({"file":{"displayName": file.name}}))
                    .await,
            )?;
            let session = response
                .headers()
                .get("x-goog-upload-url")
                .and_then(|header| header.to_str().ok())
                .and_then(|value| Url::parse(value).ok())
                .filter(|url| trusted_session(&upload, url))
                .ok_or(AiError::InvalidResponse)?;
            let request = upload_header(
                self.0.request(None)?,
                "x-goog-upload-command",
                "upload, finalize",
            )?;
            let request = upload_header(request, "x-goog-upload-offset", "0")?;
            let response: Uploaded = validate(
                request
                    .with_body(
                        file.content,
                        file.mime_type.parse().map_err(|_| AiError::InvalidFile)?,
                    )
                    .send(Method::POST, session.as_str())
                    .await,
            )?
            .json()
            .map_err(|_| AiError::InvalidResponse)?;
            response.file.try_into()
        })
    }

    fn delete_file<'a>(&'a self, id: &'a str) -> HttpFuture<'a, AiResult<bool>> {
        Box::pin(async move {
            let path = self.canonical_id(id)?;
            validate(self.0.request(None)?.send(Method::DELETE, &path).await)?;
            Ok(true)
        })
    }

    fn list_files<'a>(
        &'a self,
        after: Option<&'a str>,
        limit: u16,
    ) -> HttpFuture<'a, AiResult<ProviderFilePage>> {
        Box::pin(async move {
            let mut query = vec![("pageSize".into(), limit.to_string())];
            if let Some(cursor) = after {
                query.push(("pageToken".into(), cursor.into()));
            }
            let page: Page = validate(
                self.0
                    .request(None)?
                    .with_query_parameters(query)
                    .get("files")
                    .await,
            )?
            .json()
            .map_err(|_| AiError::InvalidResponse)?;
            if page.files.len() > usize::from(limit) {
                return Err(AiError::InvalidResponse);
            }
            Ok(ProviderFilePage {
                data: page
                    .files
                    .into_iter()
                    .map(TryInto::try_into)
                    .collect::<AiResult<_>>()?,
                next_cursor: page.next_page_token.filter(|token| !token.is_empty()),
            })
        })
    }
}

fn upload_endpoint(base: &Url) -> AiResult<Url> {
    let path = base.path().trim_end_matches('/');
    let (prefix, version) = path
        .rsplit_once('/')
        .ok_or(AiError::InvalidConfiguration("providers.<name>.url"))?;
    if !matches!(version, "v1beta" | "v1") {
        return Err(AiError::InvalidConfiguration("providers.<name>.url"));
    }
    let mut endpoint = base.clone();
    endpoint.set_path(&format!("{prefix}/upload/{version}/files"));
    Ok(endpoint)
}

fn trusted_session(upload: &Url, session: &Url) -> bool {
    session.origin() == upload.origin()
        && session.path() == upload.path()
        && session.username().is_empty()
        && session.password().is_none()
        && session.fragment().is_none()
        && session.query().is_some()
}

fn upload_header(
    request: PendingRequest,
    name: &'static str,
    value: &str,
) -> AiResult<PendingRequest> {
    let value = value.parse().map_err(|_| AiError::InvalidFile)?;
    Ok(request.with_header(name.parse().expect("valid header name"), value))
}

pub(crate) fn validate_gemini_id(name: &str) -> AiResult<()> {
    if name.is_empty()
        || name.len() > 40
        || name.starts_with('-')
        || name.ends_with('-')
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err(AiError::InvalidFile);
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Metadata {
    name: String,
    display_name: Option<String>,
    mime_type: Option<String>,
    size_bytes: Option<String>,
    create_time: Option<String>,
    expiration_time: Option<String>,
    state: Option<String>,
}

impl TryFrom<Metadata> for FileInfo {
    type Error = AiError;

    fn try_from(file: Metadata) -> AiResult<Self> {
        validate_gemini_id(
            file.name
                .strip_prefix("files/")
                .ok_or(AiError::InvalidResponse)?,
        )
        .map_err(|_| AiError::InvalidResponse)?;
        Ok(Self {
            id: file.name,
            filename: file.display_name,
            mime_type: file.mime_type,
            bytes: file
                .size_bytes
                .map(|value| value.parse().map_err(|_| AiError::InvalidResponse))
                .transpose()?,
            purpose: None,
            created_at: parse_timestamp(file.create_time)?,
            expires_at: parse_timestamp(file.expiration_time)?,
            downloadable: Some(false),
            status: file.state,
        })
    }
}

#[derive(Deserialize)]
struct Uploaded {
    file: Metadata,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Page {
    #[serde(default)]
    files: Vec<Metadata>,
    next_page_token: Option<String>,
}
