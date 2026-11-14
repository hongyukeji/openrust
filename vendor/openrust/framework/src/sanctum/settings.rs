use std::sync::{Arc, RwLock};

use zeroize::Zeroizing;

use crate::{
    auth::{AuthError, AuthResult},
    contracts::config::Repository,
    database::DatabaseManager,
    http::Request,
};

use super::{PersonalAccessToken, PersonalAccessTokenModel, PersonalAccessTokens};

type Retrieval = dyn Fn(&Request) -> Option<String> + Send + Sync;
type Authentication = dyn Fn(&PersonalAccessToken, bool) -> bool + Send + Sync;
type TokenModel = fn(&DatabaseManager, String) -> AuthResult<PersonalAccessTokens>;
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
        user: Arc<U>,
        abilities: &[&str],
        guard: Option<&str>,
    ) -> Arc<U>
    where
        U: crate::contracts::auth::Authenticatable + 'static,
    {
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

    pub fn usePersonalAccessTokenModel<M: PersonalAccessTokenModel>(&self) {
        *self.model.write().expect("Sanctum token model lock") = Some(|database, prefix| {
            let model = M::from_state(Default::default());
            let connection = database
                .connection(model.getConnectionName())
                .map_err(AuthError::backend)?;
            Ok(PersonalAccessTokens::for_model::<M>(connection, prefix))
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
        if prefix.is_empty()
            && let Some(configured) = self
                .config
                .as_ref()
                .and_then(|config| config.get("sanctum.token_prefix"))
        {
            prefix = configured
                .as_str()
                .ok_or(AuthError::Configuration("sanctum.token_prefix"))?
                .to_owned();
        }
        match model {
            Some(model) => model(database, prefix),
            None => Ok(PersonalAccessTokens::new(
                database.connection(None).map_err(AuthError::backend)?,
                prefix,
            )),
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

    pub(super) fn credential(&self, request: &Request) -> Option<Zeroizing<String>> {
        let callback = self
            .retrieval
            .read()
            .expect("Sanctum retrieval lock")
            .clone();
        if let Some(callback) = callback {
            return callback(request)
                .map(Zeroizing::new)
                .filter(|token| !token.is_empty());
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
            .map(|(_, token)| Zeroizing::new(token.to_owned()))
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
