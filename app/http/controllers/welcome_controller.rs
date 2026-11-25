use super::controller::Controller;
use openrust::{
    http::{HeaderMap, HttpError, HttpFuture, Request, Response, StatusCode},
    support::facades::App,
    view::view,
};

pub(crate) struct WelcomeController;

impl Controller for WelcomeController {}

impl WelcomeController {
    pub(crate) fn index(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        Box::pin(async move {
            let locale = App::current_locale()?;
            let page = view("welcome");
            let mut headers = HeaderMap::new();
            headers.insert("content-language", locale.parse()?);
            Response::view(StatusCode::OK, page, headers, request).await
        })
    }
}
