use rsa::{RsaPrivateKey, RsaPublicKey, pkcs1::DecodeRsaPrivateKey, pkcs8::DecodePrivateKey};
use std::{fmt, path::PathBuf};
use zeroize::Zeroizing;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum KeyError {
    InvalidConfiguration,
    Unreadable,
    Invalid,
    Generation,
    Write,
    Exists,
    Permissions,
}
impl fmt::Display for KeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Permissions => "Passport key permissions are not correct; use 600 or 660.",
            Self::InvalidConfiguration => "Invalid Passport key configuration.",
            Self::Unreadable => "Unable to read the Passport key.",
            Self::Invalid => "Invalid Passport RSA key.",
            Self::Generation => "Unable to generate Passport RSA keys.",
            Self::Write => "Unable to write Passport RSA keys.",
            Self::Exists => {
                "Encryption keys already exist. Use the --force option to overwrite them."
            }
        })
    }
}
impl std::error::Error for KeyError {}

/// A validated RSA PEM; debug output never includes its contents.
pub struct CryptKey {
    content: Zeroizing<String>,
    path: Option<PathBuf>,
}
impl fmt::Debug for CryptKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("CryptKey([REDACTED])")
    }
}
#[allow(non_snake_case)]
impl CryptKey {
    pub(crate) fn new(
        content: Zeroizing<String>,
        path: Option<PathBuf>,
        mode: Option<u32>,
    ) -> Result<Self, KeyError> {
        let private = RsaPrivateKey::from_pkcs8_pem(&content)
            .or_else(|_| RsaPrivateKey::from_pkcs1_pem(&content));
        let valid = match private {
            Ok(key) => key.validate().is_ok(),
            Err(_) => valid_public(&content),
        };
        if !valid {
            return Err(KeyError::Invalid);
        }
        if let Some(mode) = mode
            && !matches!(mode, 0o400 | 0o440 | 0o600 | 0o640 | 0o660)
        {
            return Err(KeyError::Permissions);
        }
        Ok(Self { content, path })
    }
    pub fn getKeyContents(&self) -> &str {
        &self.content
    }
    pub fn getKeyPath(&self) -> Option<&std::path::Path> {
        self.path.as_deref()
    }
}
#[derive(Debug)]
pub struct PrivateKey(pub(crate) CryptKey);
#[derive(Debug)]
pub struct PublicKey(pub(crate) CryptKey);
impl std::ops::Deref for PrivateKey {
    type Target = CryptKey;
    fn deref(&self) -> &CryptKey {
        &self.0
    }
}
impl std::ops::Deref for PublicKey {
    type Target = CryptKey;
    fn deref(&self) -> &CryptKey {
        &self.0
    }
}

fn valid_public(content: &str) -> bool {
    use rsa::pkcs8::der::Decode;
    let Ok((label, document)) = rsa::pkcs8::Document::from_pem(content) else {
        return false;
    };
    let der = if label == "PUBLIC KEY" {
        let Ok(info) = rsa::pkcs8::SubjectPublicKeyInfoRef::from_der(document.as_bytes()) else {
            return false;
        };
        if info.algorithm.oid != rsa::pkcs8::ObjectIdentifier::new_unwrap("1.2.840.113549.1.1.1") {
            return false;
        }
        let Some(key) = info.subject_public_key.as_bytes() else {
            return false;
        };
        key
    } else if label == "RSA PUBLIC KEY" {
        document.as_bytes()
    } else {
        return false;
    };
    let Ok(key) = rsa::pkcs1::RsaPublicKey::from_der(der) else {
        return false;
    };
    RsaPublicKey::new_with_max_size(
        rsa::BigUint::from_bytes_be(key.modulus.as_bytes()),
        rsa::BigUint::from_bytes_be(key.public_exponent.as_bytes()),
        usize::MAX,
    )
    .is_ok()
}
