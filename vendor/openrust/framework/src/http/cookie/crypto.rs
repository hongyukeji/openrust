//! 使用既有 AEAD 核心区分认证失败与已认证的跨名重放。
use serde_json::Value;

#[derive(Clone)]
pub(crate) enum Protection {
    Plain,
    #[cfg(feature = "encryption")]
    Encrypted(crate::encryption::Encrypter),
    #[cfg(feature = "encryption")]
    Laravel(std::sync::Arc<crate::encryption::LaravelEncrypter>),
}

// 由真实 Cookie 层建立的保护事实；不包含密钥或请求对象。
#[cfg(feature = "session")]
#[derive(Clone)]
pub(crate) struct CookieProtection {
    pub(crate) encrypted: bool,
    pub(crate) except: Vec<String>,
}

#[cfg(feature = "encryption")]
const COOKIE_DOMAIN: &[u8] = b"openrust:http-cookie:or1:v1";
#[cfg(feature = "encryption")]
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Payload {
    version: u8,
    name: String,
    value: String,
}

impl Protection {
    #[cfg(feature = "session")]
    pub(super) fn authenticity(&self, except: &[String]) -> CookieProtection {
        let encrypted = match self {
            Self::Plain => false,
            #[cfg(feature = "encryption")]
            Self::Encrypted(_) | Self::Laravel(_) => true,
        };
        CookieProtection {
            encrypted,
            except: except.to_vec(),
        }
    }

    pub(super) fn decrypt(&self, root: &mut Value, except: &[String]) {
        match self {
            Self::Plain => {
                let _ = (root, except);
            }
            #[cfg(feature = "encryption")]
            Self::Encrypted(encryptor) => {
                decrypt_roots(root, except, |name, wire| {
                    let plaintext = zeroize::Zeroizing::new(
                        encryptor.decrypt_with_context(wire, COOKIE_DOMAIN)?,
                    );
                    Ok(serde_json::from_slice::<Payload>(&plaintext)
                        .ok()
                        .filter(|payload| payload.version == 1 && payload.name == name)
                        .map(|payload| payload.value))
                });
            }
            #[cfg(feature = "encryption")]
            Self::Laravel(encrypter) => {
                decrypt_roots(root, except, |name, wire| {
                    Ok(encrypter
                        .decrypt_cookie_value(name, wire)?
                        .and_then(|bytes| {
                            let bytes = zeroize::Zeroizing::new(bytes);
                            std::str::from_utf8(&bytes).ok().map(str::to_owned)
                        }))
                });
            }
        }
    }
    // 无 encryption 的显式明文分支仍保留同一 fallible 转换契约，避免按 feature 分裂调用链。
    #[cfg_attr(not(feature = "encryption"), allow(clippy::unnecessary_wraps))]
    pub(super) fn encrypt(
        &self,
        cookie: &super::Cookie,
        except: &[String],
    ) -> crate::Result<super::Cookie> {
        if except.iter().any(|name| name == cookie.name()) {
            return Ok(cookie.clone());
        }
        match self {
            Self::Plain => Ok(cookie.clone()),
            #[cfg(feature = "encryption")]
            Self::Encrypted(encryptor) => {
                let value = serde_json::to_vec(&Payload {
                    version: 1,
                    name: cookie.name().to_owned(),
                    value: cookie.value().unwrap_or_default().to_owned(),
                })
                .map_err(crate::Error::internal)?;
                let encrypted = encryptor.encrypt_with_context(&value, COOKIE_DOMAIN)?;
                cookie.with_response_value(encrypted)
            }
            #[cfg(feature = "encryption")]
            Self::Laravel(encrypter) => {
                let wire = encrypter.encrypt_cookie_value(
                    cookie.name(),
                    cookie.value().unwrap_or_default().as_bytes(),
                )?;
                cookie.with_response_value(wire)
            }
        }
    }
}

#[cfg(feature = "encryption")]
fn visit_root(root: &mut Value, mut visit: impl FnMut(&str, &mut Value)) {
    match root {
        Value::Object(values) => {
            for (name, value) in values {
                visit(name, value);
            }
        }
        Value::Array(values) => {
            for (index, value) in values.iter_mut().enumerate() {
                visit(&index.to_string(), value);
            }
        }
        _ => {}
    }
}

#[cfg(feature = "encryption")]
fn decrypt_roots(
    root: &mut Value,
    except: &[String],
    decrypt: impl Fn(&str, &str) -> crate::Result<Option<String>>,
) {
    visit_root(root, |name, node| {
        if !except.iter().any(|exception| exception == name) && !decrypt_node(node, name, &decrypt)
        {
            *node = Value::Null;
        }
    });
}

