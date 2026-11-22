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
            let locale = App::currentLocale().map_err(|source| HttpError::Internal {
                source: Box::new(source),
            })?;
            let page = view("welcome");
            let mut headers = HeaderMap::new();
            headers.insert(
                "content-language",
                locale.parse().map_err(|source| HttpError::Internal {
                    source: Box::new(source),
                })?,
            );
            Response::view(StatusCode::OK, page, headers, request).await
        })
    }
    pub(crate) fn store_locale(
        request: &mut Request,
    ) -> HttpFuture<'_, Result<Response, HttpError>> {
        Box::pin(async move {
            let redirect = match request.post(Some("redirect")).await? {
                None => "/".to_owned(),
                Some(serde_json::Value::String(path)) => path,
                _ => return Err(HttpError::InvalidRequest),
            };
            let target = SetLocale::urlWithoutLocale(request, &redirect)?;
            SetLocale::storePreferenceFromRequest(
                request,
                &Config::string("app.locale_form_field").map_err(|source| HttpError::Internal {
                    source: Box::new(source),
                })?,
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
