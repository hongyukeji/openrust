use std::sync::Arc;

use chrono::{TimeDelta, Utc};
use serde_json::Value;

use crate::{
    auth::{AuthError, AuthIdentifier, AuthManager, AuthResult},
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
pub(crate) struct Guard {
    name: String,
    session_guards: Vec<String>,
    provider_model: Option<String>,
    expiration: Option<TimeDelta>,
    track_last_used_at: bool,
}

impl Guard {
    pub(crate) fn new(name: String, config: &dyn Repository, guard: &Value) -> AuthResult<Self> {
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

    pub(crate) async fn resolve(
        &self,
        container: &Container,
        request: &mut Request,
    ) -> AuthResult<Option<Arc<dyn Authenticatable>>> {
        let auth = container
            .make::<AuthManager>()
            .map_err(AuthError::backend)?;
        if request.hasSession() {
            for name in &self.session_guards {
                if let Some(user) = auth.userForGuard(request, Some(name)).await? {
                    RequestTokens::insert(request, &self.name, AccessToken::Transient);
                    return Ok(Some(user));
                }
            }
        }
        let sanctum = container.make::<Sanctum>().map_err(AuthError::backend)?;
        let Some(credential) = sanctum.credential(request) else {
            return Ok(None);
        };
        let database = container
            .make::<DatabaseManager>()
            .map_err(AuthError::backend)?;
        let tokens = sanctum.personalAccessTokens(&database, "")?;
        let Some(token) = tokens.findToken(&credential).await? else {
            return Ok(None);
        };
        let id = AuthIdentifier::UnsignedInteger(token.tokenable_id);
        let morphs = container
            .make::<crate::database::eloquent::relations::MorphMap>()
            .map_err(AuthError::backend)?;
        let model = morphs
            .getMorphedModel(&token.tokenable_type)
            .unwrap_or(&token.tokenable_type);
        let user = auth.retrieve_sanctum_owner(model, &id).await?;
        let valid_provider = self.provider_model.as_ref().is_none_or(|configured| {
            user.is_some() && auth.same_registered_model(configured, model)
        });
        if !sanctum.valid(
            &token,
            valid_provider && !token.isExpired(Utc::now(), self.expiration),
        ) {
            return Ok(None);
        }
        let Some(user) = user else {
            return Ok(None);
        };
        let token = Arc::new(token);
        container
            .make::<Dispatcher>()
            .map_err(AuthError::backend)?
            .dispatch(
                Arc::new(TokenAuthenticated {
                    access_token: token.clone(),
                }),
                &mut EventContext::new(),
            )
            .await
            .map_err(AuthError::backend)?;
        let token = if self.track_last_used_at {
            let mut token = Arc::unwrap_or_clone(token);
            if !tokens.record_use(&mut token, &credential).await? {
                return Ok(None);
            }
            Arc::new(token)
        } else {
            token
        };
        RequestTokens::insert(request, &self.name, AccessToken::Personal(token));
        Ok(Some(Arc::from(user)))
    }
}

pub(super) fn expiration(config: &dyn Repository) -> AuthResult<Option<TimeDelta>> {
    Ok(match config.get("sanctum.expiration") {
        None | Some(Value::Null) => None,
        Some(value) => match value.as_i64() {
            Some(0) => None,
            Some(minutes) if minutes > 0 => Some(
                TimeDelta::try_minutes(minutes)
                    .ok_or(AuthError::Configuration("sanctum.expiration"))?,
            ),
            _ => return Err(AuthError::Configuration("sanctum.expiration")),
        },
    })
}

pub(super) fn session_guards(config: &dyn Repository) -> AuthResult<Vec<String>> {
    let guards = match config.get("sanctum.guard") {
        None if config.get("auth.guards.web").is_some() => vec!["web".to_owned()],
        None => Vec::new(),
        Some(Value::String(name)) => vec![name],
        Some(Value::Array(names)) => names
            .into_iter()
            .map(|name| match name {
                Value::String(name) => Ok(name),
                _ => Err(AuthError::Configuration("sanctum.guard")),
            })
            .collect::<AuthResult<_>>()?,
        _ => return Err(AuthError::Configuration("sanctum.guard")),
    };
    for name in &guards {
        if config
            .get(&format!("auth.guards.{name}.driver"))
            .as_ref()
            .and_then(Value::as_str)
            != Some("session")
        {
            return Err(AuthError::Configuration("sanctum session guard"));
        }
    }
    Ok(guards)
}
