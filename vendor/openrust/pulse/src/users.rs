use crate::{Error, PulseFuture, Result};
use openrust::{
    contracts::auth::Authenticatable,
    support::facades::{Auth, Config, Facade},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    sync::{Arc, RwLock},
};
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct User {
    pub name: String,
    pub extra: Option<String>,
    pub avatar: Option<String>,
}
/// Resolves metric identifiers and dashboard user details in the active application.
pub trait ResolvesUsers: Send + Sync {
    fn key(&self, user: &dyn Authenticatable) -> Option<String> {
        Some(user.get_auth_identifier().to_string())
    }
    fn setFieldResolver(&self, _resolver: Arc<FieldResolver>) -> Result<()> {
        Err(Error::InvalidConfiguration(
            "The configured user resolver does not support setting user fields".into(),
        ))
    }
    fn load<'a>(&'a self, keys: &'a [String]) -> PulseFuture<'a, ()>;
    fn find(&self, key: &str) -> User;
}
pub type FieldResolver = dyn Fn(&dyn Authenticatable) -> User + Send + Sync;
#[derive(Default)]
pub struct Users {
    resolved: RwLock<BTreeMap<String, User>>,
    pub(crate) fields: Arc<RwLock<Option<Arc<FieldResolver>>>>,
}
impl Users {
    pub fn setFieldResolver(
        &self,
        resolver: impl Fn(&dyn Authenticatable) -> User + Send + Sync + 'static,
    ) -> &Self {
        *self.fields.write().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(resolver));
        self
    }
    fn fields(&self, user: &dyn Authenticatable) -> Result<User> {
        if let Some(callback) = self
            .fields
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
        {
            return Ok(callback(user));
        }
        let text = |key| -> Result<Option<String>> {
            Ok(user
                .getAuthAttribute(key)
                .map_err(Error::Auth)?
                .and_then(|v| match v {
                    openrust::database::Value::Text(v) => Some(v),
                    _ => None,
                }))
        };
        use sha2::{Digest, Sha256};
        let email = text("email")?;
        let avatar = text("avatar")?.unwrap_or_else(|| match &email {
            Some(email) => format!(
                "https://gravatar.com/avatar/{}?d=mp",
                Sha256::digest(email.trim().to_lowercase().as_bytes())
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>()
            ),
            None => "https://gravatar.com/avatar?d=mp".into(),
        });
        Ok(User {
            name: text("name")?.unwrap_or_else(|| format!("ID: {}", user.get_auth_identifier())),
            extra: email,
            avatar: Some(avatar),
        })
    }
}
impl ResolvesUsers for Users {
    fn setFieldResolver(&self, resolver: Arc<FieldResolver>) -> Result<()> {
        *self.fields.write().unwrap_or_else(|e| e.into_inner()) = Some(resolver);
        Ok(())
    }
    fn load<'a>(&'a self, keys: &'a [String]) -> PulseFuture<'a, ()> {
        Box::pin(async move {
            let config = Config::getFacadeRoot()?;
            let auth = Auth::manager().map_err(Error::Auth)?;
            let provider = config
                .get(&format!("auth.guards.{}.provider", auth.getDefaultDriver()))
                .and_then(|v| v.as_str().map(str::to_owned));
            let mut resolved = BTreeMap::new();
            if let Some(provider) = provider {
                let provider = auth.createUserProvider(&provider).map_err(Error::Auth)?;
                for key in keys {
                    if let Some(user) = provider
                        .retrieve_by_id(&key.clone().into())
                        .await
                        .map_err(Error::Auth)?
                    {
                        resolved.insert(key.clone(), self.fields(user.as_ref())?);
                    }
                }
            }
            *self.resolved.write().unwrap_or_else(|e| e.into_inner()) = resolved;
            Ok(())
        })
    }
    fn find(&self, key: &str) -> User {
        self.resolved
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(key)
            .cloned()
            .unwrap_or_else(|| User {
                name: format!("ID: {key}"),
                extra: None,
                avatar: None,
            })
    }
}

pub type LegacyUserResolver = dyn Fn(&[String]) -> Result<BTreeMap<String, User>> + Send + Sync;
/// Bulk callback adapter for the upstream deprecated users API.
pub struct LegacyUsers {
    resolver: Arc<LegacyUserResolver>,
    resolved: RwLock<BTreeMap<String, User>>,
}
impl LegacyUsers {
    pub fn new(resolver: Arc<LegacyUserResolver>) -> Self {
        Self {
            resolver,
            resolved: RwLock::default(),
        }
    }
}
impl ResolvesUsers for LegacyUsers {
    fn load<'a>(&'a self, keys: &'a [String]) -> PulseFuture<'a, ()> {
        Box::pin(async move {
            let users = (self.resolver)(keys)?;
            *self.resolved.write().unwrap_or_else(|e| e.into_inner()) = users;
            Ok(())
        })
    }
    fn find(&self, key: &str) -> User {
        self.resolved
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(key)
            .cloned()
            .unwrap_or_else(|| User {
                name: format!("ID: {key}"),
                extra: None,
                avatar: None,
            })
    }
}
