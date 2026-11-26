use openrust::{
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    support::facades::Facade,
};
use openrust_webhook_client::DatabaseWebhookStore;
use serde_json::json;

pub(super) fn inspect(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let scope = request.route("scope").ok_or(HttpError::NotFound)?;
        if scope != "webhook-acceptance"
            || request
                .headers()
                .get("x-fixture-inspector")
                .and_then(|value| value.to_str().ok())
                != Some("fixture-operator")
        {
            return Response::json(StatusCode::FORBIDDEN, json!({}));
        }
        let id = request.route("id").ok_or(HttpError::NotFound)?;
        let manager = super::Databases::get_facade_root().map_err(super::internal)?;
        let store =
            DatabaseWebhookStore::new(manager.connection(None).map_err(super::internal)?, scope)
                .map_err(super::internal)?;
        let value = match request.route("kind") {
            Some("receipt") => {
                serde_json::to_value(store.inspect_receipt(id).await.map_err(super::internal)?)
            }
            Some("attempts") => serde_json::to_value(
                store
                    .inspect_processing_attempts(id, 0, 1)
                    .await
                    .map_err(super::internal)?,
            ),
            _ => return Err(HttpError::NotFound),
        }
        .map_err(super::internal)?;
        if value.is_null() {
            return Response::json(StatusCode::NOT_FOUND, json!({}));
        }
        Response::json(StatusCode::OK, value)
    })
}
