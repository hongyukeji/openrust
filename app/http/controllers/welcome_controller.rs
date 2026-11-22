use super::controller::Controller;
use openrust::{
    foundation::Application,
    http::{
        HeaderMap, HttpError, HttpFuture, Request, Response, StatusCode,
        middleware::{LocaleSelection, SetLocale},
    },
    support::{
        Arr,
        facades::{App, Config, Redirect},
    },
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
                    "locale_can_store",
                    selection.is_some_and(|value| value.can_store),
                )
                .with(
                    "locale_preference",
                    selection.and_then(|value| value.preference.clone()),
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
    pub(crate) fn store_locale(
        request: &mut Request,
    ) -> HttpFuture<'_, Result<Response, HttpError>> {
        Box::pin(async move {
            // 仅请求体表达写入意图；缺字段或错误类型不能意外清除偏好。
            let body = request.post(None).await?.ok_or(HttpError::InvalidRequest)?;
            let value = body.get("locale").ok_or(HttpError::InvalidRequest)?;
            let locale = match &value {
                serde_json::Value::Null => None,
                serde_json::Value::String(locale) if locale.is_empty() => None,
                serde_json::Value::String(locale) => Some(locale.as_str()),
                _ => return Err(HttpError::InvalidRequest),
            };
            SetLocale::storePreference(request, locale)?;
            let mut query = request
                .query(None)?
                .unwrap_or_else(|| serde_json::json!({}));
            if let Some(parameter) = request
                .extensions()
                .get::<LocaleSelection>()
                .and_then(|selection| selection.query_parameter.as_ref())
                && let Some(query) = query.as_object_mut()
            {
                query.remove(parameter);
            }
            let query = Arr::query(&query)?;
            let target = format!(
                "{}/{}{}",
                request.getBaseUrl(),
                if query.is_empty() { "" } else { "?" },
                query
            );
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
