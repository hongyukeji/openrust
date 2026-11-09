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
