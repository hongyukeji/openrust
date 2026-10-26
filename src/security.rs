//! In-process authenticated encryption for application data.
#![doc = include_str!("security/README.md")]
#![forbid(unsafe_code)]
#![deny(missing_docs)]

use std::collections::BTreeMap;
use std::sync::Arc;

use aes_gcm_siv::aead::{Aead, KeyInit, Payload};
use aes_gcm_siv::{Aes256GcmSiv, Nonce};
use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use zeroize::Zeroizing;

use crate::config::ConfigError;
pub use crate::config::EncryptionConfig;
use crate::core::{AppBuilder, Error, Plugin, Result};

const ENVELOPE_VERSION: &str = "or1";
const KEY_BYTES: usize = 32;
const NONCE_BYTES: usize = 12;
const TAG_BYTES: usize = 16;
const MAX_KEYS: usize = 16;
const MAX_KEY_ID_BYTES: usize = 64;
const MAX_PLAINTEXT_BYTES: usize = 1024 * 1024;
const MAX_CONTEXT_BYTES: usize = 1024;
const MAX_ENVELOPE_BYTES: usize = 1_400_000;
const AAD_DOMAIN: &[u8] = b"openrust:application-encryption:or1";

#[derive(Clone)]
struct KeyRing {
    active_key_id: String,
    keys: BTreeMap<String, Aes256GcmSiv>,
}

/// Authenticated encryption service using a fixed AES-256-GCM-SIV algorithm and a rotatable key ring.
#[derive(Clone)]
pub struct Encryptor {
    inner: Arc<KeyRing>,
}

impl Encryptor {
    /// Validates and loads the active encryption key and decryption-only historical keys.
    pub fn new(config: EncryptionConfig) -> Result<Self> {
        validate_key_id(&config.active_key_id)?;
        if config.keys.is_empty() || config.keys.len() > MAX_KEYS {
            return Err(invalid_config(format!(
                "security.encryption.keys must contain between 1 and {MAX_KEYS} keys"
            )));
        }
        if !config.keys.contains_key(&config.active_key_id) {
            return Err(invalid_config(
                "security.encryption.active_key_id must name one configured key",
            ));
        }

        let mut keys = BTreeMap::new();
        for (key_id, encoded) in config.keys {
            validate_key_id(&key_id)?;
            let decoded = Zeroizing::new(STANDARD.decode(encoded.0.as_bytes()).map_err(|_| {
                invalid_config(format!(
                    "security.encryption.keys.{key_id} must be valid standard Base64"
                ))
            })?);
            if decoded.len() != KEY_BYTES {
                return Err(invalid_config(format!(
                    "security.encryption.keys.{key_id} must decode to exactly {KEY_BYTES} bytes"
                )));
            }
            let cipher = Aes256GcmSiv::new_from_slice(&decoded)
                .map_err(|_| invalid_config("could not initialize configured encryption key"))?;
            keys.insert(key_id, cipher);
        }

        Ok(Self {
            inner: Arc::new(KeyRing {
                active_key_id: config.active_key_id,
                keys,
            }),
        })
    }

    /// Encrypts up to 1 MiB of plaintext with no application-specific context.
    pub fn encrypt(&self, plaintext: &[u8]) -> Result<String> {
        self.encrypt_with_context(plaintext, &[])
    }

    /// Encrypts plaintext while authenticating a caller-supplied context of at most 1 KiB.
    ///
    /// The context is not stored in the envelope. The caller must supply the same trusted context
    /// when decrypting; do not copy it from untrusted ciphertext metadata.
    pub fn encrypt_with_context(&self, plaintext: &[u8], context: &[u8]) -> Result<String> {
        validate_input_sizes(plaintext.len(), context.len())?;
        let key_id = &self.inner.active_key_id;
        let cipher = self
            .inner
            .keys
            .get(key_id)
            .ok_or_else(|| Error::internal(MissingActiveEncryptionKey))?;
        let mut nonce_bytes = [0_u8; NONCE_BYTES];
        getrandom::fill(&mut nonce_bytes).map_err(|_| Error::internal(RandomnessFailure))?;
        let nonce = Nonce::from(nonce_bytes);
        let aad = associated_data(key_id, context)?;
        let ciphertext = cipher
            .encrypt(
                &nonce,
                Payload {
                    msg: plaintext,
                    aad: &aad,
                },
            )
            .map_err(|_| Error::internal(EncryptionFailure))?;
        Ok(format!(
            "{ENVELOPE_VERSION}.{key_id}.{}.{}",
            URL_SAFE_NO_PAD.encode(nonce_bytes),
            URL_SAFE_NO_PAD.encode(ciphertext)
        ))
    }

