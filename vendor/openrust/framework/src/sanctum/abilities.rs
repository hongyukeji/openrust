use crate::{
    auth::Auth,
    contracts::http::Middleware,
    http::{HttpError, HttpFuture, Next, Request, Response},
};

use super::AccessToken;

#[derive(Debug)]
pub struct MissingAbilityException(Vec<String>);

impl std::fmt::Display for MissingAbilityException {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(Self::MESSAGE)
    }
}

impl std::error::Error for MissingAbilityException {}

impl MissingAbilityException {
    pub(crate) const MESSAGE: &str = "Invalid ability provided.";

    pub fn new(values: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self(values.into_iter().map(Into::into).collect())
    }

    pub fn abilities(&self) -> &[String] {
        &self.0
    }
}

#[derive(Debug)]
pub struct MissingScopeException(Vec<String>);

impl std::fmt::Display for MissingScopeException {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(Self::MESSAGE)
    }
}

impl std::error::Error for MissingScopeException {}

impl MissingScopeException {
    pub(crate) const MESSAGE: &str = "Invalid scope(s) provided.";

    pub fn new(values: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self(values.into_iter().map(Into::into).collect())
    }

    pub fn scopes(&self) -> &[String] {
        &self.0
    }
}

struct AbilityCheck {
    values: Vec<String>,
    any: bool,
    scopes: bool,
}

impl Middleware for AbilityCheck {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
        next: Next<'a>,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            if Auth::user(request).await?.is_none() {
                return Err(HttpError::Unauthenticated);
            }
            let token =
                AccessToken::currentAccessToken(request).ok_or(HttpError::Unauthenticated)?;
            let missing = if self.any {
                (!self.values.iter().any(|ability| token.can(ability))).then(|| self.values.clone())
            } else {
                self.values
                    .iter()
                    .find(|ability| token.cant(ability))
                    .map(|ability| vec![ability.clone()])
            };
            match missing {
                None => Ok(next.run(request).await),
                Some(values) if self.scopes => {
                    Err(HttpError::MissingScope(MissingScopeException(values)))
                }
                Some(values) => Err(HttpError::MissingAbility(MissingAbilityException(values))),
            }
        })
    }
}

macro_rules! middleware {
    ($name:ident, $any:expr, $scopes:expr) => {
        pub struct $name(AbilityCheck);

        impl $name {
            pub fn new(abilities: impl IntoIterator<Item = impl Into<String>>) -> Self {
                Self(AbilityCheck {
                    values: abilities.into_iter().map(Into::into).collect(),
                    any: $any,
                    scopes: $scopes,
                })
            }
        }

        impl Middleware for $name {
            fn handle<'a>(
                &'a self,
                request: &'a mut Request,
                next: Next<'a>,
            ) -> HttpFuture<'a, Result<Response, HttpError>> {
                self.0.handle(request, next)
            }
        }
    };
}

middleware!(CheckAbilities, false, false);
middleware!(CheckForAnyAbility, true, false);
middleware!(CheckScopes, false, true);
middleware!(CheckForAnyScope, true, true);
