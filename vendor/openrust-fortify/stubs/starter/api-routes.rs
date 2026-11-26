use openrust::routing::RoutingError;

pub(crate) fn register() -> Result<(), RoutingError> {
    openrust::support::facades::Route::get("/",crate::app::http::controllers::api_controller::ApiController::index)?;
    #[cfg(feature = "sanctum")]
    openrust::support::facades::Route::get_with("/user", user, |route| {
        route.middleware_name("auth:sanctum");
        Ok(())
    })?;
    Ok(())
}

#[cfg(feature = "sanctum")]
fn user(request: &mut openrust::http::Request)
    -> openrust::http::HttpFuture<'_,Result<openrust::http::Response,openrust::http::HttpError>> {
    use openrust::http::{ApiResult, Response, StatusCode};

    Box::pin(async move {
        #[cfg(feature = "fortify")]
        {
            let user=openrust_fortify::starter::UserResource::current(request).await?;
            Response::json(StatusCode::OK, ApiResult::success(user))
        }
        #[cfg(not(feature = "fortify"))]
        {
            use openrust::{http::HttpError, support::facades::Auth};
            let manager = Auth::manager()?;
            let user = manager
                .user(request)
                .await?
                .ok_or(HttpError::Unauthenticated)?;
            Response::json(
                StatusCode::OK,
                ApiResult::success(manager.user_attributes(user.as_ref())?),
            )
        }
    })
}