    /// Decrypts an envelope produced by [`Self::encrypt`] or [`Self::encrypt_with_context`].
    pub fn decrypt(&self, envelope: &str) -> Result<Vec<u8>> {
        self.decrypt_with_context(envelope, &[])
    }

    /// Decrypts an envelope using the caller-supplied context used during encryption.
    pub fn decrypt_with_context(&self, envelope: &str, context: &[u8]) -> Result<Vec<u8>> {
        if context.len() > MAX_CONTEXT_BYTES {
            return Err(input_too_large());
        }
        if envelope.len() > MAX_ENVELOPE_BYTES {
            return Err(invalid_ciphertext());
        }
        let mut fields = envelope.split('.');
        let version = fields.next().ok_or_else(invalid_ciphertext)?;
        let key_id = fields.next().ok_or_else(invalid_ciphertext)?;
        let nonce_encoded = fields.next().ok_or_else(invalid_ciphertext)?;
        let ciphertext_encoded = fields.next().ok_or_else(invalid_ciphertext)?;
        if fields.next().is_some() {
            return Err(invalid_ciphertext());
        }
        if version != ENVELOPE_VERSION {
            return Err(Error::custom(
                "security_unsupported_ciphertext_version",
                "encrypted data uses an unsupported format version",
            ));
        }
        validate_key_id(key_id).map_err(|_| invalid_ciphertext())?;
        let cipher = self.inner.keys.get(key_id).ok_or_else(|| {
            Error::custom(
                "security_unknown_key_id",
                "encrypted data references an unavailable key",
            )
        })?;

        let nonce_bytes = decode_canonical(nonce_encoded).ok_or_else(invalid_ciphertext)?;
        if nonce_bytes.len() != NONCE_BYTES {
            return Err(invalid_ciphertext());
        }
        let nonce_array: [u8; NONCE_BYTES] = nonce_bytes
            .as_slice()
            .try_into()
            .map_err(|_| invalid_ciphertext())?;
        let nonce = Nonce::from(nonce_array);
        let ciphertext = decode_canonical(ciphertext_encoded).ok_or_else(invalid_ciphertext)?;
        if ciphertext.len() < TAG_BYTES || ciphertext.len() > MAX_PLAINTEXT_BYTES + TAG_BYTES {
            return Err(invalid_ciphertext());
        }
        let aad = associated_data(key_id, context)?;
        cipher
            .decrypt(
                &nonce,
                Payload {
                    msg: &ciphertext,
                    aad: &aad,
                },
            )
            .map_err(|_| {
                Error::custom(
                    "security_decryption_failed",
                    "encrypted data could not be authenticated",
                )
            })
    }
}

impl std::fmt::Debug for Encryptor {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Encryptor")
            .field("active_key_id", &self.inner.active_key_id)
            .field("key_count", &self.inner.keys.len())
            .field("keys", &"[REDACTED]")
            .finish()
    }
}

/// Installs the configured [`Encryptor`] service.
#[derive(Debug, Default, Clone, Copy)]
pub struct EncryptionPlugin;

impl Plugin for EncryptionPlugin {
    fn build(&self, app: &mut AppBuilder) -> Result<()> {
        let config = app
            .sensitive_section::<EncryptionConfig>("security.encryption")
            .map_err(Error::Config)?;
        app.insert(Encryptor::new(config)?);
        Ok(())
    }
}

fn validate_key_id(key_id: &str) -> Result<()> {
    if key_id.is_empty()
        || key_id.len() > MAX_KEY_ID_BYTES
        || !key_id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"_-".contains(&byte))
    {
        return Err(invalid_config(
            "security.encryption key IDs must contain 1-64 lowercase ASCII letters, digits, `_` or `-`",
        ));
    }
    Ok(())
}

fn validate_input_sizes(plaintext_len: usize, context_len: usize) -> Result<()> {
    if plaintext_len > MAX_PLAINTEXT_BYTES || context_len > MAX_CONTEXT_BYTES {
        return Err(input_too_large());
    }
    Ok(())
}

