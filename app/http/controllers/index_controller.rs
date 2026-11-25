use super::controller::Controller;
use openrust::{
    http::{HttpError, HttpFuture, Request, Response},
    routing::RedirectOptions,
    support::facades::{Config, Redirect},
};

pub(crate) struct IndexController;

impl Controller for IndexController {}

impl IndexController {
    pub(crate) fn index(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        Box::pin(async move {
            let mut target = Config::string_or("app.home_url", "");
            if target.trim().is_empty() {
                target = "/index.html".into();
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
