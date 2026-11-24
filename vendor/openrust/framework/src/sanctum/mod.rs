//! Personal access tokens for framework API authentication.

mod personal_access_token;

pub use personal_access_token::{
    NewAccessToken, PersonalAccessToken, PersonalAccessTokens, TokenOptions,
};

mod abilities;
mod authenticate_session;
mod ensure_frontend_requests_are_stateful;
mod guard;
mod has_api_tokens;
pub(crate) mod prune_expired;
mod settings;
mod token_model;
mod token_store;
pub use abilities::{
    CheckAbilities, CheckForAnyAbility, CheckForAnyScope, CheckScopes, MissingAbilityException,
    MissingScopeException,
};
pub use authenticate_session::AuthenticateSession;
pub use ensure_frontend_requests_are_stateful::EnsureFrontendRequestsAreStateful;
pub(crate) use ensure_frontend_requests_are_stateful::StatefulFrontend;
pub use guard::Guard;
pub use has_api_tokens::{HasApiTokens, TokenRelation};
pub use settings::Sanctum;
pub(crate) use settings::with_default_configuration;
pub use token_model::{DefaultPersonalAccessToken, PersonalAccessTokenModel};

pub mod contracts {
    pub use super::{HasAbilities, HasApiTokens};
}

pub mod events {
    pub use super::TokenAuthenticated;
}

pub mod exceptions {
    pub use super::{MissingAbilityException, MissingScopeException};
}

pub mod http {
    pub mod controllers {
        use crate::http::{HttpError, HttpFuture, Request, Response, StatusCode};

        pub struct CsrfCookieController;

        impl CsrfCookieController {
            pub fn show(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
                Box::pin(async { Ok(Response::empty(StatusCode::NO_CONTENT)) })
            }
        }
    }

    pub mod middleware {
        pub use super::super::{
            AuthenticateSession, CheckAbilities, CheckForAnyAbility, CheckForAnyScope, CheckScopes,
            EnsureFrontendRequestsAreStateful,
        };
    }
}

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

#[derive(Clone)]
pub(crate) struct TestIdentity {
    pub(crate) guard: String,
    pub(crate) user: Arc<dyn crate::contracts::auth::Authenticatable>,
    pub(crate) abilities: Arc<[String]>,
}

pub trait HasAbilities {
    fn can(&self, ability: &str) -> bool;

    fn cant(&self, ability: &str) -> bool {
        !self.can(ability)
    }
}

pub struct TransientToken;

impl HasAbilities for TransientToken {
    fn can(&self, _: &str) -> bool {
        true
    }
}

impl HasAbilities for PersonalAccessToken {
    fn can(&self, ability: &str) -> bool {
        self.can(ability)
    }
}

impl HasAbilities for AccessToken {
    fn can(&self, ability: &str) -> bool {
        self.can(ability)
    }
}

/// The credential associated with the current Sanctum identity.
#[derive(Clone)]
pub enum AccessToken {
    Test(Arc<[String]>),
    Personal(Arc<PersonalAccessToken>),
    Transient,
}

impl AccessToken {
    pub fn current_access_token(request: &Request) -> Option<&Self> {
        let name = crate::auth::AuthManager::selected_guard(request);
        let tokens = request.parts().extensions.get::<RequestTokens>()?;
        if let Some(name) = name {
            return tokens.0.get(name);
        }
        let default = crate::auth::Auth::manager().ok()?.get_default_driver();
        tokens.0.get(&default)
    }

    pub fn can(&self, ability: &str) -> bool {
        match self {
            Self::Personal(token) => token.can(ability),
            Self::Transient => true,
            Self::Test(abilities) => abilities
                .iter()
                .any(|value| value == "*" || value == ability),
        }
    }

    pub fn cant(&self, ability: &str) -> bool {
        !self.can(ability)
    }
}

/// Dispatched when a personal access token resolves to its authenticated owner.
pub struct TokenAuthenticated {
    pub token: Arc<PersonalAccessToken>,
}

impl TokenAuthenticated {
    pub const NAME: &str = "OpenRust\\Sanctum\\Events\\TokenAuthenticated";
}

impl Event for TokenAuthenticated {
    fn name(&self) -> &str {
        Self::NAME
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
                .get(
                    "/csrf-cookie",
                    http::controllers::CsrfCookieController::show,
                )?
                .name("sanctum.csrf-cookie");
            Ok(())
        },
    )
}
