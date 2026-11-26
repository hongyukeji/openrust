//! Application encryption contract.

use crate::encryption::{DecryptException, EncryptException};

/// Encrypts and decrypts byte strings, with optional PHP string serialization.
pub trait Encrypter: Send + Sync {
    fn encrypt(&self, value: &[u8], serialize: bool) -> Result<String, EncryptException>;
    fn decrypt(&self, payload: &str, unserialize: bool) -> Result<Vec<u8>, DecryptException>;
    fn encrypt_string(&self, value: &[u8]) -> Result<String, EncryptException>;
    fn decrypt_string(&self, payload: &str) -> Result<Vec<u8>, DecryptException>;
    fn get_key(&self) -> &[u8];
    fn get_all_keys(&self) -> Vec<&[u8]>;
    fn get_previous_keys(&self) -> &[Vec<u8>];
}
