//! Laravel encryptString/decryptString 的载荷格式及认证。由应用 Encrypter 接线层选择密钥。

use std::collections::BTreeMap;
use std::fmt::Write as _;

use aws_lc_rs::{aead, cipher, hmac};
use base64::{
    Engine as _, alphabet,
    engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig, general_purpose::STANDARD},
};
use serde::Serialize;
use serde_json::value::RawValue;
use subtle::ConstantTimeEq as _;
use zeroize::Zeroizing;

use crate::config::app::AppCipher;
use crate::{Error, Result};

pub(super) const MAX_PLAINTEXT_BYTES: usize = 1024 * 1024;
const MAX_PAYLOAD_BYTES: usize = 2 * 1024 * 1024;
const AES_BLOCK_BYTES: usize = 16;
const GCM_TAG_BYTES: usize = 16;
// PHP json_decode 的默认深度为 512，实际接受的最深容器层数为 511（含根对象）。
const MAX_JSON_CONTAINER_DEPTH: usize = 511;
const LARAVEL_BASE64: GeneralPurpose = GeneralPurpose::new(
    &alphabet::STANDARD,
    GeneralPurposeConfig::new()
        .with_decode_allow_trailing_bits(true)
        .with_decode_padding_mode(DecodePaddingMode::RequireNone),
);
const PADDED_BASE64: GeneralPurpose = GeneralPurpose::new(
    &alphabet::STANDARD,
    GeneralPurposeConfig::new().with_decode_allow_trailing_bits(true),
);

