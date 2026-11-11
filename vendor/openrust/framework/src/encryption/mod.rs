//! Application encryption and key rotation.

mod encrypter;
mod encryption_service_provider;
mod facade;
mod payload;

pub use encrypter::Encrypter;
pub use encryption_service_provider::EncryptionServiceProvider;
pub(crate) use encryption_service_provider::configured_cipher;
pub use facade::Crypt;

/// Cipher profiles supported by the application encrypter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Cipher {
    Aes128Cbc,
    Aes256Cbc,
    Aes128Gcm,
    Aes256Gcm,
}

/// Encryption, key configuration, or serialization failure.
#[derive(Debug)]
#[non_exhaustive]
pub enum EncryptException {
    MissingKey,
    InvalidKey,
    Unavailable(crate::contracts::container::BindingResolutionError),
    UnsupportedCipher,
    Serialization,
    EncryptionFailed,
}

/// An encrypted payload could not be authenticated or decoded.
#[derive(Debug)]
#[non_exhaustive]
pub enum DecryptException {
    InvalidPayload,
    Unavailable(crate::contracts::container::BindingResolutionError),
    AuthenticationFailed,
    Deserialization,
}

impl Cipher {
    pub fn key_length(self) -> usize {
        match self {
            Self::Aes128Cbc | Self::Aes128Gcm => 16,
            Self::Aes256Cbc | Self::Aes256Gcm => 32,
        }
    }
}

impl std::str::FromStr for Cipher {
    type Err = EncryptException;

    fn from_str(cipher: &str) -> Result<Self, Self::Err> {
        match cipher.to_ascii_lowercase().as_str() {
            "aes-128-cbc" => Ok(Self::Aes128Cbc),
            "aes-256-cbc" => Ok(Self::Aes256Cbc),
            "aes-128-gcm" => Ok(Self::Aes128Gcm),
            "aes-256-gcm" => Ok(Self::Aes256Gcm),
            _ => Err(EncryptException::UnsupportedCipher),
        }
    }
}

impl std::fmt::Display for EncryptException {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::MissingKey => "No application encryption key has been specified.",
            Self::InvalidKey => "Incorrect application encryption key length or encoding.",
            Self::UnsupportedCipher => "Unsupported application encryption cipher.",
            Self::Unavailable(_) => "The application encrypter is unavailable.",
            Self::Serialization | Self::EncryptionFailed => "Could not encrypt the data.",
        })
    }
}

impl std::error::Error for EncryptException {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Unavailable(error) => Some(error),
            _ => None,
        }
    }
}

impl std::fmt::Display for DecryptException {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidPayload => "The payload is invalid.",
            Self::AuthenticationFailed => "Could not decrypt the data.",
            Self::Deserialization => "The decrypted data is not a serialized PHP string.",
            Self::Unavailable(_) => "The application encrypter is unavailable.",
        })
    }
}

impl std::error::Error for DecryptException {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Unavailable(error) => Some(error),
            _ => None,
        }
    }
}
