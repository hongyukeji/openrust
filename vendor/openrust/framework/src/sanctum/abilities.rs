use crate::{
    auth::Auth,
    contracts::http::Middleware,
    http::{HttpError, HttpFuture, Next, Request, Response},
};

use super::AccessToken;

pub struct CheckAbilities(Vec<String>);
pub struct CheckForAnyAbility(Vec<String>);

impl CheckAbilities {
    pub fn new(abilities: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self(abilities.into_iter().map(Into::into).collect())
    }
}

impl CheckForAnyAbility {
    pub fn new(abilities: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self(abilities.into_iter().map(Into::into).collect())
    }
}

async fn current_token(request: &mut Request) -> Result<&AccessToken, HttpError> {
    if Auth::user(request).await?.is_none() {
        return Err(HttpError::Unauthenticated);
    }
    AccessToken::currentAccessToken(request).ok_or(HttpError::Unauthenticated)
}

impl Middleware for CheckAbilities {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
        next: Next<'a>,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            let token = current_token(request).await?;
            if let Some(ability) = self.0.iter().find(|ability| token.cant(ability)) {
                return Err(HttpError::AuthorizationDenied(format!(
                    "Missing ability: {ability}"
                )));
            }
            Ok(next.run(request).await)
        })
    }
}

impl Middleware for CheckForAnyAbility {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
        next: Next<'a>,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            let token = current_token(request).await?;
            if !self.0.iter().any(|ability| token.can(ability)) {
                return Err(HttpError::AuthorizationDenied(
                    "Missing required ability.".into(),
                ));
            }
            Ok(next.run(request).await)
        })
    }
}
