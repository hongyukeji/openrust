use super::*;

#[test]
fn cookie_prefix_binds_nested_name_and_accepts_current_or_historical_key() -> Result<()> {
    let name = "profile[options][theme]";
    let current_key = [0x15; 32];
    let previous_key = [0x42; 32];
    assert_eq!(
        &cookie_prefix(name, &current_key),
        b"4a5e95c703d0df51557444ad49ee7007a5ef263c|"
    );
    assert_eq!(
        &cookie_prefix(name, &previous_key),
        b"09aa735b3acbd428754ffda3615cfcc134dd8cd3|"
    );
    let make = |key: [u8; 32], previous_keys| {
        let key = String::from_utf8(key.to_vec()).map_err(Error::internal)?;
        ConfiguredEncrypter::from_config(AppConfig {
            key: Some(key),
            previous_keys,
            ..AppConfig::default()
        })
    };
    let current = make(current_key, vec!["B".repeat(32)])?;
    let previous = make(previous_key, Vec::new())?;
    let value = b"dark\0mode";
    for writer in [&current, &previous] {
        let wire = encrypt_cookie_value(writer, name, value)?;
        assert_eq!(
            decrypt_cookie_value(&current, name, &wire)?.as_deref(),
            Some(value.as_slice())
        );
        assert!(decrypt_cookie_value(&current, "other", &wire)?.is_none());
    }
    // 载荷认证与前缀校验分别按完整密钥序列检查。
    let mut crossed = cookie_prefix(name, &previous_key).to_vec();
    crossed.extend_from_slice(value);
    let wire = current.encrypt_string(&crossed)?;
    assert_eq!(
        decrypt_cookie_value(&current, name, &wire)?.as_deref(),
        Some(value.as_slice())
    );
    let empty = encrypt_cookie_value(&current, name, b"")?;
    assert_eq!(
        decrypt_cookie_value(&current, name, &empty)?,
        Some(Vec::new())
    );
    let no_prefix = current.encrypt_string(value)?;
    assert!(decrypt_cookie_value(&current, name, &no_prefix)?.is_none());
    assert!(decrypt_cookie_value(&current, name, "not a ciphertext").is_err());
    Ok(())
}

#[cfg(feature = "session")]
#[test]
fn xsrf_header_removes_prefix_after_authentication() -> Result<()> {
    let encrypter = ConfiguredEncrypter::from_config(AppConfig {
        key: Some("A".repeat(32)),
        ..AppConfig::default()
    })?;
    for name in ["XSRF-TOKEN", "other"] {
        let wire = encrypt_cookie_value(&encrypter, name, b"session-token")?;
        assert_eq!(
            decrypt_xsrf_header(&encrypter, &wire).as_deref(),
            Some("session-token")
        );
        assert_eq!(
            decrypt_cookie_value(&encrypter, "XSRF-TOKEN", &wire)?.is_some(),
            name == "XSRF-TOKEN"
        );
    }
    let short = encrypter.encrypt_string(b"short")?;
    assert_eq!(decrypt_xsrf_header(&encrypter, &short).as_deref(), Some(""));
    let invalid_utf8 = encrypt_cookie_value(&encrypter, "XSRF-TOKEN", &[0xff])?;
    assert!(decrypt_xsrf_header(&encrypter, &invalid_utf8).is_none());
    assert!(decrypt_xsrf_header(&encrypter, "not a ciphertext").is_none());
    Ok(())
}

