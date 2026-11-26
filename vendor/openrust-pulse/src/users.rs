use crate::{Error, PulseFuture, Result};
use openrust::{
    contracts::auth::Authenticatable,
    support::{
        facades::{Auth, Config, Facade},
        php_regular_compare,
    },
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    sync::{Arc, RwLock},
};
const DEFAULT_AVATAR: &str = "https://gravatar.com/avatar?d=mp";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct User {
    pub name: String,
    pub extra: Option<String>,
    pub avatar: Option<String>,
}

impl User {
    /// 从账户字段生成用量面板资料，显式补充信息和头像优先。
    pub fn from_profile(
        name: impl Into<String>,
        email: Option<String>,
        extra: Option<String>,
        avatar: Option<String>,
    ) -> Self {
        let avatar = avatar.unwrap_or_else(|| {
            let Some(email) = email.as_deref().filter(|email| !matches!(*email, "" | "0")) else {
                return DEFAULT_AVATAR.into();
            };
            // PHP 的 strtolower 和 trim 只处理这里的默认 ASCII 字节。
            let normalized = email
                .trim_matches([' ', '\t', '\n', '\r', '\0', '\x0b'])
                .to_ascii_lowercase();
            use sha2::{Digest, Sha256};
            let hash: String = Sha256::digest(normalized.as_bytes())
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect();
            format!("https://gravatar.com/avatar/{hash}?d=mp")
        });
        Self {
            name: name.into(),
            extra: Some(extra.or(email).unwrap_or_default()),
            avatar: Some(avatar),
        }
    }
}

fn missing_user(key: &str) -> User {
    User::from_profile(format!("ID: {key}"), None, None, None)
}
/// Resolves metric identifiers and dashboard user details in the active application.
pub trait ResolvesUsers: Send + Sync {
    fn key(&self, user: &dyn Authenticatable) -> Option<String> {
        Some(user.get_auth_identifier().to_string())
    }
    fn set_field_resolver(&self, _resolver: Arc<FieldResolver>) -> Result<()> {
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
    pub fn set_field_resolver(
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
                .get_auth_attribute(key)
                .map_err(Error::Auth)?
                .and_then(|v| match v {
                    openrust::database::Value::Text(v) => Some(v),
                    _ => None,
                }))
        };
        let name = text("name")?.unwrap_or_else(|| format!("ID: {}", user.get_auth_identifier()));
        Ok(User::from_profile(
            name,
            text("email")?,
            None,
            text("avatar")?,
        ))
    }
}
impl ResolvesUsers for Users {
    fn set_field_resolver(&self, resolver: Arc<FieldResolver>) -> Result<()> {
        *self.fields.write().unwrap_or_else(|e| e.into_inner()) = Some(resolver);
        Ok(())
    }
    fn load<'a>(&'a self, keys: &'a [String]) -> PulseFuture<'a, ()> {
        Box::pin(async move {
            let config = Config::get_facade_root()?;
            let auth = Auth::manager().map_err(Error::Auth)?;
            let provider = config
                .get(&format!(
                    "auth.guards.{}.provider",
                    auth.get_default_driver()
                ))
                .and_then(|v| v.as_str().map(str::to_owned));
            let mut resolved = BTreeMap::new();
            if let Some(provider) = provider {
                let provider = auth.create_user_provider(&provider).map_err(Error::Auth)?;
                let identifiers: Vec<_> = keys.iter().cloned().map(Into::into).collect();
                if let Some(users) = provider
                    .retrieve_by_model_keys(&identifiers)
                    .await
                    .map_err(Error::Auth)?
                {
                    for key in keys {
                        let requested = Value::String(key.clone());
                        if let Some(user) = users.iter().find(|user| {
                            let actual = Value::String(user.get_auth_identifier().to_string());
                            php_regular_compare(&actual, &requested).is_eq()
                        }) {
                            resolved.insert(key.clone(), self.fields(user.as_ref())?);
                        }
                    }
                } else {
                    for (key, identifier) in keys.iter().zip(&identifiers) {
                        if let Some(user) = provider
                            .retrieve_by_id(identifier)
                            .await
                            .map_err(Error::Auth)?
                        {
                            resolved.insert(key.clone(), self.fields(user.as_ref())?);
                        }
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
            .unwrap_or_else(|| missing_user(key))
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
            .unwrap_or_else(|| missing_user(key))
    }
}
