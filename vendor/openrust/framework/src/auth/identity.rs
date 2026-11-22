use std::{collections::BTreeMap, fmt};

use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::{
    database::Value,
    http::{HeaderMap, HttpError},
};

/// An identifier with its database representation preserved.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AuthIdentifier {
    Integer(i64),
    UnsignedInteger(u64),
    String(String),
}

impl fmt::Display for AuthIdentifier {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Integer(value) => value.fmt(formatter),
            Self::UnsignedInteger(value) => value.fmt(formatter),
            Self::String(value) => value.fmt(formatter),
        }
    }
}

impl From<u64> for AuthIdentifier {
    fn from(value: u64) -> Self {
        Self::UnsignedInteger(value)
    }
}

impl From<String> for AuthIdentifier {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

impl From<&str> for AuthIdentifier {
    fn from(value: &str) -> Self {
        Self::String(value.to_owned())
    }
}

impl From<AuthIdentifier> for Value {
    fn from(id: AuthIdentifier) -> Self {
        match id {
            AuthIdentifier::Integer(value) => Self::Integer(value),
            AuthIdentifier::UnsignedInteger(value) => Self::Unsigned(value),
            AuthIdentifier::String(value) => Self::Text(value),
        }
    }
}

impl TryFrom<&Value> for AuthIdentifier {
    type Error = AuthError;

    fn try_from(value: &Value) -> Result<Self, Self::Error> {
        match value {
            Value::Integer(value) => Ok(Self::Integer(*value)),
            Value::Unsigned(value) => Ok(Self::UnsignedInteger(*value)),
            Value::Text(value) => Ok(Self::String(value.clone())),
            _ => Err(AuthError::InvalidIdentity),
        }
    }
}

/// Lookup constraints and a separate, zeroized password.
pub struct Credentials {
    fields: BTreeMap<String, Value>,
    password: Zeroizing<String>,
}

impl Credentials {
    pub fn new(
        fields: impl IntoIterator<Item = (impl Into<String>, Value)>,
        password: impl Into<String>,
    ) -> Self {
        Self {
            fields: fields
                .into_iter()
                .map(|(key, value)| (key.into(), value))
                .collect(),
            password: Zeroizing::new(password.into()),
        }
    }

    pub fn fields(&self) -> &BTreeMap<String, Value> {
        &self.fields
    }
    pub fn password(&self) -> &str {
        &self.password
    }
}

impl fmt::Debug for Credentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Credentials").finish_non_exhaustive()
    }
}

#[derive(Debug)]
#[non_exhaustive]
pub enum AuthError {
    Configuration(&'static str),
    InvalidIdentity,
    InvalidPassword,
    UnprotectedRememberCookie,
    Backend(Box<dyn std::error::Error + Send + Sync>),
    Http(Box<HttpError>),
    Throttled(u64),
}

impl AuthError {
    pub(crate) fn backend(error: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self::Backend(Box::new(error))
    }
}

impl fmt::Display for AuthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Configuration(part) => write!(f, "invalid authentication configuration: {part}"),
            Self::InvalidIdentity => f.write_str("invalid authentication identity"),
            Self::InvalidPassword => {
                f.write_str("The given password does not match the current password.")
            }
            Self::UnprotectedRememberCookie => {
                f.write_str("remember cookie encryption is required")
            }
            Self::Backend(_) => f.write_str("authentication backend failed"),
            Self::Http(error) => error.fmt(f),
            Self::Throttled(_) => f.write_str("too many authentication attempts"),
        }
    }
}

impl std::error::Error for AuthError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Backend(source) => Some(source.as_ref()),
            Self::Http(source) => Some(source.as_ref()),
            _ => None,
        }
    }
}

impl From<HttpError> for AuthError {
    fn from(error: HttpError) -> Self {
        Self::Http(Box::new(error))
    }
}

impl From<AuthError> for HttpError {
    fn from(error: AuthError) -> Self {
        match error {
            AuthError::Http(source) => *source,
            AuthError::Throttled(retry) => {
                let mut headers = HeaderMap::new();
                headers.insert(::http::header::RETRY_AFTER, retry.into());
                Self::Throttled { headers }
            }
            source => Self::Internal {
                source: Box::new(source),
            },
        }
    }
}

impl From<crate::contracts::container::BindingResolutionError> for AuthError {
    fn from(source: crate::contracts::container::BindingResolutionError) -> Self {
        Self::backend(source)
    }
}

impl From<crate::database::DatabaseError> for AuthError {
    fn from(source: crate::database::DatabaseError) -> Self {
        Self::backend(source)
    }
}

impl From<crate::cache::CacheError> for AuthError {
    fn from(source: crate::cache::CacheError) -> Self {
        Self::backend(source)
    }
}

impl From<crate::hashing::HashError> for AuthError {
    fn from(source: crate::hashing::HashError) -> Self {
        Self::backend(source)
    }
}

impl From<crate::session::SessionError> for AuthError {
    fn from(source: crate::session::SessionError) -> Self {
        Self::backend(source)
    }
}

impl From<crate::config::ConfigError> for AuthError {
    fn from(source: crate::config::ConfigError) -> Self {
        Self::backend(source)
    }
}

impl From<crate::events::EventError> for AuthError {
    fn from(source: crate::events::EventError) -> Self {
        Self::backend(source)
    }
}

impl From<crate::cookie::CookieError> for AuthError {
    fn from(source: crate::cookie::CookieError) -> Self {
        Self::backend(source)
    }
}

impl From<serde_json::Error> for AuthError {
    fn from(source: serde_json::Error) -> Self {
        Self::backend(source)
    }
}

impl From<crate::routing::UrlError> for AuthError {
    fn from(source: crate::routing::UrlError) -> Self {
        Self::backend(source)
    }
}