enum Backend {
    Cbc(&'static cipher::Algorithm),
    Gcm(&'static aead::Algorithm),
}

impl Backend {
    fn select(cipher: AppCipher) -> Self {
        match cipher {
            AppCipher::Aes128Cbc => Self::Cbc(&cipher::AES_128),
            AppCipher::Aes256Cbc => Self::Cbc(&cipher::AES_256),
            AppCipher::Aes128Gcm => Self::Gcm(&aead::AES_128_GCM),
            AppCipher::Aes256Gcm => Self::Gcm(&aead::AES_256_GCM),
        }
    }
}

#[derive(Serialize)]
struct Payload {
    iv: String,
    value: String,
    mac: String,
    tag: String,
}

pub(super) fn encrypt_string(cipher: AppCipher, key: &[u8], plaintext: &[u8]) -> Result<String> {
    if plaintext.len() > MAX_PLAINTEXT_BYTES {
        return Err(encrypt_failed());
    }
    let backend = Backend::select(cipher);
    // PKCS#7 可能追加整块填充。先分配容量再复制明文，避免扩容释放旧明文缓冲区。
    let padding = if matches!(backend, Backend::Cbc(_)) {
        AES_BLOCK_BYTES
    } else {
        0
    };
    let mut buffer = Zeroizing::new(Vec::with_capacity(plaintext.len() + padding));
    buffer.extend_from_slice(plaintext);
    let (iv, tag) = match backend {
        Backend::Cbc(algorithm) => {
            let key =
                cipher::UnboundCipherKey::new(algorithm, key).map_err(|_| encrypt_failed())?;
            let encryptor =
                cipher::PaddedBlockEncryptingKey::cbc_pkcs7(key).map_err(|_| encrypt_failed())?;
            let iv = random_bytes::<AES_BLOCK_BYTES>()?;
            let context = cipher::EncryptionContext::Iv128(iv.into());
            encryptor
                .less_safe_encrypt(&mut *buffer, context)
                .map_err(|_| encrypt_failed())?;
            (STANDARD.encode(iv), String::new())
        }
        Backend::Gcm(algorithm) => {
            let encryptor = aead::UnboundKey::new(algorithm, key)
                .map(aead::LessSafeKey::new)
                .map_err(|_| encrypt_failed())?;
            let iv = random_bytes::<{ aead::NONCE_LEN }>()?;
            let nonce = aead::Nonce::assume_unique_for_key(iv);
            let tag = encryptor
                .seal_in_place_separate_tag(nonce, aead::Aad::empty(), &mut buffer)
                .map_err(|_| encrypt_failed())?;
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
        tag,
    };
    serde_json::to_vec(&payload)
        .map(|json| STANDARD.encode(json))
        .map_err(|_| encrypt_failed())
}

/// 密钥按当前、历史顺序传入，接线层须先验证每把密钥符合所选算法。
pub(super) fn decrypt_string(cipher: AppCipher, keys: &[&[u8]], payload: &str) -> Result<Vec<u8>> {
    if payload.len() > MAX_PAYLOAD_BYTES {
        return Err(invalid_payload());
    }
    let json = decode_php_base64(payload, false).ok_or_else(invalid_payload)?;
    let payload = parse_payload(&json)?;
    let iv = decode_php_base64(&payload.iv, true).ok_or_else(invalid_payload)?;
    let ciphertext = decode_php_base64(&payload.value, false).ok_or_else(decrypt_failed)?;
    if ciphertext.len() > MAX_PLAINTEXT_BYTES + AES_BLOCK_BYTES {
        return Err(invalid_payload());
    }
    match Backend::select(cipher) {
        Backend::Cbc(algorithm) => decrypt_cbc(
            algorithm,
            keys,
            &iv,
            &payload.iv,
            &payload.value,
            &payload.mac,
            &payload.tag,
            ciphertext,
        ),
        Backend::Gcm(algorithm) => decrypt_gcm(algorithm, keys, &iv, &payload.tag, &ciphertext),
    }
}

fn parse_payload(json: &[u8]) -> Result<Payload> {
    let text = std::str::from_utf8(json).map_err(|_| invalid_payload())?;
    validate_json_strings_and_depth(text)?;
    // RawValue 以迭代方式跳过未知嵌套值；映射保留 JSON 中同名字段的末值。
    let fields: BTreeMap<String, &RawValue> =
        serde_json::from_str(text).map_err(|_| invalid_payload())?;
    let string = |name: &str| -> Result<String> {
        let value = fields.get(name).ok_or_else(invalid_payload)?;
        serde_json::from_str(value.get()).map_err(|_| invalid_payload())
    };
    let tag = match fields.get("tag") {
        None => String::new(),
        Some(value) if value.get() == "null" => String::new(),
        Some(_) => string("tag")?,
    };
    Ok(Payload {
        iv: string("iv")?,
        value: string("value")?,
        mac: string("mac")?,
        tag,
    })
}

fn validate_json_strings_and_depth(text: &str) -> Result<()> {
    let mut depth = 0_usize;
    let mut string_start = None;
    let mut escaped = false;
    let mut needs_string_validation = false;
    for (index, byte) in text.bytes().enumerate() {
        if let Some(start) = string_start {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
                needs_string_validation = true;
            } else if byte == b'"' {
                // RawValue 的忽略路径不验证 UTF-16 代理项，逐个验证含转义的字符串。
                if needs_string_validation {
                    serde_json::from_str::<String>(&text[start..=index])
                        .map_err(|_| invalid_payload())?;
                }
                string_start = None;
            }
        } else {
            match byte {
                b'"' => {
                    string_start = Some(index);
                    needs_string_validation = false;
                }
                b'[' | b'{' => {
                    depth += 1;
                    if depth > MAX_JSON_CONTAINER_DEPTH {
                        return Err(invalid_payload());
                    }
                }
                b']' | b'}' => depth = depth.checked_sub(1).ok_or_else(invalid_payload)?,
                _ => {}
            }
        }
    }
    Ok(())
}

#[allow(
    clippy::too_many_arguments,
    reason = "CBC 认证使用原始编码字段，解密使用已解码字段"
)]
fn decrypt_cbc(
    algorithm: &'static cipher::Algorithm,
    keys: &[&[u8]],
    iv: &[u8],
    iv_text: &str,
    value: &str,
    mac: &str,
    tag: &str,
    ciphertext: Vec<u8>,
) -> Result<Vec<u8>> {
    let context = cipher::DecryptionContext::Iv128(iv.try_into().map_err(|_| invalid_payload())?);
    if !matches!(tag, "" | "0") {
        return Err(Error::custom(
            "laravel_payload_tag_for_cbc",
            "CBC 载荷不能包含 AEAD tag",
        ));
    }
    let mut selected = None;
    for &key in keys {
        let matches = bool::from(
            cbc_mac(key, iv_text, value)
                .as_bytes()
                .ct_eq(mac.as_bytes()),
        );
        if matches && selected.is_none() {
            selected = Some(key);
        }
    }
    let key = selected
        .ok_or_else(|| Error::custom("laravel_payload_mac_invalid", "加密载荷 MAC 无效"))?;
    let key = cipher::UnboundCipherKey::new(algorithm, key).map_err(|_| decrypt_failed())?;
    let decryptor =
        cipher::PaddedBlockDecryptingKey::cbc_pkcs7(key).map_err(|_| decrypt_failed())?;
    let mut buffer = Zeroizing::new(ciphertext);
    let plain = decryptor
        .decrypt(&mut buffer, context)
        .map_err(|_| decrypt_failed())?;
    copy_bounded_plaintext(plain)
}

