use super::controller::Controller;
use openrust::{
    foundation::Application,
    http::{
        HeaderMap, HttpError, HttpFuture, Request, Response, StatusCode,
        middleware::LocaleSelection,
    },
    support::{
        Arr,
        facades::{App, Config},
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
            let page = view("welcome")
                .with("name", Config::stringOr("app.name", "App"))
                .with("version", Config::stringOr("app.version", "1.0.0"))
                .with("repository", Application::REPOSITORY)
                .with("language", locale.clone())
                .with("language_links", language_links(request, &locale)?);
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

fn language_links(request: &Request, locale: &str) -> Result<serde_json::Value, HttpError> {
    let Some(selection) = request.extensions().get::<LocaleSelection>() else {
        return Ok(serde_json::json!([]));
    };
    let Some(parameter) = selection.query_parameter.as_deref() else {
        return Ok(serde_json::json!([]));
    };
    let mut links = Vec::new();
    let mut query = request
        .query(None)?
        .unwrap_or_else(|| serde_json::json!({}));
    // 复用框架查询集合及 RFC 3986 编码，保留其他参数；不重新选择语言。
    Arr::forget(&mut query, parameter).map_err(|_| HttpError::InvalidRequest)?;
    let path = format!("{}/", request.getBaseUrl());
    for (language, label) in [
        ("en", "english"),
        ("zh-Hans", "simplified"),
        ("zh-Hant", "traditional"),
    ] {
        if selection
            .supported
            .iter()
            .any(|supported| supported == language)
        {
            let mut values = query.clone();
            Arr::set(&mut values, parameter, language.into())
                .map_err(|_| HttpError::InvalidRequest)?;
            links.push(serde_json::json!({
                "label": format!("welcome.{label}"),
                "href": format!("{path}?{}", Arr::query(&values)?),
                "selected": selection.from_query && locale == language,
                "current": locale == language,
            }));
        }
    }
    if !links.is_empty() {
        let encoded = Arr::query(&query)?;
        links.push(serde_json::json!({
            "label": "welcome.follow_browser",
            "href": if encoded.is_empty() { path } else { format!("{path}?{encoded}") },
            "selected": !selection.from_query,
            "current": false,
        }));
    }
    Ok(links.into())
}
