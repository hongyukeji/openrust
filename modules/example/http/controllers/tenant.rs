//! 认证 → 主库成员校验 → 可信租户层；不把请求 selector 直接提升为租户身份。
use crate::modules::example::services::tenant::{RecordView, TenantRecords};
use openrust::auth::CurrentUser;
#[cfg(any(feature = "storage-local", feature = "storage-s3"))]
use openrust::http::axum::http::{HeaderValue, header};
use openrust::http::axum::{
    extract::{FromRequestParts, Request},
    middleware::Next,
    response::{IntoResponse, Response},
};
use openrust::http::{Inject, Json};
use openrust::tenant::{TenantId, TenantLayer};
use openrust::{Error, Result};
use std::sync::Arc;
#[derive(Clone)]
struct VerifiedTenant(TenantId);
pub(crate) fn trusted_layer() -> TenantLayer {
    TenantLayer::new(|request| {
        request
            .extensions()
            .get::<VerifiedTenant>()
            .map(|verified| verified.0.clone())
            .ok_or_else(|| Error::Forbidden("需要已验证的租户成员关系".into()))
    })
}
pub(crate) async fn verify_membership(request: Request, next: Next) -> Response {
    let (mut parts, body) = request.into_parts();
    let result = async {
        let user = CurrentUser::from_request_parts(&mut parts, &()).await?;
        if parts.headers.get_all("x-example-tenant").iter().count() != 1 {
            return Err(Error::Forbidden("需要唯一的租户选择值".into()));
        }
        let tenant = TenantId::new(
            parts
                .headers
                .get("x-example-tenant")
                .and_then(|value| value.to_str().ok())
                .ok_or_else(|| Error::Forbidden("缺少租户选择值".into()))?,
        )
        .map_err(|_| Error::Forbidden("租户选择值无效".into()))?;
        let scope = parts
            .extensions
            .get::<Arc<openrust::core::container::Scope>>()
            .ok_or_else(|| {
                Error::custom("example_tenant_scope_missing", "租户验证需要生产请求作用域")
            })?;
        scope
            .resolve::<TenantRecords>()
            .await?
            .verify_membership(user.id(), &tenant)
            .await?;
        parts.extensions.insert(VerifiedTenant(tenant));
        Ok(())
    }
    .await;
    if let Err(error) = result {
        return error.into_response();
    }
    next.run(Request::from_parts(parts, body)).await
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RecordInput {
    value: String,
}
pub(crate) async fn show(
    user: CurrentUser,
    Inject(records): Inject<TenantRecords>,
) -> Result<Json<RecordView>> {
    Ok(Json(records.get(&user).await?))
}
pub(crate) async fn update(
    user: CurrentUser,
    Inject(records): Inject<TenantRecords>,
    Json(input): Json<RecordInput>,
) -> Result<Json<serde_json::Value>> {
    records.put(&user, input.value).await?;
    Ok(Json(serde_json::json!({"stored":true})))
}
#[cfg(feature = "queue")]
pub(crate) async fn dispatch(
    user: CurrentUser,
    Inject(records): Inject<TenantRecords>,
) -> Result<Json<serde_json::Value>> {
    Ok(Json(
        serde_json::json!({"job_id":records.dispatch(user.id()).await?}),
    ))
}
#[cfg(any(feature = "storage-local", feature = "storage-s3"))]
pub(crate) async fn write_file(
    user: CurrentUser,
    Inject(records): Inject<TenantRecords>,
) -> Result<Json<serde_json::Value>> {
    records.write_file(user.id()).await?;
    Ok(Json(serde_json::json!({"stored":true})))
}
#[cfg(any(feature = "storage-local", feature = "storage-s3"))]
pub(crate) async fn read_file(
    user: CurrentUser,
    Inject(records): Inject<TenantRecords>,
) -> Result<Response> {
    let reader = records.read_file(user.id()).await?;
    let mut response = Response::new(openrust::http::axum::body::Body::from_stream(reader.body));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    response.headers_mut().insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    Ok(response)
}
