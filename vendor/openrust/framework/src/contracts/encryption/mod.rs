//! Application encryption contract.

#![allow(non_snake_case)]

use crate::encryption::{DecryptException, EncryptException};

/// Encrypts and decrypts byte strings, with optional PHP string serialization.
pub trait Encrypter: Send + Sync {
    fn encrypt(&self, value: &[u8], serialize: bool) -> Result<String, EncryptException>;
    fn decrypt(&self, payload: &str, unserialize: bool) -> Result<Vec<u8>, DecryptException>;
    fn encryptString(&self, value: &[u8]) -> Result<String, EncryptException>;
    fn decryptString(&self, payload: &str) -> Result<Vec<u8>, DecryptException>;
    fn getKey(&self) -> &[u8];
    fn getAllKeys(&self) -> Vec<&[u8]>;
    fn getPreviousKeys(&self) -> &[Vec<u8>];
}
