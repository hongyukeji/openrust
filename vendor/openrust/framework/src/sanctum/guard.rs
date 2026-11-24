use std::sync::Arc;

use jiff::SignedDuration;
use serde_json::Value;

use crate::{
    auth::{AuthError, AuthManager, AuthResult},
    container::Container,
    contracts::{
        auth::Authenticatable, config::Repository, container::Container as _,
        events::Dispatcher as _,
    },
    database::DatabaseManager,
    events::{Dispatcher, EventContext},
    http::Request,
};

use super::{AccessToken, RequestTokens, Sanctum, TokenAuthenticated};

#[derive(Clone)]
pub struct Guard {
    name: String,
    session_guards: Vec<String>,
    provider_model: Option<String>,
    expiration: Option<SignedDuration>,
    track_last_used_at: bool,
}

impl Guard {
    pub fn new(name: String, config: &dyn Repository, guard: &Value) -> AuthResult<Self> {
        let session_guards = session_guards(config)?;
        let provider_model = guard
            .get("provider")
            .and_then(Value::as_str)
            .map(|name| {
                config
                    .get(&format!("auth.providers.{name}.model"))
                    .and_then(|value| value.as_str().map(str::to_owned))
                    .ok_or(AuthError::Configuration("sanctum token provider model"))
            })
            .transpose()?;
        let expiration = expiration(config)?;
        Ok(Self {
            name,
            session_guards,
            provider_model,
            expiration,
            track_last_used_at: config
                .get("sanctum.last_used_at")
                .and_then(|value| value.as_bool())
                .unwrap_or(true),
        })
    }

    pub async fn resolve(
        &self,
        container: &Container,
        request: &mut Request,
    ) -> AuthResult<Option<Arc<dyn Authenticatable>>> {
        let auth = container.make::<AuthManager>()?;
        for name in &self.session_guards {
            // Delegating to another Sanctum guard can recursively enter this same chain.
            if auth.guard_uses_driver(request, name, "sanctum")? {
                continue;
            }
            if let Some(user) = auth.user_for_guard(request, Some(name)).await? {
                if auth.supports_sanctum_tokens(user.as_ref()) {
                    RequestTokens::insert(request, &self.name, AccessToken::Transient);
                }
                return Ok(Some(user));
            }
        }
        let sanctum = container.make::<Sanctum>()?;
        let Some((credential, is_bearer)) = sanctum.credential(request) else {
            return Ok(None);
        };
        let database = container.make::<DatabaseManager>()?;
        let tokens = sanctum.personal_access_tokens(&database, "")?;
        if is_bearer && !tokens.accepts_bearer(&credential) {
            return Ok(None);
        }
        let Some((token, token_model)) = tokens.find_for_authentication(&credential).await? else {
            return Ok(None);
        };
        let morphs = container.make::<crate::database::eloquent::relations::MorphMap>()?;
        let model = morphs
            .get_morphed_model(&token.tokenable_type)
            .unwrap_or(&token.tokenable_type);
        let user = token.tokenable(&auth, &morphs).await?;
        let valid_provider = self.provider_model.as_ref().is_none_or(|configured| {
            user.is_some() && auth.same_registered_model(configured, model)
        });
        if !sanctum.valid(
            &token,
            valid_provider
                && !token.is_expired(
                    crate::support::utc_now_for(Some(container)),
                    self.expiration,
                ),
        ) {
            return Ok(None);
        }
        let Some(user) = user.filter(|user| auth.supports_sanctum_tokens(user.as_ref())) else {
            return Ok(None);
        };
        if !sanctum.valid_owner(&token, user.as_ref())? {
            return Ok(None);
        }
        let token = Arc::new(token);
        container
            .make::<Dispatcher>()?
            .dispatch(
                Arc::new(TokenAuthenticated {
                    token: token.clone(),
                }),
                &mut EventContext::new(),
            )
            .await?;
        let token = if self.track_last_used_at {
            let mut token = Arc::unwrap_or_clone(token);
            if !tokens.record_use(&mut token, token_model).await? {
                return Ok(None);
            }
            Arc::new(token)
        } else {
            token
        };
        RequestTokens::insert(request, &self.name, AccessToken::Personal(token));
        Ok(Some(user))
    }
}

pub(super) fn expiration(config: &dyn Repository) -> AuthResult<Option<SignedDuration>> {
    Ok(match config.get("sanctum.expiration") {
        None | Some(Value::Null) => None,
        Some(value) => match value.as_i64() {
            Some(0) => None,
            Some(minutes) if minutes > 0 => Some(
                minutes
                    .checked_mul(60)
                    .map(SignedDuration::from_secs)
                    .ok_or(AuthError::Configuration("sanctum.expiration"))?,
            ),
            _ => return Err(AuthError::Configuration("sanctum.expiration")),
        },
    })
}

pub(super) fn session_guards(config: &dyn Repository) -> AuthResult<Vec<String>> {
    Ok(match config.get("sanctum.guard") {
        None if config.get("auth.guards.web").is_some() => vec!["web".to_owned()],
        None | Some(Value::Null) => Vec::new(),
        Some(Value::String(name)) => vec![name],
        Some(Value::Array(names)) => names
            .into_iter()
            .map(|name| match name {
                Value::String(name) => Ok(name),
                _ => Err(AuthError::Configuration("sanctum.guard")),
            })
            .collect::<AuthResult<_>>()?,
        _ => return Err(AuthError::Configuration("sanctum.guard")),
    })
}
