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
            let mut target = Config::string_or("app.home_path", "");
            if target.trim().is_empty() {
                target = "/index.html".into();
            }
            if !target.starts_with('/')
                || target.starts_with("//")
                || target == "/"
                || target.contains(['?', '#'])
            {
                return Err(HttpError::InvalidRequest);
            }
            if let Some(query) = request.uri().query() {
                target.push('?');
                target.push_str(query);
            }
            Ok(Redirect::to(&target, RedirectOptions::default())?.into())
        })
    }
}
