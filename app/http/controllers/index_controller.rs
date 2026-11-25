use super::controller::Controller;
use openrust::{
    http::{HttpError, HttpFuture, Request, Response, StatusCode, middleware::SetLocale},
    routing::RedirectOptions,
    support::facades::{Config, Redirect},
};

pub(crate) struct IndexController;

impl Controller for IndexController {}

impl IndexController {
    pub(crate) fn index(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        Box::pin(async move {
            let mut target = Config::string_or("app.home_path", "/welcome");
            if !target.starts_with('/')
                || target.starts_with("//")
                || target == "/"
                || target.contains(['?', '#'])
            {
                return Err(HttpError::InvalidRequest);
            }
            if let Some(query) = request.uri().query() {
                target.push('?');
                target.push_str(query);
            }
            Ok(Redirect::to(&target, RedirectOptions::default())?.into())
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
                RedirectOptions {
                    status: StatusCode::SEE_OTHER,
                    ..Default::default()
                },
            )?
            .into())
        })
    }
}
