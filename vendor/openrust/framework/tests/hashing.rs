use std::{collections::BTreeMap, sync::Arc};

use openrust::{
    config::Repository,
    contracts::hashing::Hasher,
    hashing::{
        Argon2IdHasher, ArgonConfig, ArgonHasher, BcryptConfig, BcryptHasher, HashError,
        HashManager, HashOptions,
    },
};
use serde_json::json;

#[tokio::test]
async fn supported_drivers_hash_verify_inspect_and_rehash() {
    let config = ArgonConfig {
        memory: 1024,
        time: 2,
        threads: 1,
        verify: true,
    };
    let bcrypt = BcryptHasher::new(BcryptConfig {
        rounds: 4,
        verify: true,
        limit: None,
    });
    let argon = ArgonHasher::new(config);
    let argon2id = Argon2IdHasher::new(config);
    let options = HashOptions::new();
    for (name, driver, override_options) in [
        ("bcrypt", &bcrypt as &dyn Hasher, json!({"rounds":5})),
        ("argon2i", &argon, json!({"time":3})),
        ("argon2id", &argon2id, json!({"time":3})),
    ] {
        let value = b"test\xff bytes";
        let stored = driver.make(value, &options).await.unwrap();
        let info = driver.info(&stored).unwrap();
        assert_eq!(info.algoName, name);
        assert!(driver.check(value, &stored, &options).await.unwrap());
        assert!(!driver.check(b"wrong", &stored, &options).await.unwrap());
        assert!(!driver.check(value, "", &options).await.unwrap());
        assert!(driver.verifyConfiguration(&stored).unwrap());
        assert!(!driver.needsRehash(&stored, &options).unwrap());
        assert!(
            driver
                .needsRehash(&stored, override_options.as_object().unwrap())
                .unwrap()
        );
        let changed = driver
            .make(value, override_options.as_object().unwrap())
            .await
            .unwrap();
        assert!(!driver.verifyConfiguration(&changed).unwrap());
        assert!(driver.needsRehash(&changed, &options).unwrap());
        assert!(matches!(
            driver.check(value, "malformed", &options).await,
            Err(HashError::AlgorithmMismatch)
        ));
    }
    let stored = bcrypt.make(b"hello", &options).await.unwrap();
    assert!(matches!(
        argon.check(b"hello", &stored, &options).await,
        Err(HashError::AlgorithmMismatch)
    ));
    let migration = Argon2IdHasher::new(ArgonConfig {
        verify: false,
        ..config
    });
    assert!(migration.check(b"hello", &stored, &options).await.unwrap());
    assert!(migration.needsRehash(&stored, &options).unwrap());
    let limited = BcryptHasher::new(BcryptConfig {
        rounds: 4,
        limit: Some(4),
        ..Default::default()
    });
    assert!(matches!(
        limited.make(b"hello", &options).await,
        Err(HashError::InvalidInput)
    ));
    assert!(matches!(
        bcrypt.make(b"hello\0suffix", &options).await,
        Err(HashError::InvalidInput)
    ));
    assert!(
        bcrypt
            .check(b"hello\0suffix", &stored, &options)
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn historical_2x_hashes_use_php_compatible_byte_verification() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/hashing_legacy_2x_php85.json")).unwrap();
    let hasher = BcryptHasher::new(BcryptConfig::default());
    let options = HashOptions::new();
    for row in fixture["vectors"].as_array().unwrap() {
        let bytes = row["value"].as_str().unwrap().as_bytes();
        let hashed = row["hash"].as_str().unwrap();
        assert_eq!(
            hasher.check(bytes, hashed, &options).await.unwrap(),
            row["valid"].as_bool().unwrap()
        );
        assert!(!hasher.check(b"wrong", hashed, &options).await.unwrap());
        assert!(hasher.needsRehash(hashed, &options).unwrap());
        assert_eq!(hasher.info(hashed).unwrap().algo, None);
    }
}

#[tokio::test]
async fn named_drivers_share_capacity_and_decline_expensive_untrusted_hashes() {
    let config = Repository::new(BTreeMap::from([(
        "hashing".into(),
        json!({
            "max_concurrent_operations": 1,
            "bcrypt": {"rounds": 11},
            "argon": {"memory": 1024, "time": 2, "threads": 1},
        }),
    )]));
    let manager = HashManager::new(Arc::new(config)).unwrap();
    let bcrypt = manager.driver(None).unwrap();
    let argon = manager.driver(Some("argon")).unwrap();
    assert!(Arc::ptr_eq(
        &bcrypt,
        &manager.driver(Some("bcrypt")).unwrap()
    ));
    let options = HashOptions::new();
    let (first, overflow) = tokio::join!(
        bcrypt.make(b"slow", &options),
        argon.make(b"over capacity", &options)
    );
    assert!(first.is_ok());
    assert!(matches!(overflow, Err(HashError::CapacityExhausted)));
    assert!(argon.make(b"retry", &options).await.is_ok());

    let encoded = first.unwrap();
    let expensive = encoded.replacen("$2y$11$", "$2y$31$", 1);
    assert!(matches!(
        bcrypt.check(b"slow", &expensive, &options).await,
        Err(HashError::ExecutionLimit)
    ));
    assert!(matches!(
        argon
            .make(b"slow", json!({"memory": 524288}).as_object().unwrap())
            .await,
        Err(HashError::ExecutionLimit)
    ));
}

#[tokio::test]
async fn historical_argon2_version_16_matches_php_verification() {
    use argon2::{Algorithm, Argon2, Params, Version, password_hash::PasswordHasher as _};
    let config = ArgonConfig {
        memory: 1024,
        time: 2,
        threads: 1,
        verify: true,
    };
    let argon_i = ArgonHasher::new(config);
    let argon_id = Argon2IdHasher::new(config);
    let options = HashOptions::new();
    for (algorithm, driver) in [
        (Algorithm::Argon2i, &argon_i as &dyn Hasher),
        (Algorithm::Argon2id, &argon_id as &dyn Hasher),
    ] {
        let params = Params::new(1024, 2, 1, None).unwrap();
        let encoded = Argon2::new(algorithm, Version::V0x10, params)
            .hash_password(b"sample")
            .unwrap()
            .to_string();
        assert!(driver.check(b"sample", &encoded, &options).await.unwrap());
        assert!(!driver.needsRehash(&encoded, &options).unwrap());
        let legacy = encoded.replace("$v=16", "");
        assert!(driver.check(b"sample", &legacy, &options).await.unwrap());
        assert!(driver.needsRehash(&legacy, &options).unwrap());
    }
}
