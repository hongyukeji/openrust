use super::controller::Controller;
use openrust::{
    foundation::Application,
    http::{
        HeaderMap, HttpError, HttpFuture, Request, Response, StatusCode,
        middleware::{LocaleSelection, SetLocale},
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
            let selection = request.extensions().get::<LocaleSelection>();
            let page = view("welcome")
                .with("name", Config::stringOr("app.name", "App"))
                .with("version", Config::stringOr("app.version", "1.0.0"))
                .with("repository", Application::REPOSITORY)
                .with("language", locale.clone())
                .with("locale_selection", serde_json::json!(selection))
                .with(
                    "query",
                    request
                        .query(None)?
                        .unwrap_or_else(|| serde_json::json!({})),
                )
                .with("base_path", request.getBaseUrl())
                .with("path", request.uri().path());
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
            SetLocale::storePreferenceFromRequest(request, "locale").await?;
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
