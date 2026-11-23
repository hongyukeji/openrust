use openrust::{
    contracts::container::Container as _,
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
};
use openrust_socialite::SocialiteManager;

pub fn redirect(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let manager = openrust::container::Container::get_instance()
            .make::<SocialiteManager>()
            .map_err(|_| HttpError::InvalidRequest)?;
        let response = manager
            .driver("github", request)
            .map_err(|_| HttpError::InvalidRequest)?
            .redirect()
            .await
            .map_err(|_| HttpError::InvalidRequest)?;
        Ok(response.into())
    })
}

pub fn callback(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let manager = openrust::container::Container::get_instance()
            .make::<SocialiteManager>()
            .map_err(|_| HttpError::InvalidRequest)?;
        let user = manager
            .driver("github", request)
            .map_err(|_| HttpError::InvalidRequest)?
            .user()
            .await
            .map_err(|_| HttpError::InvalidRequest)?;
        Response::json(
            StatusCode::OK,
            serde_json::json!({
                "id": user.getId(), "name": user.getName(), "email": user.getEmail(),
            }),
        )
    })
}
