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
    #[error(transparent)]
    Cache(#[from] openrust::cache::CacheError),
}

pub type FortifyResult<T> = Result<T, FortifyError>;
