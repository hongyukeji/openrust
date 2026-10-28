//! 文章附件 HTTP 边界；身份来自标准认证，文件名和下载头由服务端确定。

use openrust::Result;
use openrust::auth::CurrentUser;
use openrust::http::axum::{
    body::Body,
    extract::Request,
    http::{HeaderValue, StatusCode, header},
    response::Response,
};
use openrust::http::{Created, Inject, Json, Path};

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
        .upload(user.id(), id, declaration, request.into_body())
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
    let reader = attachments.open(user.id(), id).await?;
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
    attachments.delete(user.id(), id).await?;
    Ok(StatusCode::NO_CONTENT)
}
