use std::sync::Arc;

use crate::{contracts::auth::UserProvider, http::Request};

use super::{RequestGuard, request_guard::Resolver};

/// Stateless authentication through a provider's configured Bearer token column.
pub struct TokenGuard;

impl TokenGuard {
    pub(super) fn bind<'a>(
        name: String,
        request: &'a mut Request,
        provider: Arc<dyn UserProvider>,
        column: String,
        hash: bool,
    ) -> RequestGuard<'a> {
        let resolve: Arc<Resolver> = Arc::new(move |request| {
            let provider = provider.clone();
            let column = column.clone();
            Box::pin(async move {
                let Some(header) = request
                    .headers()
                    .get(::http::header::AUTHORIZATION)
                    .and_then(|value| value.to_str().ok())
                else {
                    return Ok(None);
                };
                let Some((scheme, token)) = header.split_once(' ') else {
                    return Ok(None);
                };
                if !scheme.eq_ignore_ascii_case("bearer")
                    || token.is_empty()
                    || token.len() > 4096
                    || token.bytes().any(|byte| byte.is_ascii_whitespace())
                {
                    return Ok(None);
                }
                let token = if hash {
                    aws_lc_rs::digest::digest(&aws_lc_rs::digest::SHA256, token.as_bytes())
                        .as_ref()
                        .iter()
                        .map(|byte| format!("{byte:02x}"))
                        .collect()
                } else {
                    token.to_owned()
                };
                provider.retrieve_by_api_token(&column, &token).await
            })
        });
        RequestGuard::new(name, request, resolve)
    }
}
