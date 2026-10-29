//! 使用既有 AEAD 核心区分认证失败与已认证的跨名重放。
use serde_json::Value;

#[derive(Clone)]
pub(crate) enum Protection {
    Plain,
    #[cfg(feature = "encryption")]
    Encrypted(crate::security::Encryptor),
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
    pub(super) fn decrypt(&self, root: &mut Value, except: &[String]) {
        match self {
            Self::Plain => {
                let _ = (root, except);
            }
            #[cfg(feature = "encryption")]
            Self::Encrypted(encryptor) => {
                visit_root(root, |name, node| {
                    if !except.iter().any(|exception| exception == name)
                        && !decrypt_node(encryptor, node, name)
                    {
                        *node = Value::Null;
                    }
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

// false仅代表DecryptException类别，交根调用者把整个根cookie置null。
#[cfg(feature = "encryption")]
fn decrypt_node(encryptor: &crate::security::Encryptor, node: &mut Value, name: &str) -> bool {
    match node {
        Value::String(ciphertext) => {
            let Ok(plaintext) = encryptor.decrypt_with_context(ciphertext, COOKIE_DOMAIN) else {
                return false;
            };
            let Ok(payload) = serde_json::from_slice::<Payload>(&plaintext) else {
                *node = Value::Null;
                return true;
            };
            if payload.version != 1 {
                *node = Value::Null;
                return true;
            }
            *node = if payload.name == name {
                Value::String(payload.value)
            } else {
                Value::Null
            };
        }
        Value::Array(values) => {
            for (index, value) in values.iter_mut().enumerate() {
                if !decrypt_node(encryptor, value, &format!("{name}[{index}]")) {
                    return false;
                }
            }
        }
        Value::Object(values) => {
            for (key, value) in values {
                if !decrypt_node(encryptor, value, &format!("{name}[{key}]")) {
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

    fn encryptor(active: &str) -> crate::Result<crate::security::Encryptor> {
        crate::security::Encryptor::new(crate::config::EncryptionConfig::new(
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
