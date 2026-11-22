use super::controller::Controller;
use openrust::{
    foundation::Application,
    http::{
        HeaderMap, HttpError, HttpFuture, Request, Response, StatusCode,
        middleware::LocaleSelection,
    },
    support::facades::{App, Config},
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
                .with(
                    "locale_parameter",
                    selection.and_then(|value| value.query_parameter.clone()),
                )
                .with(
                    "locale_candidates",
                    selection
                        .map(|value| value.supported.clone())
                        .unwrap_or_default(),
                )
                .with(
                    "locale_from_query",
                    selection.is_some_and(|value| value.from_query),
                )
                .with(
                    "query",
                    request
                        .query(None)?
                        .unwrap_or_else(|| serde_json::json!({})),
                )
                .with("base_path", request.getBaseUrl());
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
}
