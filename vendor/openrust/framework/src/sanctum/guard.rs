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

use super::{AccessToken, PersonalAccessTokens, RequestTokens, TokenAuthenticated};

#[derive(Clone)]
pub(crate) struct Guard {
    name: String,
    session_guards: Vec<String>,
    provider: Option<(String, String)>,
    expiration: Option<TimeDelta>,
    track_last_used_at: bool,
}

impl Guard {
    pub(crate) fn new(name: String, config: &dyn Repository, guard: &Value) -> AuthResult<Self> {
        let session_guards = session_guards(config)?;
        let provider = guard
            .get("provider")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or_else(|| {
                config
                    .get("auth.guards.web.provider")
                    .and_then(|value| value.as_str().map(str::to_owned))
            });
        let provider = provider
            .map(|name| {
                let model = config
                    .get(&format!("auth.providers.{name}.model"))
                    .and_then(|value| value.as_str().map(str::to_owned))
                    .ok_or(AuthError::Configuration("sanctum token provider model"))?;
                Ok::<_, AuthError>((name, model))
            })
            .transpose()?;
        let expiration = match config.get("sanctum.expiration") {
            None | Some(Value::Null) => None,
            Some(value) => match value.as_i64() {
                Some(0) => None,
                Some(minutes) if minutes > 0 => Some(
                    TimeDelta::try_minutes(minutes)
                        .ok_or(AuthError::Configuration("sanctum.expiration"))?,
                ),
                _ => return Err(AuthError::Configuration("sanctum.expiration")),
            },
        };
        Ok(Self {
            name,
            session_guards,
            provider,
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
        let Some(credential) = request
            .headers()
            .get(::http::header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|header| header.split_once(' '))
            .filter(|(scheme, token)| {
                scheme.eq_ignore_ascii_case("bearer")
                    && !token.is_empty()
                    && token.len() <= 4096
                    && !token.bytes().any(|byte| byte.is_ascii_whitespace())
            })
            .map(|(_, token)| token)
        else {
            return Ok(None);
        };
        let Some((provider, model)) = &self.provider else {
            return Err(AuthError::Configuration("sanctum token provider"));
        };
        let database = container
            .make::<DatabaseManager>()
            .map_err(AuthError::backend)?;
        let tokens =
            PersonalAccessTokens::new(database.connection(None).map_err(AuthError::backend)?, "");
        let Some(token) = tokens.findToken(credential).await? else {
            return Ok(None);
        };
        if token.tokenable_type != *model || token.isExpired(Utc::now(), self.expiration) {
            return Ok(None);
        }
        let user = auth
            .createUserProvider(provider)?
            .retrieve_by_id(&AuthIdentifier::UnsignedInteger(token.tokenable_id))
            .await?;
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
            if !tokens.record_use(&mut token, credential).await? {
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
