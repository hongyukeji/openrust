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

/// Sanctum extensions owned by one application.
#[derive(Default)]
pub struct Sanctum {
    config: Option<Arc<dyn Repository>>,
    model: RwLock<Option<TokenModel>>,
    retrieval: RwLock<Option<Arc<Retrieval>>>,
    authentication: RwLock<Option<Arc<Authentication>>>,
}

#[allow(non_snake_case, reason = "Matches Laravel's Sanctum API")]
impl Sanctum {
    pub(crate) fn configured(config: Arc<dyn Repository>) -> Self {
        Self {
            config: Some(config),
            ..Self::default()
        }
    }

    /// Authenticates subsequent requests made through this in-process test application.
    pub fn actingAs<U>(
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
    pub fn currentRequestHost() -> &'static str {
        ",__SANCTUM_CURRENT_REQUEST_HOST__"
    }

    /// Returns the host and explicit port as a comma-prefixed stateful-domain suffix.
    pub fn currentApplicationUrlWithPort(&self) -> String {
        self.config
            .as_deref()
            .and_then(application_host)
            .map_or_else(String::new, |host| format!(",{host}"))
    }

    pub fn personalAccessTokenModel(&self) -> &'static str {
        self.model
            .read()
            .expect("Sanctum token model lock")
            .map_or(type_name::<super::DefaultPersonalAccessToken>(), |model| {
                model.name
            })
    }

    pub(super) fn require_token_model<M: PersonalAccessTokenModel>(&self) -> AuthResult<()> {
        if self.personalAccessTokenModel() == type_name::<M>() {
            Ok(())
        } else {
            Err(AuthError::Configuration("Sanctum token relation model"))
        }
    }

    pub fn usePersonalAccessTokenModel<M: PersonalAccessTokenModel>(&self) {
        *self.model.write().expect("Sanctum token model lock") = Some(TokenModel {
            name: type_name::<M>(),
            store: |database, prefix| {
                let model = M::from_state(Default::default());
                let connection = database
                    .connection(model.getConnectionName())
                    .map_err(AuthError::backend)?;
                Ok(PersonalAccessTokens::for_model::<M>(connection, prefix))
            },
        });
    }

    /// Resolves the token store for issuance, lookup, revocation and pruning.
    pub fn personalAccessTokens(
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
                database.connection(None).map_err(AuthError::backend)?,
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

    pub fn getAccessTokenFromRequestUsing(&self, callback: Option<Arc<Retrieval>>) {
        *self.retrieval.write().expect("Sanctum retrieval lock") = callback;
    }

    pub fn authenticateAccessTokensUsing(
        &self,
        callback: impl Fn(&PersonalAccessToken, bool) -> bool + Send + Sync + 'static,
    ) {
        *self
            .authentication
            .write()
            .expect("Sanctum authentication lock") = Some(Arc::new(callback));
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
