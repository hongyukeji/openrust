use super::controller::Controller;
use openrust::{
    http::{HttpError, HttpFuture, Request, Response, StatusCode, middleware::SetLocale},
    routing::RedirectOptions,
    support::facades::{Config, Redirect},
};

pub(crate) struct LocaleController;

impl Controller for LocaleController {}

impl LocaleController {
    pub(crate) fn store(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
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
                RedirectOptions {
                    status: StatusCode::SEE_OTHER,
                    ..Default::default()
                },
            )?
            .into())
        })
    }
}
