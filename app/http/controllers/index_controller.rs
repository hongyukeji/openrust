use super::controller::Controller;
use openrust::{
    http::{HttpError, HttpFuture, Method, Request, Response},
    routing::RedirectOptions,
    support::facades::{Config, Facade, Redirect, Route},
};

pub(crate) struct IndexController;

impl Controller for IndexController {}

impl IndexController {
    pub(crate) fn index(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        Box::pin(async move {
            let mut target = Config::string_or("app.home_url", "");
            if target.trim().is_empty() {
                let host = request.get_host()?;
                let route = Route::get_facade_root()?
                    .get_routes()
                    .into_iter()
                    .find(|route| {
                        route.uri != "/"
                            && !route.uri.contains('{')
                            && route
                                .method
                                .split('|')
                                .any(|method| method == Method::GET.as_str())
                            && route
                                .middleware
                                .iter()
                                .any(|middleware| middleware == "web")
                            && route
                                .domain
                                .as_ref()
                                .is_none_or(|domain| domain.eq_ignore_ascii_case(&host))
                    })
                    .ok_or(HttpError::NotFound)?;
                target = format!("/{}", route.uri);
            }
            if let Some(query) = request.uri().query().filter(|query| !query.is_empty())
                && !target.starts_with('#')
            {
                let end = target.find('#').unwrap_or(target.len());
                let separator = match target[..end].split_once('?') {
                    None => "?",
                    Some((_, existing)) if existing.is_empty() || existing.ends_with('&') => "",
                    Some(_) => "&",
                };
                target.insert_str(end, &format!("{separator}{query}"));
            }
            Ok(Redirect::to(&target, RedirectOptions::default())?.into())
        })
    }
}
