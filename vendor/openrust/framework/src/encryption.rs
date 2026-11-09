//! 在应用进程内对应用数据进行认证加密。
#![doc = include_str!("encryption/README.md")]
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
use crate::core::{Error, Result};

mod application;
mod application_payload;
#[cfg(feature = "session")]
pub(crate) use application::decrypt_xsrf_header;
pub use application::{ApplicationEncrypter, ConfiguredEncrypter};
pub(crate) use application::{decrypt_cookie_value, encrypt_cookie_value};

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

/// 使用固定 AES-256-GCM-SIV 算法和可轮换密钥环的认证加密服务。
#[derive(Clone)]
pub struct Encrypter {
    inner: Arc<KeyRing>,
}

impl Encrypter {
    /// 校验并加载当前加密密钥，以及仅用于解密的历史密钥。
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

    /// 加密最多 1 MiB 的明文，不附加应用专属上下文。
    pub fn encrypt(&self, plaintext: &[u8]) -> Result<String> {
        self.encrypt_with_context(plaintext, &[])
    }

    /// 加密明文，并认证调用方提供的上下文；上下文最多为 1 KiB。
    ///
    /// 上下文不会存入密文封装。解密时调用方必须提供相同的可信上下文；不要从不可信的密文元数据中复制上下文。
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

    /// 解密由 [`Self::encrypt`] 或 [`Self::encrypt_with_context`] 生成的密文封装。
    pub fn decrypt(&self, envelope: &str) -> Result<Vec<u8>> {
        self.decrypt_with_context(envelope, &[])
    }

    /// 使用加密时由调用方提供的上下文解密密文封装。
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

impl std::fmt::Debug for Encrypter {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Encryptor")
            .field("active_key_id", &self.inner.active_key_id)
            .field("key_count", &self.inner.keys.len())
            .field("keys", &"[REDACTED]")
            .finish()
    }
}

/// 将加密服务注册到统一容器；首次解析时读取并校验当前密钥。
///
#[derive(Debug, Default, Clone, Copy)]
pub struct EncryptionServiceProvider;

impl crate::support::ServiceProvider for EncryptionServiceProvider {
    fn register<'a>(
        &'a self,
        app: &'a mut crate::foundation::RegisterContext<'_>,
    ) -> crate::support::BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            use crate::config::repository::SectionSchema;
            app.config_section(
                "security.encryption",
                serde_json::json!({}),
                SectionSchema::object([
                    ("active_key_id", SectionSchema::String),
                    (
                        "keys",
                        SectionSchema::map(SectionSchema::String.sensitive()),
                    ),
                ]),
                |_| Ok(()),
            )?;
            if !app.is_bound::<dyn ApplicationEncrypter>() {
                app.singleton::<dyn ApplicationEncrypter, _>(|resolver| {
                    Box::pin(async move {
                        let config = resolver.section::<crate::config::AppConfig>("app")?;
                        Ok(Arc::new(ConfiguredEncrypter::from_config(config)?)
                            as Arc<dyn ApplicationEncrypter>)
                    })
                })
                .await?;
            }
            app.singleton::<Encrypter, _>(|resolver| {
                Box::pin(async move {
                    let config = resolver.section::<EncryptionConfig>("security.encryption")?;
                    Ok(Arc::new(Encrypter::new(config)?))
                })
            })
            .await
        })
    }
}

#[cfg(test)]
#[path = "../tests/unit/encryption/provider.rs"]
mod provider_tests;

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
#[path = "../tests/unit/encryption/contextual.rs"]
mod tests;
