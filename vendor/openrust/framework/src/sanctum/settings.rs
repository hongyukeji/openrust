use std::{
    any::type_name,
    sync::{Arc, RwLock},
};

use zeroize::Zeroizing;

use crate::{
    auth::{AuthError, AuthResult},
    contracts::config::Repository,
    database::{DatabaseManager, eloquent::Model},
    http::Request,
};

use super::{PersonalAccessToken, PersonalAccessTokenModel, PersonalAccessTokens};

type Retrieval = dyn Fn(&Request) -> Option<String> + Send + Sync;
type OwnerAuthentication = dyn Fn(&PersonalAccessToken, &dyn crate::contracts::auth::Authenticatable) -> AuthResult<bool>
    + Send
    + Sync;
type Authentication = dyn Fn(&PersonalAccessToken, bool) -> bool + Send + Sync;
#[derive(Clone, Copy)]
struct TokenModel {
    name: &'static str,
    store: fn(&DatabaseManager, String) -> AuthResult<PersonalAccessTokens>,
}

pub(super) fn application_host(config: &dyn Repository) -> Option<String> {
    let value = config.get("app.url")?;
    let value = value.as_str()?;
    let url = url::Url::parse(value).ok()?;
    let authority = value
        .split_once("://")?
        .1
        .split(['/', '?', '#'])
        .next()?
        .rsplit('@')
        .next()?;
    let authority = authority.parse::<::http::uri::Authority>().ok();
    let host = authority
        .as_ref()
        .map(::http::uri::Authority::host)
        .or_else(|| url.host_str())?;
    let port = authority
        .as_ref()
        .and_then(::http::uri::Authority::port_u16)
        .or_else(|| url.port());
    Some(match port {
        Some(port) => format!("{host}:{port}"),
        None => host.to_owned(),
    })
}
pub(super) const CURRENT_REQUEST_HOST: &str = "__SANCTUM_CURRENT_REQUEST_HOST__";

pub(crate) fn with_default_configuration(
    mut values: std::collections::BTreeMap<String, serde_json::Value>,
    env: &crate::config::Environment,
) -> Result<std::collections::BTreeMap<String, serde_json::Value>, crate::config::ConfigError> {
    use crate::config::ConfigError;
    use serde_json::{Value, json};

    let config = values.entry("sanctum".into()).or_insert_with(|| json!({}));
    let Some(config) = config.as_object_mut() else {
        return Ok(values);
    };
    if !config.contains_key("expiration") {
        let expiration = env
            .get_utf8("SANCTUM_EXPIRATION")?
            .filter(|value| !matches!(value.to_ascii_lowercase().as_str(), "null" | "(null)"))
            .map(|value| value.parse::<u64>())
            .transpose()
            .map_err(|_| ConfigError::InvalidType {
                key: "SANCTUM_EXPIRATION".into(),
                expected: "a non-negative integer",
            })?;
        config.insert("expiration".into(), json!(expiration));
    }
    if !config.contains_key("token_prefix") {
        let prefix = env.get_utf8("SANCTUM_TOKEN_PREFIX")?.unwrap_or_default();
        config.insert("token_prefix".into(), Value::from(prefix));
    }
    if !config.contains_key("stateful")
        && let Some(domains) = env.get_utf8("SANCTUM_STATEFUL_DOMAINS")?
    {
        config.insert("stateful".into(), Value::from(domains));
    }
    Ok(values)
}

/// Sanctum extensions owned by one application.
#[derive(Default)]
pub struct Sanctum {
    config: Option<Arc<dyn Repository>>,
    model: RwLock<Option<TokenModel>>,
    retrieval: RwLock<Option<Arc<Retrieval>>>,
    authentication: RwLock<Option<Arc<Authentication>>>,
    owner_authentication: RwLock<Option<Arc<OwnerAuthentication>>>,
}

impl Sanctum {
    pub(crate) fn configured(config: Arc<dyn Repository>) -> Self {
        Self {
            config: Some(config),
            ..Self::default()
        }
    }

    /// Authenticates subsequent requests made through this in-process test application.
    pub fn acting_as<U>(
        app: &mut crate::testing::TestApp,
        mut user: U,
        abilities: &[&str],
        guard: Option<&str>,
    ) -> Arc<U>
    where
        U: crate::contracts::auth::Authenticatable + Model,
    {
        user.state_mut().clear_recently_created();
        let user = Arc::new(user);
        app.sanctum_identity = Some(super::TestIdentity {
            guard: guard.unwrap_or("sanctum").to_owned(),
            user: user.clone(),
            abilities: abilities
                .iter()
                .map(|ability| (*ability).to_owned())
                .collect(),
        });
        user
    }

