use super::controller::Controller;
use openrust::{
    config::ConfigError,
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
            let value =
                Config::getOr("app.locale_form_field", "locale".into()).map_err(|source| {
                    HttpError::Internal {
                        source: Box::new(source),
                    }
                })?;
            let field = match value {
                serde_json::Value::String(field)
                    if !field.is_empty()
                        && !matches!(field.as_str(), "redirect" | "_token" | "_method")
                        && !field.chars().any(|c| {
                            c.is_whitespace() || c.is_control() || matches!(c, '.' | '[' | ']')
                        }) =>
                {
                    field
                }
                _ => {
                    return Err(HttpError::Internal {
                        source: Box::new(ConfigError::InvalidType {
                            key: "app.locale_form_field".into(),
                            expected: "不与 redirect、_token、_method 冲突的非空顶层字段名（不含空白、控制字符、点或方括号）",
                        }),
                    });
                }
            };
            let redirect = match request.post(Some("redirect")).await? {
                None => "/".to_owned(),
                Some(serde_json::Value::String(path)) => path,
                _ => return Err(HttpError::InvalidRequest),
            };
            let target = SetLocale::urlWithoutLocale(request, &redirect)?;
            SetLocale::storePreferenceFromRequest(request, &field).await?;
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