fn associated_data(key_id: &str, context: &[u8]) -> Result<Vec<u8>> {
    let mut aad = Vec::with_capacity(AAD_DOMAIN.len() + 2 + key_id.len() + context.len());
    aad.extend_from_slice(AAD_DOMAIN);
    let key_id_len = u16::try_from(key_id.len()).map_err(|_| Error::internal(EncryptionFailure))?;
    aad.extend_from_slice(&key_id_len.to_be_bytes());
    aad.extend_from_slice(key_id.as_bytes());
    aad.extend_from_slice(context);
    Ok(aad)
}

fn decode_canonical(encoded: &str) -> Option<Vec<u8>> {
    let decoded = URL_SAFE_NO_PAD.decode(encoded).ok()?;
    (URL_SAFE_NO_PAD.encode(&decoded) == encoded).then_some(decoded)
}

fn invalid_config(message: impl Into<String>) -> Error {
    Error::Config(ConfigError::Invalid(message.into()))
}

fn input_too_large() -> Error {
    Error::custom(
        "security_input_too_large",
        "plaintext or encryption context exceeds its configured size limit",
    )
}

fn invalid_ciphertext() -> Error {
    Error::custom(
        "security_invalid_ciphertext",
        "encrypted data has an invalid format",
    )
}

#[derive(Debug, thiserror::Error)]
#[error("encryption key ring is missing its active key")]
struct MissingActiveEncryptionKey;

#[derive(Debug, thiserror::Error)]
#[error("OS randomness provider failed")]
struct RandomnessFailure;

#[derive(Debug, thiserror::Error)]
#[error("encryption operation failed")]
struct EncryptionFailure;

#[cfg(test)]
mod tests {
    use super::*;

    fn key(value: u8) -> String {
        STANDARD.encode([value; KEY_BYTES])
    }

    fn config(active: &str, keys: &[(&str, u8)]) -> EncryptionConfig {
        EncryptionConfig::new(
            active,
            keys.iter()
                .map(|(key_id, value)| ((*key_id).to_owned(), key(*value)))
                .collect(),
        )
    }

    #[test]
    fn encrypts_binary_and_empty_plaintext_and_generates_fresh_nonces() -> Result<()> {
        let encryptor = Encryptor::new(config("current", &[("current", 7)]))?;
        for plaintext in [&b"\0OpenRust\xff"[..], &b""[..]] {
            let first = encryptor.encrypt(plaintext)?;
            let second = encryptor.encrypt(plaintext)?;
            assert_ne!(first, second);
            assert_eq!(encryptor.decrypt(&first)?, plaintext);
        }
        Ok(())
    }

    #[test]
    fn context_is_authenticated_and_must_come_from_the_caller() -> Result<()> {
        let encryptor = Encryptor::new(config("current", &[("current", 7)]))?;
        let ciphertext = encryptor.encrypt_with_context(b"pii", b"users.email:42")?;
        assert_eq!(
            encryptor.decrypt_with_context(&ciphertext, b"users.email:42")?,
            b"pii"
        );
        assert_eq!(
            encryptor
                .decrypt_with_context(&ciphertext, b"users.email:43")
                .err()
                .map(|error| error.code().to_owned()),
            Some("security_decryption_failed".to_owned())
        );
        Ok(())
    }

    #[test]
    fn key_rotation_writes_only_with_active_key_and_reads_old_ciphertexts() -> Result<()> {
        let previous = Encryptor::new(config("old", &[("old", 3)]))?;
        let legacy = previous.encrypt(b"existing value")?;
        let all_keys = Encryptor::new(config("old", &[("old", 3), ("new", 9)]))?;
        let rotated = Encryptor::new(config("new", &[("old", 3), ("new", 9)]))?;
        assert_eq!(all_keys.decrypt(&legacy)?, b"existing value");
        assert_eq!(rotated.decrypt(&legacy)?, b"existing value");
        let current = rotated.encrypt(b"new value")?;
        assert!(current.starts_with("or1.new."));
        assert_eq!(rotated.decrypt(&current)?, b"new value");
        assert_eq!(
            Encryptor::new(config("new", &[("new", 9)]))?
                .decrypt(&legacy)
                .err()
                .map(|e| e.code().to_owned()),
            Some("security_unknown_key_id".to_owned())
        );
        Ok(())
    }

