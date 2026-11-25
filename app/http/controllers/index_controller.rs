use super::controller::Controller;
use openrust::{
    http::{HeaderMap, HttpError, HttpFuture, Request, Response, StatusCode},
    routing::{RedirectOptions, UrlError},
    support::facades::{App, Config, Redirect},
    view::view,
};

pub(crate) struct IndexController;

impl Controller for IndexController {}

impl IndexController {
    pub(crate) fn index(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        Box::pin(async move {
            let mut target = Config::string_or("app.home_url", "");
            let path = target.split(['?', '#']).next().unwrap_or_default().trim();
            if path.is_empty() || path == "/" {
                return Self::welcome(request).await;
            }
            if let Some(query) = request.uri().query().filter(|query| !query.is_empty()) {
                let end = target.find('#').unwrap_or(target.len());
                let separator = match target[..end].split_once('?') {
                    None => "?",
                    Some((_, existing)) if existing.is_empty() || existing.ends_with('&') => "",
                    Some(_) => "&",
                };
                target.insert_str(end, &format!("{separator}{query}"));
            }
            match Redirect::to(&target, RedirectOptions::default()) {
                Ok(redirect) => {
                    let response: Response = redirect.into();
                    let current_url = request.url()?;
                    let same_page = response.headers()["location"]
                        .to_str()
                        .ok()
                        .and_then(|location| location.split(['?', '#']).next())
                        .is_some_and(|location| location.trim_end_matches('/') == current_url);
                    if same_page {
                        return Self::welcome(request).await;
                    }
                    Ok(response)
                }
                Err(
                    UrlError::InvalidParameter
                    | UrlError::InvalidOrigin
                    | UrlError::InvalidRedirect,
                ) => Self::welcome(request).await,
                Err(error) => Err(error.into()),
            }
        })
    }

    async fn welcome(request: &mut Request) -> Result<Response, HttpError> {
        let locale = App::current_locale()?;
        let page = view("welcome");
        let mut headers = HeaderMap::new();
        headers.insert("content-language", locale.parse()?);
        Response::view(StatusCode::OK, page, headers, request).await
    }
}
