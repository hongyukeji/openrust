use std::fmt::Write as _;

use aws_lc_rs::{aead, cipher, hmac};
use base64::{
    Engine as _, alphabet,
    engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig, general_purpose::STANDARD},
};
use serde::{Deserialize, Serialize};
use subtle::ConstantTimeEq as _;
use zeroize::Zeroizing;

use super::{Cipher, DecryptException, EncryptException};

const AES_BLOCK: usize = 16;
const GCM_TAG: usize = 16;
const PHP_BASE64: GeneralPurpose = GeneralPurpose::new(
    &alphabet::STANDARD,
    GeneralPurposeConfig::new()
        .with_decode_allow_trailing_bits(true)
        .with_decode_padding_mode(DecodePaddingMode::Indifferent),
);
const PHP_PADDED_BASE64: GeneralPurpose = GeneralPurpose::new(
    &alphabet::STANDARD,
    GeneralPurposeConfig::new().with_decode_allow_trailing_bits(true),
);

enum Backend {
    Cbc(&'static cipher::Algorithm),
    Gcm(&'static aead::Algorithm),
}

impl From<Cipher> for Backend {
    fn from(cipher: Cipher) -> Self {
        match cipher {
            Cipher::Aes128Cbc => Self::Cbc(&cipher::AES_128),
            Cipher::Aes256Cbc => Self::Cbc(&cipher::AES_256),
            Cipher::Aes128Gcm => Self::Gcm(&aead::AES_128_GCM),
            Cipher::Aes256Gcm => Self::Gcm(&aead::AES_256_GCM),
        }
    }
}

#[derive(Serialize, Deserialize)]
struct Payload {
    iv: String,
    value: String,
    mac: String,
    tag: Option<String>,
}

pub(super) fn encrypt(
    cipher: Cipher,
    key: &[u8],
    plaintext: &[u8],
) -> Result<String, EncryptException> {
    let failed = |_| EncryptException::EncryptionFailed;
    let backend = Backend::from(cipher);
    let padding = if matches!(backend, Backend::Cbc(_)) {
        AES_BLOCK
    } else {
        0
    };
    let mut buffer = Zeroizing::new(Vec::with_capacity(plaintext.len().saturating_add(padding)));
    buffer.extend_from_slice(plaintext);
    let (iv, tag) = match backend {
        Backend::Cbc(algorithm) => {
            let cipher_key = cipher::UnboundCipherKey::new(algorithm, key).map_err(failed)?;
            let encryptor =
                cipher::PaddedBlockEncryptingKey::cbc_pkcs7(cipher_key).map_err(failed)?;
            let mut iv = [0; AES_BLOCK];
            aws_lc_rs::rand::fill(&mut iv).map_err(failed)?;
            encryptor
                .less_safe_encrypt(&mut *buffer, cipher::EncryptionContext::Iv128(iv.into()))
                .map_err(failed)?;
            (STANDARD.encode(iv), String::new())
        }
        Backend::Gcm(algorithm) => {
            let encryptor = aead::UnboundKey::new(algorithm, key)
                .map(aead::LessSafeKey::new)
                .map_err(failed)?;
            let mut iv = [0; aead::NONCE_LEN];
            aws_lc_rs::rand::fill(&mut iv).map_err(failed)?;
            let nonce = aead::Nonce::assume_unique_for_key(iv);
            let tag = encryptor
                .seal_in_place_separate_tag(nonce, aead::Aad::empty(), &mut buffer)
                .map_err(failed)?;
            (STANDARD.encode(iv), STANDARD.encode(tag.as_ref()))
        }
    };
    let value = STANDARD.encode(&*buffer);
    let mac = if tag.is_empty() {
        cbc_mac(key, &iv, &value)
    } else {
        String::new()
    };
    let payload = Payload {
        iv,
        value,
        mac,
        tag: Some(tag),
    };
    serde_json::to_vec(&payload)
        .map(|json| STANDARD.encode(json))
        .map_err(|_| EncryptException::Serialization)
}

pub(super) fn decrypt<'a>(
    cipher: Cipher,
    keys: impl Iterator<Item = &'a [u8]>,
    encrypted: &str,
) -> Result<Vec<u8>, DecryptException> {
    let invalid = DecryptException::InvalidPayload;
    let json = decode_php(encrypted).ok_or(invalid)?;
    let payload: Payload =
        serde_json::from_slice(&json).map_err(|_| DecryptException::InvalidPayload)?;
    let iv = decode_strict(&payload.iv).ok_or(DecryptException::InvalidPayload)?;
    let ciphertext = decode_php(&payload.value).ok_or(DecryptException::InvalidPayload)?;
    match Backend::from(cipher) {
        Backend::Cbc(algorithm) => decrypt_cbc(algorithm, keys, &iv, payload, ciphertext),
        Backend::Gcm(algorithm) => {
            decrypt_gcm(algorithm, keys, &iv, payload.tag.as_deref(), &ciphertext)
        }
    }
}

fn decrypt_cbc<'a>(
    algorithm: &'static cipher::Algorithm,
    keys: impl Iterator<Item = &'a [u8]>,
    iv: &[u8],
    payload: Payload,
    ciphertext: Vec<u8>,
) -> Result<Vec<u8>, DecryptException> {
    let context = cipher::DecryptionContext::Iv128(
        iv.try_into()
            .map_err(|_| DecryptException::InvalidPayload)?,
    );
    if !matches!(payload.tag.as_deref(), None | Some("" | "0")) {
        return Err(DecryptException::InvalidPayload);
    }
    let failed = |_| DecryptException::AuthenticationFailed;
    let mut selected = None;
    for key in keys {
        let valid = cbc_mac(key, &payload.iv, &payload.value)
            .as_bytes()
            .ct_eq(payload.mac.as_bytes());
        if bool::from(valid) && selected.is_none() {
            selected = Some(key);
        }
    }
    let key = selected.ok_or(DecryptException::AuthenticationFailed)?;
    let key = cipher::UnboundCipherKey::new(algorithm, key).map_err(failed)?;
    let decryptor = cipher::PaddedBlockDecryptingKey::cbc_pkcs7(key).map_err(failed)?;
    let mut buffer = Zeroizing::new(ciphertext);
    let plaintext = decryptor.decrypt(&mut buffer, context).map_err(failed)?;
    Ok(plaintext.to_vec())
}