// false 代表密文认证或解码失败，交根调用者废弃整个根 Cookie。
#[cfg(feature = "encryption")]
fn decrypt_node(
    node: &mut Value,
    name: &str,
    decrypt: &impl Fn(&str, &str) -> crate::Result<Option<String>>,
) -> bool {
    match node {
        Value::String(ciphertext) => {
            let Ok(plaintext) = decrypt(name, ciphertext) else {
                return false;
            };
            *node = plaintext.map_or(Value::Null, Value::String);
        }
        Value::Array(values) => {
            for (index, value) in values.iter_mut().enumerate() {
                if !decrypt_node(value, &format!("{name}[{index}]"), decrypt) {
                    return false;
                }
            }
        }
        Value::Object(values) => {
            for (key, value) in values.iter_mut() {
                if !decrypt_node(value, &format!("{name}[{key}]"), decrypt) {
                    return false;
                }
            }
        }
        _ => {}
    }
    true
}

#[cfg(all(test, feature = "encryption"))]
mod tests {
    use super::*;
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use serde_json::json;

    fn encryptor(active: &str) -> crate::Result<crate::encryption::Encrypter> {
        crate::encryption::Encrypter::new(crate::config::EncryptionConfig::new(
            active,
            [
                ("old".to_owned(), STANDARD.encode([7u8; 32])),
                ("current".to_owned(), STANDARD.encode([8u8; 32])),
            ]
            .into(),
        ))
    }
    fn ciphertext(protection: &Protection, name: &str, value: &str) -> crate::Result<String> {
        let cookie = super::super::CookieFactory::default().make(
            name,
            value,
            0,
            super::super::CookieOptions::default(),
        )?;
        Ok(protection
            .encrypt(&cookie, &[])?
            .value()
            .unwrap_or_default()
            .to_owned())
    }
    #[test]
    fn authenticated_name_mismatch_is_leaf_null_but_bad_tag_nulls_root() -> crate::Result<()> {
        let old = Protection::Encrypted(encryptor("old")?);
        let current = Protection::Encrypted(encryptor("current")?);
        let good = ciphertext(&old, "preferences[a]", "semi;plus+雪")?;
        let replay = ciphertext(&old, "different", "hidden")?;
        let mut tree = json!({"preferences":{"a":good,"b":replay}});
        current.decrypt(&mut tree, &[]);
        assert_eq!(tree, json!({"preferences":{"a":"semi;plus+雪","b":null}}));
        let mut tree =
            json!({"preferences":{"a":ciphertext(&old,"preferences[a]","ok")?,"b":"or1.invalid"}});
        current.decrypt(&mut tree, &[]);
        assert_eq!(tree, json!({"preferences":null}));
        assert!(ciphertext(&current, "preferences", "new")?.starts_with("or1.current."));
        Ok(())
    }
    #[tokio::test]
    async fn laravel_mode_uses_history_and_distinguishes_prefix_miss_from_bad_ciphertext()
    -> crate::Result<()> {
        use crate::encryption::{EncryptionServiceProvider, LaravelEncrypter};
        use crate::foundation::{ProviderBuilder, ProviderRuntime};
        use figment::{Figment, providers::Serialized};

        async fn start(key: &str, previous_keys: Vec<String>) -> crate::Result<ProviderRuntime> {
            let mut builder = ProviderBuilder::new();
            builder.provider(EncryptionServiceProvider);
            let sources = Figment::from(Serialized::defaults(json!({
                "app": {"key": key, "previous_keys": previous_keys},
            })));
            builder.start(sources).await
        }

        let previous_key = "b".repeat(32);
        let current = start(&"a".repeat(32), vec![previous_key.clone()]).await?;
        let previous = start(&previous_key, Vec::new()).await?;
        let old_protection =
            Protection::Laravel(previous.container().resolve::<LaravelEncrypter>().await?);
        let protection =
            Protection::Laravel(current.container().resolve::<LaravelEncrypter>().await?);
        let valid = ciphertext(&old_protection, "group[a]", "valid")?;
        let foreign = ciphertext(&old_protection, "other", "hidden")?;
        let mut tree = json!({"group":{"a":valid,"b":foreign},"clear":"unchanged"});
        protection.decrypt(&mut tree, &["clear".to_owned()]);
        assert_eq!(
            tree,
            json!({"group":{"a":"valid","b":null},"clear":"unchanged"})
        );
        let valid = ciphertext(&old_protection, "group[a]", "valid")?;
        let mut tree = json!({"group":{"a":valid,"b":"bad ciphertext"}});
        protection.decrypt(&mut tree, &[]);
        assert_eq!(tree, json!({"group":null}));
        for runtime in [current, previous] {
            runtime
                .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(2))
                .await?;
        }
        Ok(())
    }

    #[test]
    fn except_and_authenticated_invalid_payload_preserve_source_classification() -> crate::Result<()>
    {
        let encryptor = encryptor("current")?;
        let invalid = encryptor.encrypt_with_context(b"not a payload", COOKIE_DOMAIN)?;
        let protection = Protection::Encrypted(encryptor);
        let mut tree = json!({"group":{"a":invalid,"b":ciphertext(&protection,"group[b]","ok")?},"plain":"clear"});
        protection.decrypt(&mut tree, &["plain".to_owned()]);
        assert_eq!(tree, json!({"group":{"a":null,"b":"ok"},"plain":"clear"}));
        Ok(())
    }
}
