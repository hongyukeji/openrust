//! Personal access tokens for Laravel-style API authentication.

mod personal_access_token;

pub use personal_access_token::{NewAccessToken, PersonalAccessToken, PersonalAccessTokens};

mod abilities;
mod authenticate_session;
mod ensure_frontend_requests_are_stateful;
mod guard;
pub use abilities::{CheckAbilities, CheckForAnyAbility};
pub use authenticate_session::AuthenticateSession;
pub(crate) use ensure_frontend_requests_are_stateful::{
    EnsureFrontendRequestsAreStateful, StatefulFrontend,
};
pub(crate) use guard::Guard;

use crate::{contracts::events::Event, http::Request};
use std::{collections::HashMap, sync::Arc};

#[derive(Clone, Default)]
pub(crate) struct RequestTokens(HashMap<String, AccessToken>);

impl RequestTokens {
    pub(crate) fn insert(request: &mut Request, guard: &str, token: AccessToken) {
        request
            .parts_mut()
            .extensions
            .get_or_insert_default::<Self>()
            .0
            .insert(guard.to_owned(), token);
    }
}

/// The credential associated with the current Sanctum identity.
#[derive(Clone)]
pub enum AccessToken {
    Personal(Arc<PersonalAccessToken>),
    Transient,
}

#[allow(non_snake_case, reason = "Matches Laravel's Sanctum token API")]
impl AccessToken {
    pub fn currentAccessToken(request: &Request) -> Option<&Self> {
        let name = crate::auth::AuthManager::selected_guard(request);
        let tokens = request.parts().extensions.get::<RequestTokens>()?;
        if let Some(name) = name {
            return tokens.0.get(name);
        }
        let default = crate::auth::Auth::manager().ok()?.getDefaultDriver();
        tokens.0.get(&default)
    }

    pub fn can(&self, ability: &str) -> bool {
        match self {
            Self::Personal(token) => token.can(ability),
            Self::Transient => true,
        }
    }

    pub fn cant(&self, ability: &str) -> bool {
        !self.can(ability)
    }
}

/// Dispatched when a personal access token resolves to its authenticated owner.
pub struct TokenAuthenticated {
    pub access_token: Arc<PersonalAccessToken>,
}

impl Event for TokenAuthenticated {
    fn name(&self) -> &str {
        "TokenAuthenticated"
    }
}

pub(crate) fn register_routes(
    app: &crate::foundation::Application,
    router: &mut crate::routing::Router,
) -> Result<(), crate::routing::RoutingError> {
    use crate::contracts::foundation::Application as _;
    let config = app
        .config()
        .map_err(|source| crate::routing::RoutingError::ContainerFailed { source })?;
    if config
        .get("sanctum.routes")
        .and_then(|value| value.as_bool())
        == Some(false)
    {
        return Ok(());
    }
    let prefix = config.get("sanctum.prefix");
    let prefix = prefix
        .as_ref()
        .and_then(serde_json::Value::as_str)
        .unwrap_or("sanctum");
    router.group(
        crate::routing::RouteGroup::new()
            .prefix(prefix)
            .middleware_name("web"),
        |router| {
            router
                .get("/csrf-cookie", csrf_cookie)?
                .name("sanctum.csrf-cookie");
            Ok(())
        },
    )
}

fn csrf_cookie(
    _: &mut Request,
) -> crate::http::HttpFuture<'_, Result<crate::http::Response, crate::http::HttpError>> {
    Box::pin(async {
        Ok(crate::http::Response::empty(
            crate::http::StatusCode::NO_CONTENT,
        ))
    })
}
