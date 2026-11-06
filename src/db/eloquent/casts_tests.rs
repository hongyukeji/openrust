//! hashed cast 的分支、配置错误及不升级旧哈希行为回归。

use super::*;
use crate::{
    Error,
    hashing::{ArgonConfig, BcryptConfig, HashingConfig},
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

fn bcrypt(rounds: u32) -> Result<HashManager> {
    HashManager::new(HashingConfig {
        driver: "bcrypt".into(),
        bcrypt: BcryptConfig {
            rounds,
            verify: true,
            limit: None,
        },
        ..HashingConfig::default()
    })
}

fn required(value: Option<String>) -> Result<String> {
    value.ok_or_else(|| Error::custom("test_cast_missing", "属性转换未返回哈希"))
}

#[tokio::test]
async fn plaintext_and_empty_string_get_random_salts_and_verifiable_hashes() -> Result<()> {
    let hashing = bcrypt(4)?;
    for value in ["ordinary password", ""] {
        let first = required(cast_attribute_as_hashed_string(&hashing, Some(value)).await?)?;
        let second = required(cast_attribute_as_hashed_string(&hashing, Some(value)).await?)?;
        assert_ne!(first, second);
        assert!(
            hashing
                .check(value, Some(&first), &HashOptions::new())
                .await?
        );
        assert!(
            hashing
                .check(value, Some(&second), &HashOptions::new())
                .await?
        );
        assert!(hashing.is_hashed(&first)?);
    }
    Ok(())
}

#[tokio::test]
async fn null_never_resolves_an_invalid_default_driver() -> Result<()> {
    let hashing = HashManager::new(HashingConfig {
        driver: "missing".into(),
        ..HashingConfig::default()
    })?;
    assert_eq!(cast_attribute_as_hashed_string(&hashing, None).await?, None);
    assert!(
        matches!(cast_attribute_as_hashed_string(&hashing, Some("")).await, Err(error) if error.code() == "hashing_driver_unsupported")
    );
    Ok(())
}

#[tokio::test]
async fn lower_cost_hash_stays_byte_identical_even_when_it_needs_rehash() -> Result<()> {
    let encoded = bcrypt(4)?.make("old password", &HashOptions::new()).await?;
    let hashing = bcrypt(5)?;
    assert!(hashing.needs_rehash(&encoded, &HashOptions::new())?);
    assert!(hashing.verify_configuration(&encoded)?);
    assert_eq!(
        cast_attribute_as_hashed_string(&hashing, Some(&encoded)).await?,
        Some(encoded)
    );
    Ok(())
}

#[tokio::test]
async fn higher_cost_and_other_algorithm_hashes_are_rejected_with_fixed_internal_error()
-> Result<()> {
    let hashing = bcrypt(4)?;
    let high = bcrypt(5)?.make("high cost", &HashOptions::new()).await?;
    let other = HashManager::new(HashingConfig {
        driver: "argon2id".into(),
        argon: ArgonConfig {
            memory: 1024,
            time: 1,
            threads: 1,
            verify: true,
        },
        ..HashingConfig::default()
    })?
    .make("other algorithm", &HashOptions::new())
    .await?;
    for encoded in [high, other] {
        assert!(hashing.is_hashed(&encoded)?);
        assert!(!hashing.verify_configuration(&encoded)?);
        let error = cast_attribute_as_hashed_string(&hashing, Some(&encoded))
            .await
            .err()
            .ok_or_else(|| Error::custom("test_cast_accepted", "不匹配的已有哈希被接受"))?;
        assert_eq!(error.code(), "model_hash_configuration_invalid");
        assert_eq!(error.status().as_u16(), 500);
        assert!(!format!("{error:?}").contains(&encoded));
    }
    Ok(())
}

#[tokio::test]
async fn factory_errors_propagate_unchanged_and_remain_retryable() -> Result<()> {
    let hashing = HashManager::new(HashingConfig {
        driver: "failure".into(),
        ..HashingConfig::default()
    })?;
    let calls = Arc::new(AtomicUsize::new(0));
    let attempts = Arc::clone(&calls);
    hashing.extend("failure", move || {
        attempts.fetch_add(1, Ordering::SeqCst);
        Err(Error::custom("cast_factory_failure", "驱动工厂失败"))
    })?;
    assert_eq!(cast_attribute_as_hashed_string(&hashing, None).await?, None);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    for value in [
        "plain",
        "$2y$04$abcdefghijklmnopqrstuuCi15uRb1eH7NAlJ/TgeJertyknQpYn2",
    ] {
        assert!(
            matches!(cast_attribute_as_hashed_string(&hashing, Some(value)).await, Err(error) if error.code() == "cast_factory_failure")
        );
    }
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    Ok(())
}