    #[test]
    fn malformed_or_tampered_envelopes_fail_without_echoing_input() -> Result<()> {
        let encryptor = Encryptor::new(config("current", &[("current", 7)]))?;
        let ciphertext = encryptor.encrypt(b"secret plaintext")?;
        for malformed in [
            "or1.current.bad.bad",
            "or1.current.AA.bad",
            "or1.current.AAAAAAAAAAAAAAAA.AA",
            "or1.current.AAAAAAAAAAAAAAAA.AA.extra",
            "or9.current.AAAAAAAAAAAAAAAA.AAAAAAAAAAAAAAAAAAAAAA",
        ] {
            assert!(encryptor.decrypt(malformed).is_err());
        }
        let mut fields = ciphertext.split('.').map(str::to_owned).collect::<Vec<_>>();
        fields[3].push('A');
        let tampered = fields.join(".");
        let error = encryptor
            .decrypt(&tampered)
            .err()
            .ok_or_else(|| Error::custom("test_failure", "tampering must be rejected"))?;
        assert_eq!(error.code(), "security_decryption_failed");
        assert!(!error.to_string().contains("secret plaintext"));
        assert!(
            encryptor
                .decrypt(&format!("{ciphertext}.trailing"))
                .is_err()
        );
        Ok(())
    }

    #[test]
    fn rejects_bad_key_rings_and_redacts_configuration_debug() {
        let invalid = [
            EncryptionConfig::new("missing", [("other".to_owned(), key(1))].into()),
            EncryptionConfig::new("bad id", [("bad id".to_owned(), key(1))].into()),
            EncryptionConfig::new(
                "current",
                [("current".to_owned(), "not-base64".into())].into(),
            ),
            EncryptionConfig::new(
                "current",
                [("current".to_owned(), STANDARD.encode([1_u8; 31]))].into(),
            ),
        ];
        for config in invalid {
            assert!(Encryptor::new(config).is_err());
        }
        let secret = key(42);
        let config =
            EncryptionConfig::new("current", [("current".to_owned(), secret.clone())].into());
        assert!(!format!("{config:?}").contains(&secret));
    }

    #[test]
    fn rejects_oversized_inputs_before_encryption() -> Result<()> {
        let encryptor = Encryptor::new(config("current", &[("current", 7)]))?;
        assert_eq!(
            encryptor
                .encrypt(&vec![0_u8; MAX_PLAINTEXT_BYTES + 1])
                .err()
                .map(|e| e.code().to_owned()),
            Some("security_input_too_large".to_owned())
        );
        assert_eq!(
            encryptor
                .encrypt_with_context(b"ok", &vec![0_u8; MAX_CONTEXT_BYTES + 1])
                .err()
                .map(|e| e.code().to_owned()),
            Some("security_input_too_large".to_owned())
        );
        Ok(())
    }

    #[test]
    fn known_aes_256_gcm_siv_vector_matches_rfc_8452() -> Result<()> {
        let mut key_bytes = [0_u8; KEY_BYTES];
        key_bytes[0] = 1;
        let cipher = Aes256GcmSiv::new_from_slice(&key_bytes).map_err(Error::internal)?;
        let mut nonce_bytes = [0_u8; NONCE_BYTES];
        nonce_bytes[0] = 3;
        let nonce = Nonce::from(nonce_bytes);
        let ciphertext = cipher
            .encrypt(&nonce, Payload { msg: b"", aad: b"" })
            .map_err(|_| Error::internal(EncryptionFailure))?;
        assert_eq!(
            URL_SAFE_NO_PAD.encode(ciphertext),
            URL_SAFE_NO_PAD.encode([
                0x07, 0xf5, 0xf4, 0x16, 0x9b, 0xbf, 0x55, 0xa8, 0x40, 0x0c, 0xd4, 0x7e, 0xa6, 0xfd,
                0x40, 0x0f
            ])
        );
        Ok(())
    }

    #[test]
    fn independent_encryptors_with_the_same_ring_can_decrypt_each_other() -> Result<()> {
        let first = Encryptor::new(config("current", &[("current", 7)]))?;
        let second = Encryptor::new(config("current", &[("current", 7)]))?;
        let envelope = first.encrypt(b"shared application data")?;
        assert_eq!(second.decrypt(&envelope)?, b"shared application data");
        Ok(())
    }
}
