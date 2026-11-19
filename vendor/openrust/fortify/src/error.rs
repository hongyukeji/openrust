/// Errors preserve the upstream distinction between malformed secrets and invalid codes.
#[derive(Debug, thiserror::Error)]
pub enum FortifyError {
    #[error("The two-factor secret contains invalid Base32 characters.")]
    InvalidCharacters,
    #[error("The two-factor secret is not compatible with Google Authenticator.")]
    IncompatibleWithGoogleAuthenticator,
    #[error("The two-factor secret is too short.")]
    SecretKeyTooShort,
    #[error("The two-factor time or window overflows its supported range.")]
    InvalidTime,
    #[error("The two-factor secret exceeds available memory.")]
    Allocation,
    #[error("The two-factor secure random source is unavailable.")]
    Entropy,
    #[error("The cached two-factor timestamp is invalid.")]
    InvalidCacheTimestamp,
    #[error("The two-factor model attribute has an invalid type: {0}.")]
    InvalidAttribute(&'static str),
    #[error("The two-factor secret length is outside the supported range.")]
    InvalidSecretLength,
    #[error(transparent)]
    Configuration(#[from] openrust::config::ConfigError),
    #[error(transparent)]
    Unavailable(#[from] openrust::contracts::container::BindingResolutionError),
    #[error(transparent)]
    Encryption(#[from] openrust::encryption::EncryptException),
    #[error(transparent)]
    Decryption(#[from] openrust::encryption::DecryptException),
    #[error(transparent)]
    Database(#[from] openrust::database::DatabaseError),
    #[error(transparent)]
    Authentication(#[from] openrust::auth::AuthError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("The decrypted two-factor value is not UTF-8.")]
    InvalidUtf8,
    #[error(transparent)]
    Cache(#[from] openrust::cache::CacheError),
}

pub type FortifyResult<T> = Result<T, FortifyError>;