    /// Returns the suffix to append to a comma-separated list of stateful domains.
    pub fn current_request_host() -> &'static str {
        ",__SANCTUM_CURRENT_REQUEST_HOST__"
    }

    /// Returns the host and explicit port as a comma-prefixed stateful-domain suffix.
    pub fn current_application_url_with_port(&self) -> String {
        self.config
            .as_deref()
            .and_then(application_host)
            .map_or_else(String::new, |host| format!(",{host}"))
    }

    pub fn personal_access_token_model(&self) -> &'static str {
        self.model
            .read()
            .expect("Sanctum token model lock")
            .map_or(type_name::<super::DefaultPersonalAccessToken>(), |model| {
                model.name
            })
    }

    pub(super) fn require_token_model<M: PersonalAccessTokenModel>(&self) -> AuthResult<()> {
        if self.personal_access_token_model() == type_name::<M>() {
            Ok(())
        } else {
            Err(AuthError::Configuration("Sanctum token relation model"))
        }
    }

    pub fn use_personal_access_token_model<M: PersonalAccessTokenModel>(&self) {
        *self.model.write().expect("Sanctum token model lock") = Some(TokenModel {
            name: type_name::<M>(),
            store: |database, prefix| {
                let model = M::from_state(Default::default());
                let connection = database.connection(model.get_connection_name())?;
                Ok(PersonalAccessTokens::for_model::<M>(connection, prefix))
            },
        });
    }

    /// Resolves the token store for issuance, lookup, revocation and pruning.
    pub fn personal_access_tokens(
        &self,
        database: &DatabaseManager,
        prefix: impl Into<String>,
    ) -> AuthResult<PersonalAccessTokens> {
        let model = *self.model.read().expect("Sanctum token model lock");
        let mut prefix = prefix.into();
        if prefix.is_empty() {
            prefix = self.token_prefix()?;
        }
        match model {
            Some(model) => (model.store)(database, prefix),
            None => Ok(PersonalAccessTokens::new(
                database.connection(None)?,
                prefix,
            )),
        }
    }

    pub(super) fn token_prefix(&self) -> AuthResult<String> {
        match self
            .config
            .as_ref()
            .and_then(|config| config.get("sanctum.token_prefix"))
        {
            Some(value) => value
                .as_str()
                .map(str::to_owned)
                .ok_or(AuthError::Configuration("sanctum.token_prefix")),
            None => Ok(String::new()),
        }
    }

    pub fn get_access_token_from_request_using(&self, callback: Option<Arc<Retrieval>>) {
        *self.retrieval.write().expect("Sanctum retrieval lock") = callback;
    }

    pub fn authenticate_access_tokens_using(
        &self,
        callback: impl Fn(&PersonalAccessToken, bool) -> bool + Send + Sync + 'static,
    ) {
        *self
            .authentication
            .write()
            .expect("Sanctum authentication lock") = Some(Arc::new(callback));
    }

    /// 在标准有效期及 Provider 校验后，按最新归属身份追加应用级凭据检查。
    pub fn validate_token_owner_using(
        &self,
        callback: impl Fn(
            &PersonalAccessToken,
            &dyn crate::contracts::auth::Authenticatable,
        ) -> AuthResult<bool>
        + Send
        + Sync
        + 'static,
    ) {
        *self
            .owner_authentication
            .write()
            .expect("Sanctum owner authentication lock") = Some(Arc::new(callback));
    }

    pub(super) fn valid_owner(
        &self,
        token: &PersonalAccessToken,
        owner: &dyn crate::contracts::auth::Authenticatable,
    ) -> AuthResult<bool> {
        let callback = self
            .owner_authentication
            .read()
            .expect("Sanctum owner authentication lock")
            .clone();
        callback.map_or(Ok(true), |callback| callback(token, owner))
    }

    pub(super) fn credential(&self, request: &Request) -> Option<(Zeroizing<String>, bool)> {
        let callback = self
            .retrieval
            .read()
            .expect("Sanctum retrieval lock")
            .clone();
        if let Some(callback) = callback {
            return callback(request)
                .map(Zeroizing::new)
                .filter(|token| !token.is_empty())
                .map(|token| (token, false));
        }
        request
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
            .map(|(_, token)| (Zeroizing::new(token.to_owned()), true))
    }

    pub(super) fn valid(&self, token: &PersonalAccessToken, valid: bool) -> bool {
        let callback = self
            .authentication
            .read()
            .expect("Sanctum authentication lock")
            .clone();
        callback.map_or(valid, |callback| callback(token, valid))
    }
}
