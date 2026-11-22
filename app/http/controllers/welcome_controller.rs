use openrust::{
    http::{HeaderMap, HttpError, HttpFuture, Request, Response, StatusCode},
    support::facades::{App, Config, Lang},
    view::view,
};

pub(crate) struct WelcomeController;

impl WelcomeController {
    pub(crate) fn index(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        Box::pin(async move {
            let locale = App::currentLocale().map_err(|source| HttpError::Internal {
                source: Box::new(source),
            })?;
            let fallback = App::getFallbackLocale().map_err(|source| HttpError::Internal {
                source: Box::new(source),
            })?;
            // 页面主文案可回退语言，文档只提供三语；两者均不改变应用语言。
            let language =
                if Lang::hasForLocale("welcome.heading", Some(&locale)).map_err(|source| {
                    HttpError::Internal {
                        source: Box::new(source),
                    }
                })? {
                    &locale
                } else if Lang::hasForLocale("welcome.heading", Some(&fallback)).map_err(
                    |source| HttpError::Internal {
                        source: Box::new(source),
                    },
                )? {
                    &fallback
                } else {
                    &locale
                };
            let docs_locale = [language.as_str(), fallback.as_str(), "en"]
                .into_iter()
                .find(|locale| matches!(*locale, "en" | "zh-Hans" | "zh-Hant"))
                .expect("英文文档始终可用");
            let page = view("welcome")
                .with("name", Config::stringOr("app.name", "App"))
                .with("version", Config::stringOr("app.version", "1.0.0"))
                .with("language", language.to_owned())
                .with("docs_locale", docs_locale);
            let mut headers = HeaderMap::new();
            headers.insert(
                "content-language",
                language.parse().map_err(|source| HttpError::Internal {
                    source: Box::new(source),
                })?,
            );
            Response::view(StatusCode::OK, page, headers, request).await
        })
    }
}
