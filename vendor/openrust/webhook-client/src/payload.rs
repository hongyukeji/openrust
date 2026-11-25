use crate::{Error, Result, VerifiedWebhook};
use http::{HeaderValue, Method, Uri, header::CONTENT_TYPE};
use openrust::http::{Body, Request, UploadedFileValue};

pub(crate) struct ReceiptPayload {
    pub input: serde_json::Value,
    pub attachment_media_type: Option<String>,
}

impl ReceiptPayload {
    pub async fn prepare(webhook: &VerifiedWebhook<'_>) -> Result<Self> {
        let request = webhook.request();
        let media_type = request
            .headers()
            .get(CONTENT_TYPE)
            .map(|header| header.to_str().map_err(|_| Error::InvalidRecord))
            .transpose()?;
        let mut native =
            original_request(request.method(), request.uri(), request.body(), media_type)?;
        let input = native.input(None).await?.unwrap_or_default();
        let attachment_media_type = if webhook.config().store_attachments()?
            && media_type.is_some_and(is_multipart)
            && matches!(native.all_files().await?, UploadedFileValue::Files(files) if !files.is_empty())
        {
            media_type.map(str::to_owned)
        } else {
            None
        };
        Ok(Self {
            input,
            attachment_media_type,
        })
    }
}

pub(crate) async fn attachments(
    method: &Method,
    uri: &str,
    body: &[u8],
    media_type: &str,
) -> Result<UploadedFileValue> {
    if !is_multipart(media_type) {
        return Err(Error::InvalidRecord);
    }
    let uri = uri.parse().map_err(|_| Error::InvalidRecord)?;
    Ok(original_request(method, &uri, body, Some(media_type))?
        .all_files()
        .await?)
}

fn is_multipart(value: &str) -> bool {
    value
        .split(';')
        .next()
        .is_some_and(|media| media.trim().eq_ignore_ascii_case("multipart/form-data"))
}

fn original_request(
    method: &Method,
    uri: &Uri,
    body: &[u8],
    media_type: Option<&str>,
) -> Result<Request> {
    let mut request = http::Request::new(Body::from(body.to_vec()));
    *request.method_mut() = method.clone();
    *request.uri_mut() = uri.clone();
    if let Some(media_type) = media_type {
        request.headers_mut().insert(
            CONTENT_TYPE,
            HeaderValue::from_str(media_type).map_err(|_| Error::InvalidRecord)?,
        );
    }
    Ok(request.into())
}
