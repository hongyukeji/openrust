use std::{collections::HashSet, sync::Arc};

use ::http::{HeaderValue, header::SET_COOKIE};
use percent_encoding::{percent_decode_str, utf8_percent_encode};
use serde_json::Value;
use zeroize::Zeroizing;

use crate::{
    contracts::{container::Container as _, encryption::Encrypter, http::Middleware},
    cookie::{CookieEncryption, CookieError, CookieValuePrefix},
    http::{HttpError, HttpFuture, Next, Request, Response},
};

/// Decrypts request cookies and encrypts response cookies for the application.
pub struct EncryptCookies {
    encrypter: Option<Arc<dyn Encrypter>>,
    except: Arc<HashSet<String>>,
}

#[allow(non_snake_case, reason = "Matches Laravel's EncryptCookies API")]
impl EncryptCookies {
    pub fn new(encrypter: Arc<dyn Encrypter>) -> Self {
        Self {
            encrypter: Some(encrypter),
            except: Arc::default(),
        }
    }

    pub(crate) fn for_application(except: Arc<HashSet<String>>) -> Self {
        Self {
            encrypter: None,
            except,
        }
    }

    fn resolve_encrypter(&self) -> Result<Arc<dyn Encrypter>, HttpError> {
        if let Some(encrypter) = &self.encrypter {
            return Ok(encrypter.clone());
        }
        crate::support::facades::current_container()
            .ok_or(CookieError)?
            .get::<dyn Encrypter>(crate::container::ServiceKey::typed::<dyn Encrypter>())
            .map_err(|source| HttpError::Internal {
                source: Box::new(source),
            })
    }

    pub fn disableFor(&mut self, names: impl IntoIterator<Item = impl Into<String>>) {
        Arc::make_mut(&mut self.except).extend(names.into_iter().map(Into::into));
    }

    pub fn isDisabled(&self, name: &str) -> bool {
        self.except.contains(name)
    }

    fn decrypt_value(encrypter: &dyn Encrypter, name: &str, value: &mut Value) -> Result<(), ()> {
        match value {
            Value::String(ciphertext) => {
                let clear = Zeroizing::new(encrypter.decryptString(ciphertext).map_err(|_| ())?);
                *value = CookieValuePrefix::validate(name, &clear, encrypter.getAllKeys())
                    .and_then(|bytes| std::str::from_utf8(bytes).ok())
                    .map_or(Value::Null, |text| Value::String(text.to_owned()));
            }
            Value::Array(items) => {
                for (index, value) in items.iter_mut().enumerate() {
                    Self::decrypt_value(encrypter, &format!("{name}[{index}]"), value)?;
                }
            }
            Value::Object(items) => {
                for (key, value) in items {
                    Self::decrypt_value(encrypter, &format!("{name}[{key}]"), value)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn decrypt(&self, encrypter: &dyn Encrypter, cookies: &mut Value) {
        let apply = |name: &str, value: &mut Value| {
            if !self.isDisabled(name) && Self::decrypt_value(encrypter, name, value).is_err() {
                *value = Value::Null;
            }
        };
        match cookies {
            Value::Object(cookies) => {
                for (name, value) in cookies {
                    apply(name, value);
                }
            }
            Value::Array(cookies) => {
                for (index, value) in cookies.iter_mut().enumerate() {
                    apply(&index.to_string(), value);
                }
            }
            _ => {}
        }
    }

    fn encrypt(&self, encrypter: &dyn Encrypter, response: &mut Response) -> Result<(), HttpError> {
        let parts = response.parts_mut();
        let cookies = parts
            .extensions
            .remove::<crate::cookie::ResponseCookies>()
            .unwrap_or_default();
        let headers = &mut parts.headers;
        if !headers.contains_key(SET_COOKIE) {
            return Ok(());
        }
        let encrypted = headers
            .get_all(SET_COOKIE)
            .iter()
            .map(|header| {
                let cookie = cookies
                    .0
                    .iter()
                    .find(|(current, _)| current == header)
                    .map(|(_, cookie)| cookie);
                self.encrypt_header(encrypter, header, cookie)
            })
            .collect::<Result<Vec<_>, _>>()?;
        headers.remove(SET_COOKIE);
        for header in encrypted {
            headers.append(SET_COOKIE, header);
        }
        Ok(())
    }

    fn encrypt_header(
        &self,
        encrypter: &dyn Encrypter,
        header: &HeaderValue,
        cookie: Option<&crate::cookie::Cookie>,
    ) -> Result<HeaderValue, HttpError> {
        let text = header.to_str().map_err(|_| CookieError)?;
        let (pair, attributes) = text.split_once(';').unwrap_or((text, ""));
        let (name, value) = pair.split_once('=').ok_or(CookieError)?;
        let decoded_name = match cookie {
            Some(cookie) => std::borrow::Cow::Borrowed(cookie.name()),
            None => percent_decode_str(name.trim())
                .decode_utf8()
                .map_err(|_| CookieError)?,
        };
        if self.isDisabled(&decoded_name) {
            return Ok(header.clone());
        }
        let value = match cookie {
            Some(cookie) => std::borrow::Cow::Borrowed(cookie.value().unwrap_or_default()),
            None => percent_decode_str(value)
                .decode_utf8()
                .map_err(|_| CookieError)?,
        };
        let mut clear = Zeroizing::new(Vec::with_capacity(41 + value.len()));
        clear.extend_from_slice(&CookieValuePrefix::create(
            &decoded_name,
            encrypter.getKey(),
        ));
        clear.extend_from_slice(value.as_bytes());
        let ciphertext = encrypter
            .encryptString(&clear)
            .map_err(|source| HttpError::Internal {
                source: Box::new(source),
            })?;
        let ciphertext =
            utf8_percent_encode(&ciphertext, super::super::cookie_definition::URL_ENCODE);
        let wire = if attributes.is_empty() {
            format!("{name}={ciphertext}")
        } else {
            format!("{name}={ciphertext};{attributes}")
        };
        if wire.len() > 64 * 1024 {
            return Err(CookieError.into());
        }
        let mut result = HeaderValue::from_str(&wire).map_err(|_| CookieError)?;
        result.set_sensitive(true);
        Ok(result)
    }
}

impl Middleware for EncryptCookies {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
        next: Next<'a>,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            let encrypter = self.resolve_encrypter()?;
            let mut cookies = request.cookie(None)?.unwrap_or_default();
            self.decrypt(encrypter.as_ref(), &mut cookies);
            let previous = request.replace_cookies(Some(cookies));
            let previous_encryption = request.cookie_encryption.replace(CookieEncryption {
                encrypter: encrypter.clone(),
                exceptions: self.except.clone(),
            });
            let mut response = next.run(request).await;
            request.cookie_encryption = previous_encryption;
            request.replace_cookies(previous);
            self.encrypt(encrypter.as_ref(), &mut response)?;
            Ok(response)
        })
    }
}