fn decrypt_gcm<'a>(
    algorithm: &'static aead::Algorithm,
    keys: impl Iterator<Item = &'a [u8]>,
    iv: &[u8],
    tag: Option<&str>,
    ciphertext: &[u8],
) -> Result<Vec<u8>, DecryptException> {
    if iv.len() != algorithm.nonce_len() {
        return Err(DecryptException::InvalidPayload);
    }
    let tag = tag
        .and_then(decode_php)
        .filter(|tag| tag.len() == GCM_TAG)
        .ok_or(DecryptException::AuthenticationFailed)?;
    let mut buffer = Zeroizing::new(ciphertext.to_vec());
    for key in keys {
        let nonce = aead::Nonce::try_assume_unique_for_key(iv)
            .map_err(|_| DecryptException::InvalidPayload)?;
        let decryptor = aead::UnboundKey::new(algorithm, key)
            .map(aead::LessSafeKey::new)
            .map_err(|_| DecryptException::AuthenticationFailed)?;
        buffer.copy_from_slice(ciphertext);
        if let Ok(plaintext) =
            decryptor.open_in_place_separate_tag(nonce, aead::Aad::empty(), &tag, &mut buffer)
        {
            return Ok(plaintext.to_vec());
        }
    }
    Err(DecryptException::AuthenticationFailed)
}

pub(super) fn decode_key(encoded: &str) -> Result<Zeroizing<Vec<u8>>, EncryptException> {
    if encoded.is_empty() {
        return Err(EncryptException::MissingKey);
    }
    if let Some(encoded) = encoded.strip_prefix("base64:") {
        return decode_php(encoded)
            .map(Zeroizing::new)
            .ok_or(EncryptException::InvalidKey);
    }
    Ok(Zeroizing::new(encoded.as_bytes().to_vec()))
}

fn cbc_mac(key: &[u8], iv: &str, value: &str) -> String {
    let key = hmac::Key::new(hmac::HMAC_SHA256, key);
    let mut context = hmac::Context::with_key(&key);
    context.update(iv.as_bytes());
    context.update(value.as_bytes());
    let mut mac = String::with_capacity(64);
    for byte in context.sign().as_ref() {
        let _ = write!(mac, "{byte:02x}");
    }
    mac
}

fn decode_php(value: &str) -> Option<Vec<u8>> {
    let mut bytes = Zeroizing::new(Vec::with_capacity(value.len()));
    bytes.extend(
        value
            .bytes()
            .filter(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/')),
    );
    if bytes.len() % 4 == 1 {
        bytes.pop();
    }
    PHP_BASE64.decode(bytes.as_slice()).ok()
}

fn decode_strict(value: &str) -> Option<Vec<u8>> {
    let bytes: Vec<_> = value
        .bytes()
        .filter(|b| !matches!(b, b' ' | b'\t' | b'\n' | b'\r'))
        .collect();
    if bytes.contains(&b'=') {
        PHP_PADDED_BASE64.decode(bytes).ok()
    } else {
        PHP_BASE64.decode(bytes).ok()
    }
}

pub(super) fn appears_encrypted(value: &str) -> bool {
    let Some(json) = decode_strict(value) else {
        return false;
    };
    let Ok(serde_json::Value::Object(fields)) = serde_json::from_slice(&json) else {
        return false;
    };
    ["iv", "value", "mac"]
        .iter()
        .all(|key| fields.get(*key).is_some_and(|value| !value.is_null()))
}
