use super::controller::Controller;
use openrust::{
    http::{ApiResponse, HttpError, HttpFuture, Request, Response, StatusCode},
    support::facades::Config,
};
use serde_json::json;

pub(crate) struct ApiController;
impl Controller for ApiController {}
impl ApiController {
    pub(crate) fn index(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        Box::pin(async {
            Response::json(
                StatusCode::OK,
                ApiResponse::success(json!({
                    "name": Config::string("app.name")?,
                    "version": Config::string("app.version")?,
                })),
            )
        })
    }
}
