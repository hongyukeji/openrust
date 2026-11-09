use super::*;

fn key(value: u8) -> String {
    STANDARD.encode([value; KEY_BYTES])
}

fn config(active: &str, keys: &[(&str, u8)]) -> EncryptionConfig {
    EncryptionConfig::new(
        active,
        keys.iter()
            .map(|(key_id, value)| ((*key_id).to_owned(), key(*value)))
            .collect(),
    )
}

#[test]
fn encrypts_binary_and_empty_plaintext_and_generates_fresh_nonces() -> Result<()> {
    let encryptor = ContextualEncrypter::new(config("current", &[("current", 7)]))?;
    for plaintext in [&b"\0OpenRust\xff"[..], &b""[..]] {
        let first = encryptor.encrypt(plaintext)?;
        let second = encryptor.encrypt(plaintext)?;
        assert_ne!(first, second);
        assert_eq!(encryptor.decrypt(&first)?, plaintext);
    }
    Ok(())
}

#[test]
fn context_is_authenticated_and_must_come_from_the_caller() -> Result<()> {
    let encryptor = ContextualEncrypter::new(config("current", &[("current", 7)]))?;
    let ciphertext = encryptor.encrypt_with_context(b"pii", b"users.email:42")?;
    assert_eq!(
        encryptor.decrypt_with_context(&ciphertext, b"users.email:42")?,
        b"pii"
    );
    assert_eq!(
        encryptor
            .decrypt_with_context(&ciphertext, b"users.email:43")
            .err()
            .map(|error| error.code().to_owned()),
        Some("security_decryption_failed".to_owned())
    );
    Ok(())
}

#[test]
fn key_rotation_writes_only_with_active_key_and_reads_old_ciphertexts() -> Result<()> {
    let previous = ContextualEncrypter::new(config("old", &[("old", 3)]))?;
    let legacy = previous.encrypt(b"existing value")?;
    let all_keys = ContextualEncrypter::new(config("old", &[("old", 3), ("new", 9)]))?;
    let rotated = ContextualEncrypter::new(config("new", &[("old", 3), ("new", 9)]))?;
    assert_eq!(all_keys.decrypt(&legacy)?, b"existing value");
    assert_eq!(rotated.decrypt(&legacy)?, b"existing value");
    let current = rotated.encrypt(b"new value")?;
    assert!(current.starts_with("or1.new."));
    assert_eq!(rotated.decrypt(&current)?, b"new value");
    assert_eq!(
        ContextualEncrypter::new(config("new", &[("new", 9)]))?
            .decrypt(&legacy)
            .err()
            .map(|e| e.code().to_owned()),
        Some("security_unknown_key_id".to_owned())
    );
    Ok(())
}

#[test]
fn malformed_or_tampered_envelopes_fail_without_echoing_input() -> Result<()> {
    let encryptor = ContextualEncrypter::new(config("current", &[("current", 7)]))?;
    let ciphertext = encryptor.encrypt(b"secret plaintext")?;
    for malformed in [
        "or1.current.bad.bad",
        "or1.current.AA.bad",
        "or1.current.AAAAAAAAAAAAAAAA.AA",
        "or1.current.AAAAAAAAAAAAAAAA.AA.extra",
        "or9.current.AAAAAAAAAAAAAAAA.AAAAAAAAAAAAAAAAAAAAAA",
    ] {
        assert!(encryptor.decrypt(malformed).is_err());
    }
    let mut fields = ciphertext.split('.').map(str::to_owned).collect::<Vec<_>>();
    fields[3].push('A');
    let tampered = fields.join(".");
    let error = encryptor
        .decrypt(&tampered)
        .err()
        .ok_or_else(|| Error::custom("test_failure", "tampering must be rejected"))?;
    assert_eq!(error.code(), "security_decryption_failed");
    assert!(!error.to_string().contains("secret plaintext"));
    assert!(
        encryptor
            .decrypt(&format!("{ciphertext}.trailing"))
            .is_err()
    );
    Ok(())
}

#[test]
fn rejects_bad_key_rings_and_redacts_configuration_debug() {
    let invalid = [
        EncryptionConfig::new("missing", [("other".to_owned(), key(1))].into()),
        EncryptionConfig::new("bad id", [("bad id".to_owned(), key(1))].into()),
        EncryptionConfig::new(
            "current",
            [("current".to_owned(), "not-base64".into())].into(),
        ),
        EncryptionConfig::new(
            "current",
            [("current".to_owned(), STANDARD.encode([1_u8; 31]))].into(),
        ),
    ];
    for config in invalid {
        assert!(ContextualEncrypter::new(config).is_err());
    }
    let secret = key(42);
    let config = EncryptionConfig::new("current", [("current".to_owned(), secret.clone())].into());
    assert!(!format!("{config:?}").contains(&secret));
}

#[test]
fn rejects_oversized_inputs_before_encryption() -> Result<()> {
    let encryptor = ContextualEncrypter::new(config("current", &[("current", 7)]))?;
    assert_eq!(
        encryptor
            .encrypt(&vec![0_u8; MAX_PLAINTEXT_BYTES + 1])
            .err()
            .map(|e| e.code().to_owned()),
        Some("security_input_too_large".to_owned())
    );
    assert_eq!(
        encryptor
            .encrypt_with_context(b"ok", &vec![0_u8; MAX_CONTEXT_BYTES + 1])
            .err()
            .map(|e| e.code().to_owned()),
        Some("security_input_too_large".to_owned())
    );
    Ok(())
}

#[test]
fn known_aes_256_gcm_siv_vector_matches_rfc_8452() -> Result<()> {
    let mut key_bytes = [0_u8; KEY_BYTES];
    key_bytes[0] = 1;
    let cipher = Aes256GcmSiv::new_from_slice(&key_bytes).map_err(Error::internal)?;
    let mut nonce_bytes = [0_u8; NONCE_BYTES];
    nonce_bytes[0] = 3;
    let nonce = Nonce::from(nonce_bytes);
    let ciphertext = cipher
        .encrypt(&nonce, Payload { msg: b"", aad: b"" })
        .map_err(|_| Error::internal(EncryptionFailure))?;
    assert_eq!(
        URL_SAFE_NO_PAD.encode(ciphertext),
        URL_SAFE_NO_PAD.encode([
            0x07, 0xf5, 0xf4, 0x16, 0x9b, 0xbf, 0x55, 0xa8, 0x40, 0x0c, 0xd4, 0x7e, 0xa6, 0xfd,
            0x40, 0x0f
        ])
    );
    Ok(())
}

#[test]
fn independent_encryptors_with_the_same_ring_can_decrypt_each_other() -> Result<()> {
    let first = ContextualEncrypter::new(config("current", &[("current", 7)]))?;
    let second = ContextualEncrypter::new(config("current", &[("current", 7)]))?;
    let envelope = first.encrypt(b"shared application data")?;
    assert_eq!(second.decrypt(&envelope)?, b"shared application data");
    Ok(())
}

#[test]
fn reads_contextual_ciphertext_from_before_the_rename() -> Result<()> {
    // 载荷由改名前的 839f1b8a 生成，使用测试密钥。
    let wire = include_str!("fixtures/pre_rename_contextual.txt").trim();
    let reader = ContextualEncrypter::new(config("current", &[("legacy", 7), ("current", 9)]))?;
    assert_eq!(
        reader.decrypt_with_context(wire, b"record:41")?,
        b"persisted\0\xff"
    );
    assert!(reader.decrypt_with_context(wire, b"record:42").is_err());
    Ok(())
}
