use std::{fmt, future::Future, pin::Pin};

pub type SettingsFuture<'a, T> = Pin<Box<dyn Future<Output = SettingsResult<T>> + Send + 'a>>;
pub type SettingsResult<T> = Result<T, SettingsError>;

/// Errors deliberately exclude SQL bindings, property values and Serde diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsError {
    InvalidDefinition,
    InvalidValue,
    UnknownProperty,
    Locked,
    Conflict,
    LimitExceeded,
    Database,
    Encryption,
    Decryption,
    Commit,
}

impl fmt::Display for SettingsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidDefinition => "Invalid settings group, property, table or registration.",
            Self::InvalidValue => "Settings values do not match the declared schema or validation.",
            Self::UnknownProperty => "The settings property is not declared.",
            Self::Locked => "The settings property is locked.",
            Self::Conflict => "Settings have changed. Refresh before saving again.",
            Self::LimitExceeded => "Settings exceed the configured size or property limit.",
            Self::Database => "Settings database operation failed; check connection and migrations.",
            Self::Encryption => "Settings encryption failed.",
            Self::Decryption => "Settings decryption failed; check the application key ring.",
            Self::Commit => "Settings commit failed or its completion hook failed; reload to confirm persistence.",
        })
    }
}
impl std::error::Error for SettingsError {}
