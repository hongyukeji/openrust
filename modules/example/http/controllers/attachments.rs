//! 文章附件 HTTP 边界；身份来自标准认证，文件名和下载头由服务端确定。

use openrust::Result;
use openrust::auth::CurrentUser;
use openrust::http::axum::{
    body::Body,
    extract::Request,
    http::{HeaderValue, StatusCode, header},
    response::Response,
};
use openrust::http::{Created, Inject, Json, Path, Request as InputRequest, UploadedFile};
use openrust::storage::Visibility;
use tokio::io::AsyncReadExt;

use crate::modules::example::http::requests::attachments::AttachmentUpload;
use crate::modules::example::services::attachments::ArticleAttachments;

#[derive(serde::Serialize)]
pub(crate) struct AttachmentInfo {
    bytes: u64,
    content_type: &'static str,
}

pub(crate) async fn upload(
    user: CurrentUser,
    Inject(attachments): Inject<ArticleAttachments>,
    Path(id): Path<i64>,
    request: Request,
) -> Result<Created<Json<AttachmentInfo>>> {
    let declaration = AttachmentUpload::from_headers(request.headers())?;
    let stored = attachments
        .upload(user.integer_id()?, id, declaration, request.into_body())
        .await?;
    Ok(Created(Json(AttachmentInfo {
        bytes: stored.metadata.size,
        content_type: "text/plain; charset=utf-8",
    })))
}

/// 通过公开请求文件 API 上传普通业务附件，沿原服务执行授权和对象发布。
pub(crate) async fn upload_multipart(
    user: CurrentUser,
    Inject(attachments): Inject<ArticleAttachments>,
    Path(id): Path<i64>,
    request: InputRequest,
) -> Result<Created<Json<AttachmentInfo>>> {
    let file = request
        .file("document")?
        .ok_or_else(|| openrust::Error::custom("example_attachment_missing", "缺少附件文件"))?;
    let file = file
        .downcast_ref::<UploadedFile>()
        .ok_or_else(|| openrust::Error::custom("example_attachment_invalid", "附件字段不是文件"))?;
    if file.size() > crate::modules::example::http::requests::attachments::MAX_ATTACHMENT_BYTES {
        return Err(crate::modules::example::http::requests::attachments::too_large());
    }
    let mime = file.mime_type().await?.unwrap_or_default();
    let declaration = AttachmentUpload::from_content_type(&mime)?;
    let reader = file.open().await?;
    let body = Body::from_stream(futures_util::stream::unfold(
        Some(reader),
        |reader| async move {
            let mut reader = reader?;
            let mut buffer = vec![0_u8; 16 * 1024];
            match reader.read(&mut buffer).await {
                Ok(0) => None,
                Ok(size) => {
                    buffer.truncate(size);
                    Some((Ok::<_, std::io::Error>(buffer), Some(reader)))
                }
                Err(error) => Some((Err(error), None)),
            }
        },
    ));
    let stored = attachments
        .upload(user.integer_id()?, id, declaration, body)
        .await?;
    Ok(Created(Json(AttachmentInfo {
        bytes: stored.metadata.size,
        content_type: "text/plain; charset=utf-8",
    })))
}

pub(crate) async fn download(
    user: CurrentUser,
    Inject(attachments): Inject<ArticleAttachments>,
    Path(id): Path<i64>,
) -> Result<Response> {
    let reader = attachments.open(user.integer_id()?, id).await?;
    let mut response = Response::new(Body::from_stream(reader.body));
    *response.status_mut() = StatusCode::OK;
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_static("attachment; filename=\"article-note.txt\""),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    Ok(response)
}

pub(crate) async fn delete(
    user: CurrentUser,
    Inject(attachments): Inject<ArticleAttachments>,
    Path(id): Path<i64>,
) -> Result<StatusCode> {
    attachments.delete(user.integer_id()?, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(serde::Deserialize)]
pub(crate) struct AttachmentVisibilityInput {
    visibility: Visibility,
}

pub(crate) async fn set_visibility(
    user: CurrentUser,
    Inject(attachments): Inject<ArticleAttachments>,
    Path(id): Path<i64>,
    Json(input): Json<AttachmentVisibilityInput>,
) -> Result<StatusCode> {
    attachments
        .set_visibility(user.integer_id()?, id, input.visibility)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
