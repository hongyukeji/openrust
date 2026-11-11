use crate::{
    container::ServiceKey, contracts::container::BindingResolutionError, support::facades::Facade,
};

use super::{DecryptException, EncryptException, Encrypter};

/// Resolves the encrypter bound to the current application.
pub struct Crypt;

impl Facade for Crypt {
    type Service = Encrypter;

    fn getFacadeAccessor() -> ServiceKey {
        "encrypter".into()
    }
}

#[allow(non_snake_case, reason = "Matches Laravel's Crypt facade API")]
impl Crypt {
    pub fn encrypt(value: impl AsRef<[u8]>, serialize: bool) -> Result<String, EncryptException> {
        Self::getFacadeRoot()
            .map_err(encrypter_error)?
            .encrypt(value.as_ref(), serialize)
    }

    pub fn decrypt(payload: &str, unserialize: bool) -> Result<Vec<u8>, DecryptException> {
        Self::getFacadeRoot()
            .map_err(DecryptException::Unavailable)?
            .decrypt(payload, unserialize)
    }

    pub fn encryptString(value: impl AsRef<[u8]>) -> Result<String, EncryptException> {
        Self::encrypt(value, false)
    }

    pub fn decryptString(payload: &str) -> Result<Vec<u8>, DecryptException> {
        Self::decrypt(payload, false)
    }

    pub fn appearsEncrypted(payload: &str) -> bool {
        Encrypter::appearsEncrypted(payload)
    }
}

fn encrypter_error(error: BindingResolutionError) -> EncryptException {
    match error {
        BindingResolutionError::FactoryFailed {
            service: "encrypter",
            source,
        } => match source.downcast::<EncryptException>() {
            Ok(error) => *error,
            Err(source) => EncryptException::Unavailable(BindingResolutionError::FactoryFailed {
                service: "encrypter",
                source,
            }),
        },
        error => EncryptException::Unavailable(error),
    }
}
