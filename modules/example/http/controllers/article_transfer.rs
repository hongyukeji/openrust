//! 认证文章文件 API；文件路径、所有者与下载文件名由服务端控制。

use crate::modules::example::http::requests::article_transfer::{
    ArticleFormat, MAX_TRANSFER_BYTES, TransferQuery, too_large,
};
use crate::modules::example::services::article_transfer::{ArticleTransfer, TransferReport};
use openrust::Result;
use openrust::auth::CurrentUser;
use openrust::http::axum::{
    body::{Body, to_bytes},
    extract::Request,
    http::{HeaderValue, header},
    response::Response,
};
use openrust::http::{Inject, Json, Query};

pub(crate) async fn import(
    user: CurrentUser,
    Inject(transfer): Inject<ArticleTransfer>,
    Query(query): Query<TransferQuery>,
    request: Request,
) -> Result<Json<TransferReport>> {
    query.format.validate_headers(request.headers())?;
    let bytes = to_bytes(request.into_body(), MAX_TRANSFER_BYTES)
        .await
        .map_err(|_| too_large())?;
    Ok(Json(
        transfer
            .import(user.integer_id()?, query.format, bytes.to_vec())
            .await?,
    ))
}
pub(crate) async fn export(
    user: CurrentUser,
    Inject(transfer): Inject<ArticleTransfer>,
    Query(query): Query<TransferQuery>,
) -> Result<Response> {
    let bytes = transfer.export(user.integer_id()?, query.format).await?;
    Ok(download_response(Body::from(bytes), query.format))
}
fn download_response(body: Body, format: ArticleFormat) -> Response {
    let mut response = Response::new(body);
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(format.mime()),
    );
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_static(format.disposition()),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    response
}

#[cfg(all(feature = "data-exchange-tasks", feature = "storage-local"))]
#[derive(serde::Serialize)]
pub(crate) struct ExportQueued {
    job_id: String,
}
#[cfg(all(feature = "data-exchange-tasks", feature = "storage-local"))]
pub(crate) async fn dispatch_export(
    user: CurrentUser,
    Inject(queue): Inject<openrust::queue::Queue>,
    Query(query): Query<TransferQuery>,
) -> Result<openrust::http::Created<Json<ExportQueued>>> {
    let job_id = crate::modules::example::services::article_transfer::dispatch_export(
        &queue,
        user.integer_id()?,
        query.format,
    )
    .await?;
    Ok(openrust::http::Created(Json(ExportQueued { job_id })))
}
#[cfg(all(feature = "data-exchange-tasks", feature = "storage-local"))]
pub(crate) async fn download_export(
    user: CurrentUser,
    Inject(storage): Inject<openrust::storage::Storage>,
    openrust::http::Path((job_id, attempt)): openrust::http::Path<(String, u64)>,
    Query(query): Query<TransferQuery>,
) -> Result<Response> {
    let reader = crate::modules::example::services::article_transfer::open_export(
        &storage,
        user.integer_id()?,
        query.format,
        &job_id,
        attempt,
    )
    .await?;
    Ok(download_response(
        Body::from_stream(reader.body),
        query.format,
    ))
}

#[cfg(all(feature = "data-exchange-tasks", feature = "storage-local"))]
pub(crate) async fn dispatch_import(
    user: CurrentUser,
    Inject(db): Inject<openrust::Db>,
    Inject(storage): Inject<openrust::storage::Storage>,
    Inject(queue): Inject<openrust::queue::Queue>,
    Query(query): Query<TransferQuery>,
    request: Request,
) -> Result<
    openrust::http::Created<
        Json<crate::modules::example::services::article_transfer::ImportQueued>,
    >,
> {
    query.format.validate_headers(request.headers())?;
    let bytes = to_bytes(request.into_body(), MAX_TRANSFER_BYTES)
        .await
        .map_err(|_| too_large())?;
    let queued = crate::modules::example::services::article_transfer::upload_import(
        &db,
        &storage,
        &queue,
        user.integer_id()?,
        query.format,
        bytes.to_vec(),
    )
    .await?;
    Ok(openrust::http::Created(Json(queued)))
}

#[cfg(all(feature = "data-exchange-tasks", feature = "storage-local"))]
pub(crate) async fn import_report(
    user: CurrentUser,
    Inject(storage): Inject<openrust::storage::Storage>,
    openrust::http::Path((import_id, job_id, attempt)): openrust::http::Path<(String, String, u64)>,
) -> Result<Response> {
    let reader = crate::modules::example::services::article_transfer::open_import_report(
        &storage,
        user.integer_id()?,
        &import_id,
        &job_id,
        attempt,
    )
    .await?;
    let mut response = Response::new(Body::from_stream(reader.body));
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
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

#[cfg(all(feature = "data-exchange-tasks", feature = "storage-local"))]
pub(crate) async fn import_status(
    user: CurrentUser,
    Inject(db): Inject<openrust::Db>,
    openrust::http::Path(import_id): openrust::http::Path<String>,
) -> Result<Json<crate::modules::example::services::article_transfer::ImportStatus>> {
    Ok(Json(
        crate::modules::example::services::article_transfer::import_status(
            &db,
            user.integer_id()?,
            &import_id,
        )
        .await?,
    ))
}
