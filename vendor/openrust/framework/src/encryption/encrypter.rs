use std::io::Write as _;

use zeroize::Zeroizing;

use super::{Cipher, DecryptException, EncryptException, payload};

/// Encrypts application data with the current key and reads with the key ring.
pub struct Encrypter {
    cipher: Cipher,
    key: Zeroizing<Vec<u8>>,
    previous_keys: Zeroizing<Vec<Vec<u8>>>,
}

#[allow(non_snake_case, reason = "Matches Laravel's encrypter API")]
impl Encrypter {
    pub fn new(key: impl AsRef<[u8]>, cipher: Cipher) -> Result<Self, EncryptException> {
        let key = key.as_ref();
        if !Self::supported(key, cipher) {
            return Err(EncryptException::InvalidKey);
        }
        Ok(Self {
            cipher,
            key: Zeroizing::new(key.to_vec()),
            previous_keys: Zeroizing::default(),
        })
    }

    pub fn supported(key: &[u8], cipher: Cipher) -> bool {
        key.len() == cipher.key_length()
    }

    pub fn generateKey(cipher: Cipher) -> Result<Vec<u8>, EncryptException> {
        let mut key = vec![0; cipher.key_length()];
        aws_lc_rs::rand::fill(&mut key).map_err(|_| EncryptException::EncryptionFailed)?;
        Ok(key)
    }

    pub fn previousKeys(mut self, keys: Vec<Vec<u8>>) -> Result<Self, EncryptException> {
        let keys = Zeroizing::new(keys);
        if keys.iter().any(|key| !Self::supported(key, self.cipher)) {
            return Err(EncryptException::InvalidKey);
        }
        self.previous_keys = keys;
        Ok(self)
    }

    pub fn getKey(&self) -> &[u8] {
        self.key.as_slice()
    }

    pub fn getAllKeys(&self) -> Vec<&[u8]> {
        self.keys().collect()
    }

    pub fn getPreviousKeys(&self) -> &[Vec<u8>] {
        self.previous_keys.as_slice()
    }

    pub fn encrypt(&self, value: &[u8], serialize: bool) -> Result<String, EncryptException> {
        if !serialize {
            return self.encryptString(value);
        }
        let mut serialized = Zeroizing::new(Vec::with_capacity(value.len().saturating_add(27)));
        write!(serialized, "s:{}:\"", value.len()).map_err(|_| EncryptException::Serialization)?;
        serialized.extend_from_slice(value);
        serialized.extend_from_slice(b"\";");
        self.encryptString(&serialized)
    }

    pub fn decrypt(&self, encrypted: &str, unserialize: bool) -> Result<Vec<u8>, DecryptException> {
        let decrypted = self.decryptString(encrypted)?;
        if !unserialize {
            return Ok(decrypted);
        }
        let mut decrypted = Zeroizing::new(decrypted);
        let start = serialized_string_start(&decrypted).ok_or(DecryptException::Deserialization)?;
        let end = decrypted.len() - 2;
        decrypted.copy_within(start..end, 0);
        decrypted[end - start..].fill(0);
        decrypted.truncate(end - start);
        Ok(std::mem::take(&mut *decrypted))
    }

    pub fn encryptString(&self, value: &[u8]) -> Result<String, EncryptException> {
        payload::encrypt(self.cipher, &self.key, value)
    }

    pub fn decryptString(&self, encrypted: &str) -> Result<Vec<u8>, DecryptException> {
        payload::decrypt(self.cipher, self.keys(), encrypted)
    }

    pub fn appearsEncrypted(value: &str) -> bool {
        payload::appears_encrypted(value)
    }

    fn keys(&self) -> impl Iterator<Item = &[u8]> {
        std::iter::once(self.getKey()).chain(self.previous_keys.iter().map(Vec::as_slice))
    }
}

#[allow(non_snake_case, reason = "Implements Laravel's encrypter contract")]
impl crate::contracts::encryption::Encrypter for Encrypter {
    fn encrypt(&self, value: &[u8], serialize: bool) -> Result<String, EncryptException> {
        self.encrypt(value, serialize)
    }

    fn decrypt(&self, payload: &str, unserialize: bool) -> Result<Vec<u8>, DecryptException> {
        self.decrypt(payload, unserialize)
    }

    fn encryptString(&self, value: &[u8]) -> Result<String, EncryptException> {
        self.encryptString(value)
    }

    fn decryptString(&self, payload: &str) -> Result<Vec<u8>, DecryptException> {
        self.decryptString(payload)
    }

    fn getKey(&self) -> &[u8] {
        self.getKey()
    }

    fn getAllKeys(&self) -> Vec<&[u8]> {
        self.getAllKeys()
    }

    fn getPreviousKeys(&self) -> &[Vec<u8>] {
        self.getPreviousKeys()
    }
}

fn serialized_string_start(value: &[u8]) -> Option<usize> {
    let value = value.strip_prefix(b"s:")?.strip_suffix(b"\";")?;
    let separator = value.iter().position(|byte| *byte == b':')?;
    let (length, content) = value.split_at(separator);
    if length.is_empty() || !length.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let length: usize = std::str::from_utf8(length).ok()?.parse().ok()?;
    (content.strip_prefix(b":\"")?.len() == length).then_some(separator + 4)
}
