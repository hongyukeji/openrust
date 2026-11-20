use openrust::{
    auth::Auth,
    http::{HttpError, Request, Response, StatusCode},
};
use openrust_oidc::facades::Oidc;
use serde_json::json;

fn internal(error: impl std::error::Error + Send + Sync + 'static) -> HttpError {
    HttpError::Internal {
        source: Box::new(error),
    }
}

/// Register on a POST route in the native web group with CSRF protection.
pub async fn logout(request: &mut Request, federated: bool) -> Result<Response, HttpError> {
    let hint = Oidc::provider("work", request)
        .map_err(internal)?
        .takeLogoutHint()
        .ok();
    Auth::sessionGuard(request, Some("web"))
        .map_err(internal)?
        .logoutAndInvalidate()
        .await
        .map_err(internal)?;
    if federated
        && let Some(hint) = hint
        && let Ok(url) = Oidc::provider("work", request)
            .map_err(internal)?
            .logoutUrl(&hint)
            .await
    {
        return Ok(
            openrust::support::facades::Redirect::away(&url, Default::default())
                .map_err(internal)?
                .into(),
        );
    }
    Response::json(StatusCode::OK, json!({"local":true,"federated":false}))
}
