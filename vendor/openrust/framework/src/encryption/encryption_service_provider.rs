use std::sync::Arc;

use zeroize::Zeroizing;

use crate::{
    contracts::{
        config::Repository,
        container::{BindingResolutionError, Container as _},
        foundation::Application as _,
    },
    foundation::Application,
    http::HttpFuture,
    support::{ProviderResult, ServiceProvider},
};

use super::{Cipher, EncryptException, Encrypter, payload};

/// Registers the application encrypter in the service container.
pub struct EncryptionServiceProvider;

impl ServiceProvider for EncryptionServiceProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            let container = app.container();
            container.singletonNamed("encrypter", |container| {
                let config = container.get::<dyn Repository>("config")?;
                resolve(config.as_ref()).map(Arc::new).map_err(|source| {
                    BindingResolutionError::FactoryFailed {
                        service: "encrypter",
                        source: Box::new(source),
                    }
                })
            })?;
            container.alias(
                "encrypter",
                crate::container::ServiceKey::typed::<Encrypter>(),
            )?;
            container.singleton::<dyn crate::contracts::encryption::Encrypter, _>(|container| {
                let encrypter = container.get::<Encrypter>("encrypter")?;
                Ok(encrypter)
            })?;
            Ok(())
        })
    }
}

fn resolve(config: &dyn Repository) -> Result<Encrypter, EncryptException> {
    let key = config.get("app.key").unwrap_or_default();
    let key = Zeroizing::new(match key {
        serde_json::Value::Null => return Err(EncryptException::MissingKey),
        serde_json::Value::String(key) => key,
        _ => return Err(EncryptException::InvalidKey),
    });
    let cipher = configured_cipher(config)?;
    let key = payload::decode_key(&key)?;
    let encrypter = Encrypter::new(key.as_slice(), cipher)?;
    let mut keys = Zeroizing::new(Vec::new());
    match config.get("app.previous_keys") {
        None | Some(serde_json::Value::Null) => {}
        Some(serde_json::Value::Array(previous)) => {
            for value in previous {
                let serde_json::Value::String(value) = value else {
                    return Err(EncryptException::InvalidKey);
                };
                let value = Zeroizing::new(value);
                let mut key = payload::decode_key(&value)?;
                keys.push(std::mem::take(&mut *key));
            }
        }
        _ => return Err(EncryptException::InvalidKey),
    }
    encrypter.previousKeys(std::mem::take(&mut *keys))
}

pub(crate) fn configured_cipher(config: &dyn Repository) -> Result<Cipher, EncryptException> {
    match config.get("app.cipher") {
        Some(serde_json::Value::String(cipher)) => cipher.parse(),
        None => Ok(Cipher::Aes256Cbc),
        _ => Err(EncryptException::UnsupportedCipher),
    }
}