#[test]
fn serialized_string_uses_byte_lengths_and_authenticates_before_parsing() -> Result<()> {
    let old = ConfiguredEncrypter::from_config(AppConfig {
        key: Some("B".repeat(32)),
        ..AppConfig::default()
    })?;
    let reader = ConfiguredEncrypter::from_config(AppConfig {
        key: Some("A".repeat(32)),
        previous_keys: vec!["B".repeat(32)],
        ..AppConfig::default()
    })?;
    let bytes = b"\xe9\x9b\xaa\0+\"";
    let framed = b"s:6:\"\xe9\x9b\xaa\0+\"\";";
    for writer in [&old, &reader] {
        let wire = writer.encrypt_serialized_string(bytes)?;
        assert_eq!(&*reader.decrypt_serialized_string(&wire)?, bytes);
        assert_eq!(reader.decrypt_string(&wire)?, framed);
    }
    let empty = reader.encrypt_serialized_string(b"")?;
    assert_eq!(&*reader.decrypt_serialized_string(&empty)?, b"");

    for invalid in [
        b"s:2:\"x\";".as_slice(),
        b"s:+1:\"x\";",
        b"s:1:\"x\";trailing",
        b"s:99999999999999999999999999:\"x\";",
        b"a:0:{}",
        b"raw string",
    ] {
        let wire = reader.encrypt_string(invalid)?;
        let Err(error) = reader.decrypt_serialized_string(&wire) else {
            return Err(Error::custom("test_expected_failure", "无效字符串帧被接受"));
        };
        assert_eq!(error.code(), "encryption_serialized_string_invalid");
    }
    assert!(reader.decrypt_serialized_string("invalid wire").is_err());
    let oversized = vec![0; application_payload::MAX_PLAINTEXT_BYTES];
    let Err(error) = reader.encrypt_serialized_string(&oversized) else {
        return Err(Error::custom(
            "test_expected_failure",
            "超长序列化正文被接受",
        ));
    };
    assert_eq!(error.code(), "encryption_payload_encrypt_failed");
    Ok(())
}

#[test]
fn parses_all_historical_keys_before_validating_their_lengths() -> Result<()> {
    for missing in ["", "0"] {
        let config = AppConfig {
            key: Some("0123456789abcdef".to_owned()),
            cipher: "aes-128-cbc".to_owned(),
            previous_keys: vec!["short".to_owned(), missing.to_owned()],
            ..AppConfig::default()
        };
        let Err(error) = ConfiguredEncrypter::from_config(config) else {
            return Err(Error::custom("test_expected_failure", "空历史密钥被接受"));
        };
        assert_eq!(error.code(), "encryption_app_key_missing");
    }
    Ok(())
}

#[test]
fn reads_fixed_php_ciphertexts_with_rotated_keys() -> Result<()> {
    #[derive(serde::Deserialize)]
    struct Profile {
        cipher: String,
        wire: String,
    }

    #[derive(serde::Deserialize)]
    struct Fixtures {
        profiles: [Profile; 4],
        cookie: String,
    }

    // 使用固定 d52c3b31 的 Encrypter 和 CookieValuePrefix 生成的测试载荷。
    let fixtures: Fixtures =
        serde_json::from_str(include_str!("fixtures/fixed_php_encryption.json"))
            .map_err(Error::internal)?;
    for profile in fixtures.profiles {
        let key_len = if profile.cipher.starts_with("aes-128-") {
            16
        } else {
            32
        };
        let mut config = AppConfig {
            cipher: profile.cipher,
            key: Some("n".repeat(key_len)),
            ..AppConfig::default()
        };
        let current = ConfiguredEncrypter::from_config(config.clone())?;
        assert!(current.decrypt_string(&profile.wire).is_err());
        config.previous_keys = vec!["o".repeat(key_len)];
        let rotated = ConfiguredEncrypter::from_config(config)?;
        assert_eq!(rotated.decrypt_string(&profile.wire)?, b"persisted\0\xff");
    }

    let rotated = ConfiguredEncrypter::from_config(AppConfig {
        cipher: "aes-128-cbc".to_owned(),
        key: Some("n".repeat(16)),
        previous_keys: vec!["o".repeat(16)],
        ..AppConfig::default()
    })?;
    assert_eq!(
        decrypt_cookie_value(&rotated, "remember_fixture", &fixtures.cookie)?.as_deref(),
        Some(b"kept".as_slice())
    );
    assert!(decrypt_cookie_value(&rotated, "different", &fixtures.cookie)?.is_none());
    Ok(())
}