fn decrypt_gcm(
    algorithm: &'static aead::Algorithm,
    keys: &[&[u8]],
    iv: &[u8],
    tag: &str,
    ciphertext: &[u8],
) -> Result<Vec<u8>> {
    if iv.len() != algorithm.nonce_len() {
        return Err(invalid_payload());
    }
    let tag = decode_php_base64(tag, false)
        .filter(|tag| tag.len() == GCM_TAG_BYTES)
        .ok_or_else(decrypt_failed)?;
    for &key in keys {
        let nonce = aead::Nonce::try_assume_unique_for_key(iv).map_err(|_| invalid_payload())?;
        let decryptor = aead::UnboundKey::new(algorithm, key)
            .map(aead::LessSafeKey::new)
            .map_err(|_| decrypt_failed())?;
        let mut buffer = Zeroizing::new(ciphertext.to_vec());
        if let Ok(plain) =
            decryptor.open_in_place_separate_tag(nonce, aead::Aad::empty(), &tag, &mut buffer)
        {
            return copy_bounded_plaintext(plain);
        }
    }
    Err(decrypt_failed())
}

// 仅在仍借用受控缓冲区时检查限额；超限时不创建普通明文副本。
fn copy_bounded_plaintext(plain: &[u8]) -> Result<Vec<u8>> {
    if plain.len() > MAX_PLAINTEXT_BYTES {
        return Err(invalid_payload());
    }
    Ok(plain.to_vec())
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

fn decode_php_base64(text: &str, strict: bool) -> Option<Vec<u8>> {
    if strict {
        let bytes: Vec<_> = text
            .bytes()
            .filter(|b| !matches!(b, b' ' | b'\t' | b'\n' | b'\r'))
            .collect();
        if bytes.contains(&b'=') {
            return PADDED_BASE64.decode(bytes).ok();
        }
        return LARAVEL_BASE64.decode(bytes).ok();
    }
    LARAVEL_BASE64
        .decode(non_strict_base64_input(text).as_slice())
        .ok()
}

pub(super) fn decode_php_base64_key(text: &str) -> Option<Zeroizing<Vec<u8>>> {
    let input = non_strict_base64_input(text);
    let mut output = Zeroizing::new(vec![0; base64::decoded_len_estimate(input.len())]);
    let length = LARAVEL_BASE64
        .decode_slice(input.as_slice(), &mut output)
        .ok()?;
    output.truncate(length);
    Some(output)
}

fn non_strict_base64_input(text: &str) -> Zeroizing<Vec<u8>> {
    let mut bytes = Zeroizing::new(Vec::with_capacity(text.len()));
    bytes.extend(
        text.bytes()
            .filter(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/')),
    );
    if bytes.len() % 4 == 1 {
        bytes.pop();
    }
    bytes
}

fn random_bytes<const N: usize>() -> Result<[u8; N]> {
    let mut bytes = [0; N];
    getrandom::fill(&mut bytes).map_err(|_| encrypt_failed())?;
    Ok(bytes)
}

fn encrypt_failed() -> Error {
    Error::custom(
        "laravel_payload_encrypt_failed",
        "无法加密 Laravel 字符串载荷",
    )
}

fn invalid_payload() -> Error {
    Error::custom("laravel_payload_invalid", "Laravel 加密载荷格式无效")
}

fn decrypt_failed() -> Error {
    Error::custom(
        "laravel_payload_decrypt_failed",
        "无法解密 Laravel 字符串载荷",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    const PROFILES: [(AppCipher, usize, bool); 4] = [
        (AppCipher::Aes128Cbc, 16, false),
        (AppCipher::Aes256Cbc, 32, false),
        (AppCipher::Aes128Gcm, 16, true),
        (AppCipher::Aes256Gcm, 32, true),
    ];

    fn unpack(payload: &str) -> Result<Value> {
        let json = STANDARD.decode(payload).map_err(|_| invalid_payload())?;
        serde_json::from_slice(&json).map_err(|_| invalid_payload())
    }

    fn pack(payload: &Value) -> Result<String> {
        serde_json::to_vec(payload)
            .map(|json| STANDARD.encode(json))
            .map_err(|_| invalid_payload())
    }

    #[test]
    fn wire_fields_and_rotated_keys_round_trip_in_all_profiles() -> Result<()> {
        for (name, key_size, gcm) in PROFILES {
            let current = vec![1; key_size];
            let previous = vec![2; key_size];
            for bytes in [b"".as_slice(), b"\0\xffopenrust\r\n".as_slice()] {
                let payload = encrypt_string(name, &previous, bytes)?;
                let fields = unpack(&payload)?;
                let iv = fields["iv"].as_str().ok_or_else(invalid_payload)?;
                let tag = fields["tag"].as_str().ok_or_else(invalid_payload)?;
                let mac = fields["mac"].as_str().ok_or_else(invalid_payload)?;
                assert_eq!(
                    STANDARD.decode(iv).map_err(|_| invalid_payload())?.len(),
                    if gcm { 12 } else { 16 }
                );
                if gcm {
                    assert_eq!(
                        STANDARD.decode(tag).map_err(|_| invalid_payload())?.len(),
                        16
                    );
                    assert_eq!(mac, "");
                } else {
                    assert_eq!(tag, "");
                    assert_eq!(mac.len(), 64);
                }
                assert!(decrypt_string(name, &[&current], &payload).is_err());
                assert_eq!(
                    decrypt_string(name, &[&current, &previous], &payload)?,
                    bytes
                );
            }
        }
        Ok(())
    }

    #[test]
    fn authenticated_plaintext_over_the_limit_is_rejected_before_delivery() -> Result<()> {
        let key = [3; 32];
        let mut plaintext = Zeroizing::new(vec![7; MAX_PLAINTEXT_BYTES + 1]);
        let gcm = aead::UnboundKey::new(&aead::AES_256_GCM, &key)
            .map(aead::LessSafeKey::new)
            .map_err(|_| encrypt_failed())?;
        let iv = [1; aead::NONCE_LEN];
        let tag = gcm
            .seal_in_place_separate_tag(
                aead::Nonce::assume_unique_for_key(iv),
                aead::Aad::empty(),
                &mut plaintext,
            )
            .map_err(|_| encrypt_failed())?;
        let tag = STANDARD.encode(tag.as_ref());
        assert!(decrypt_gcm(&aead::AES_256_GCM, &[&key], &iv, &tag, &plaintext).is_err());

        let mut plaintext = Zeroizing::new(Vec::with_capacity(
            MAX_PLAINTEXT_BYTES + 1 + AES_BLOCK_BYTES,
        ));
        plaintext.resize(MAX_PLAINTEXT_BYTES + 1, 7);
        let cbc =
            cipher::UnboundCipherKey::new(&cipher::AES_256, &key).map_err(|_| encrypt_failed())?;
        let cbc = cipher::PaddedBlockEncryptingKey::cbc_pkcs7(cbc).map_err(|_| encrypt_failed())?;
        let iv = [2; AES_BLOCK_BYTES];
        cbc.less_safe_encrypt(&mut *plaintext, cipher::EncryptionContext::Iv128(iv.into()))
            .map_err(|_| encrypt_failed())?;
        let iv_text = STANDARD.encode(iv);
        let value = STANDARD.encode(&*plaintext);
        let mac = cbc_mac(&key, &iv_text, &value);
        assert!(
            decrypt_cbc(
                &cipher::AES_256,
                &[&key],
                &iv,
                &iv_text,
                &value,
                &mac,
                "",
                plaintext.to_vec(),
            )
            .is_err()
        );
        Ok(())
    }

    #[test]
    fn json_wire_applies_php_depth_last_duplicate_and_unicode_rules() -> Result<()> {
        for (cipher, key_bytes, _) in PROFILES {
            let key = vec![3; key_bytes];
            let wire = encrypt_string(cipher, &key, b"payload")?;
            let decoded = STANDARD.decode(wire).map_err(|_| invalid_payload())?;
            let json = std::str::from_utf8(&decoded).map_err(|_| invalid_payload())?;
            let prefix = json.strip_suffix('}').ok_or_else(invalid_payload)?;
            for depth in [130, 510, 511] {
                let extra = format!("{}null{}", "[".repeat(depth), "]".repeat(depth));
                let wire = STANDARD.encode(format!(r#"{prefix},"extra":{extra}}}"#));
                let result = decrypt_string(cipher, &[&key], &wire);
                if depth <= 510 {
                    assert_eq!(result?, b"payload");
                } else {
                    assert!(result.is_err());
                }
            }
            let tail = json.strip_prefix('{').ok_or_else(invalid_payload)?;
            let wire = STANDARD.encode(format!(r#"{{"value":123,{tail}"#));
            assert_eq!(decrypt_string(cipher, &[&key], &wire)?, b"payload");
            let wire = STANDARD.encode(format!(r#"{prefix},"value":123}}"#));
            assert!(decrypt_string(cipher, &[&key], &wire).is_err());
            let wire = STANDARD.encode(format!(r#"{prefix},"extra":"\uD800"}}"#));
            assert!(decrypt_string(cipher, &[&key], &wire).is_err());
            let wire = STANDARD.encode(format!(r#"{prefix},"extra":"\uD83D\uDE00 [{{}}"}}"#));
            assert_eq!(decrypt_string(cipher, &[&key], &wire)?, b"payload");
        }
        Ok(())
    }

    #[test]
    fn authentication_uses_the_fields_required_by_each_profile() -> Result<()> {
        for (name, key_size, gcm) in PROFILES {
            let key = vec![3; key_size];
            let payload = encrypt_string(name, &key, b"data")?;
            let mut fields = unpack(&payload)?;
            fields["value"] = Value::String("AA==".to_owned());
            assert!(decrypt_string(name, &[&key], &pack(&fields)?).is_err());

            let mut fields = unpack(&payload)?;
            fields["mac"] = Value::String("modified".to_owned());
            let result = decrypt_string(name, &[&key], &pack(&fields)?);
            if gcm {
                assert_eq!(result?, b"data");
            } else {
                assert!(result.is_err());
            }

            let mut fields = unpack(&payload)?;
            fields["tag"] = Value::String("AA==".to_owned());
            assert!(decrypt_string(name, &[&key], &pack(&fields)?).is_err());
        }
        Ok(())
    }
}
