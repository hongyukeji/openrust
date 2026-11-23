use super::controller::Controller;
use openrust::{
    http::{
        HeaderMap, HttpError, HttpFuture, Request, Response, StatusCode, middleware::SetLocale,
    },
    support::facades::{App, Config, Redirect},
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
    pub(crate) fn store_locale(
        request: &mut Request,
    ) -> HttpFuture<'_, Result<Response, HttpError>> {
        Box::pin(async move {
            let redirect = request.post_or("redirect", "/").await?;
            let redirect = redirect.as_str().ok_or(HttpError::InvalidRequest)?;
            let target = SetLocale::url_without_locale(request, redirect)?;
            SetLocale::store_preference_from_request(
                request,
                &Config::string("app.locale_form_field")?,
            )
            .await?;
            Ok(Redirect::to(
                &target,
                openrust::routing::RedirectOptions {
                    status: StatusCode::SEE_OTHER,
                    ..Default::default()
                },
            )?
            .into())
        })
    }
}
