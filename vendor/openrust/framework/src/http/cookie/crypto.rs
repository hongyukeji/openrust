//! 使用既有 AEAD 核心区分认证失败与已认证的跨名重放。
use serde_json::Value;

#[derive(Clone)]
pub(crate) enum Protection {
    Plain,
    #[cfg(feature = "encryption")]
    Encrypted(crate::encryption::ContextualEncrypter),
    #[cfg(feature = "encryption")]
    NameBound(std::sync::Arc<dyn crate::encryption::ApplicationEncrypter>),
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
            Self::Encrypted(_) | Self::NameBound(_) => true,
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
            Self::NameBound(encrypter) => {
                decrypt_roots(root, except, |name, wire| {
                    Ok(
                        crate::encryption::decrypt_cookie_value(encrypter.as_ref(), name, wire)?
                            .and_then(|bytes| {
                                let bytes = zeroize::Zeroizing::new(bytes);
                                std::str::from_utf8(&bytes).ok().map(str::to_owned)
                            }),
                    )
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
            Self::NameBound(encrypter) => {
                let wire = crate::encryption::encrypt_cookie_value(
                    encrypter.as_ref(),
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
#[path = "../../../tests/unit/http/cookie/crypto.rs"]
mod tests;
