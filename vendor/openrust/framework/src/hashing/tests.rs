//! 三驱动、共享管理器及PHP边界回归；测试成本与安装默认分开。

use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

fn bcrypt(verify: bool) -> Result<BcryptHasher> {
    BcryptHasher::new(BcryptConfig {
        rounds: 4,
        verify,
        limit: None,
    })
}
fn argon(verify: bool) -> ArgonConfig {
    ArgonConfig {
        memory: 1024,
        time: 1,
        threads: 1,
        verify,
    }
}
fn config() -> HashingConfig {
    HashingConfig {
        bcrypt: BcryptConfig {
            rounds: 4,
            ..BcryptConfig::default()
        },
        argon: argon(false),
        ..HashingConfig::default()
    }
}

#[test]
fn naked_and_installed_defaults_are_distinct() {
    let naked = BcryptConfig::default();
    assert_eq!(naked.rounds, 12);
    assert!(!naked.verify);
    assert_eq!(naked.limit, None);
    let naked = ArgonConfig::default();
    assert_eq!(
        (naked.memory, naked.time, naked.threads, naked.verify),
        (1024, 2, 2, false)
    );
    let installed = HashingConfig::default();
    assert_eq!(installed.driver, "bcrypt");
    assert!(installed.bcrypt.verify);
    assert_eq!(installed.bcrypt.rounds, 12);
    assert_eq!(installed.bcrypt.limit, None);
    assert_eq!(
        (
            installed.argon.memory,
            installed.argon.time,
            installed.argon.threads,
            installed.argon.verify
        ),
        (65_536, 4, 1, true)
    );
    assert!(installed.rehash_on_login);
}

#[tokio::test]
async fn all_algorithms_hash_random_salts_and_verify_without_rehashing_salts() -> Result<()> {
    let options = HashOptions::new();
    let drivers: Vec<Arc<dyn Hasher>> = vec![
        Arc::new(bcrypt(false)?),
        Arc::new(ArgonHasher::new(argon(false))?),
        Arc::new(Argon2IdHasher::new(argon(false))?),
    ];
    for (driver, algorithm) in drivers.into_iter().zip(["bcrypt", "argon2i", "argon2id"]) {
        let first = driver.make("correct password".as_bytes(), &options).await?;
        let second = driver.make("correct password".as_bytes(), &options).await?;
        assert_ne!(first, second);
        assert_eq!(driver.info(&first)?.algo_name, algorithm);
        assert!(
            driver
                .check("correct password".as_bytes(), Some(&first), &options)
                .await?
        );
        assert!(
            !driver
                .check("wrong password".as_bytes(), Some(&first), &options)
                .await?
        );
        assert!(!driver.check("".as_bytes(), None, &options).await?);
        assert!(!driver.check("".as_bytes(), Some(""), &options).await?);
        assert!(!driver.needs_rehash(&first, &options)?);
        if algorithm == "bcrypt" {
            assert!(first.starts_with("$2y$04$"));
        }
    }
    Ok(())
}

#[tokio::test]
async fn verify_configuration_does_not_replace_needs_rehash() -> Result<()> {
    let options = HashOptions::new();
    let low = bcrypt(false)?.make("password".as_bytes(), &options).await?;
    let high = BcryptHasher::new(BcryptConfig {
        rounds: 5,
        ..BcryptConfig::default()
    })?;
    assert!(high.verify_configuration(&low)?);
    assert!(high.needs_rehash(&low, &options)?);
    let changed = HashOptions::from([("rounds".into(), 5.into())]);
    assert!(!high.needs_rehash(&low, &HashOptions::from([("rounds".into(), 4.into())]))?);
    let new = high.make("password".as_bytes(), &changed).await?;
    assert!(!bcrypt(false)?.verify_configuration(&new)?);
    assert!(high.needs_rehash(&new, &HashOptions::from([("rounds".into(), 4.into())]))?);
    Ok(())
}

#[tokio::test]
async fn permissive_drivers_verify_all_three_algorithms_and_strict_drivers_reject_others()
-> Result<()> {
    let options = HashOptions::new();
    let permissive: Vec<Arc<dyn Hasher>> = vec![
        Arc::new(bcrypt(false)?),
        Arc::new(ArgonHasher::new(argon(false))?),
        Arc::new(Argon2IdHasher::new(argon(false))?),
    ];
    let strict: Vec<Arc<dyn Hasher>> = vec![
        Arc::new(bcrypt(true)?),
        Arc::new(ArgonHasher::new(argon(true))?),
        Arc::new(Argon2IdHasher::new(argon(true))?),
    ];
    for (index, maker) in permissive.iter().enumerate() {
        let encoded = maker.make("password".as_bytes(), &options).await?;
        for (other, verifier) in permissive.iter().enumerate() {
            assert!(
                verifier
                    .check("password".as_bytes(), Some(&encoded), &options)
                    .await?
            );
            let actual = strict[other]
                .check("password".as_bytes(), Some(&encoded), &options)
                .await;
            if index == other {
                assert!(actual?);
            } else {
                assert!(
                    matches!(actual, Err(ref error) if error.code() == "hashing_algorithm_mismatch")
                );
            }
        }
    }
    Ok(())
}

#[tokio::test]
async fn bcrypt_truncates_72_bytes_but_optional_limit_is_checked_before_hashing() -> Result<()> {
    let options = HashOptions::new();
    let seventy_two = "a".repeat(72);
    let longer = format!("{seventy_two}b");
    let driver = bcrypt(false)?;
    let encoded = driver.make(longer.as_bytes(), &options).await?;
    assert!(
        driver
            .check(seventy_two.as_bytes(), Some(&encoded), &options)
            .await?
    );
    assert!(
        driver
            .check(longer.as_bytes(), Some(&encoded), &options)
            .await?
    );
    let limited = BcryptHasher::new(BcryptConfig {
        rounds: 4,
        limit: Some(72),
        verify: false,
    })?;
    assert!(
        matches!(limited.make(longer.as_bytes(), &options).await, Err(ref error) if error.code() == "hashing_value_too_long")
    );
    assert!(limited.make(seventy_two.as_bytes(), &options).await.is_ok());
    Ok(())
}

#[tokio::test]
async fn nul_make_and_check_boundaries_follow_php_oracle() -> Result<()> {
    let options = HashOptions::new();
    let driver = bcrypt(false)?;
    for value in ["\0", "correct\0tail", "prefix\0"] {
        assert!(driver.make(value.as_bytes(), &options).await.is_err());
    }
    let encoded = driver.make("correct".as_bytes(), &options).await?;
    assert!(
        driver
            .check("correct\0tail".as_bytes(), Some(&encoded), &options)
            .await?
    );
    for driver in [
        Arc::new(ArgonHasher::new(argon(false))?) as Arc<dyn Hasher>,
        Arc::new(Argon2IdHasher::new(argon(false))?) as Arc<dyn Hasher>,
    ] {
        for value in ["", "correct\0tail"] {
            let encoded = driver.make(value.as_bytes(), &options).await?;
            assert!(
                driver
                    .check(value.as_bytes(), Some(&encoded), &options)
                    .await?
            );
        }
        let encoded = driver.make("correct".as_bytes(), &options).await?;
        assert!(
            !driver
                .check("correct\0tail".as_bytes(), Some(&encoded), &options)
                .await?
        );
    }
    Ok(())
}

#[tokio::test]
async fn argon_options_and_known_other_algorithm_metadata_are_available() -> Result<()> {
    let options = HashOptions::new();
    let driver = Argon2IdHasher::new(argon(false))?;
    let encoded = driver
        .make(
            "value".as_bytes(),
            &HashOptions::from([
                ("memory".into(), 2048.into()),
                ("time".into(), 2.into()),
                ("threads".into(), 2.into()),
            ]),
        )
        .await?;
    assert_eq!(
        driver.info(&encoded)?.options,
        HashOptions::from([
            ("memory_cost".into(), 2048.into()),
            ("time_cost".into(), 2.into()),
            ("threads".into(), 2.into())
        ])
    );
    assert!(driver.needs_rehash(&encoded, &options)?);
    assert!(!driver.verify_configuration(&encoded)?);
    assert_eq!(bcrypt(false)?.info(&encoded)?.algo_name, "argon2id");
    Ok(())
}

#[tokio::test]
async fn php_metadata_recognition_is_not_full_phc_validation() -> Result<()> {
    let driver = Argon2IdHasher::new(argon(false))?;
    for encoded in [
        "$argon2id$",
        "$argon2id$m=1,t=2,p=3$",
        "$argon2id$v=19$p=1,t=2,m=1024$",
    ] {
        let info = driver.info(encoded)?;
        assert_eq!(info.algo_name, "argon2id");
        assert_eq!(
            info.options,
            HashOptions::from([
                ("memory_cost".into(), 65_536.into()),
                ("time_cost".into(), 4.into()),
                ("threads".into(), 1.into())
            ])
        );
        assert!(
            !driver
                .check("value".as_bytes(), Some(encoded), &HashOptions::new())
                .await?
        );
    }
    let info = driver.info("$argon2i$v=20$m=1024,t=1,p=2$bad$bad")?;
    assert_eq!(info.algo_name, "argon2i");
    assert_eq!(info.options.get("threads"), Some(&2.into()));
    Ok(())
}

#[tokio::test]
async fn bcrypt_foreign_prefixes_remain_unknown_but_permissive_ascii_verification_works()
-> Result<()> {
    let options = HashOptions::new();
    let driver = bcrypt(false)?;
    let encoded = driver.make("ascii-password".as_bytes(), &options).await?;
    for prefix in ["2a", "2b", "2x"] {
        let variant = encoded.replacen("2y", prefix, 1);
        assert_eq!(driver.info(&variant)?.algo, None);
        assert!(
            driver
                .check("ascii-password".as_bytes(), Some(&variant), &options)
                .await?
        );
        assert!(
            bcrypt(true)?
                .check("ascii-password".as_bytes(), Some(&variant), &options)
                .await
                .is_err()
        );
    }
    Ok(())
}

#[tokio::test]
async fn malformed_stored_values_do_not_leak_and_unknown_values_need_rehash() -> Result<()> {
    let driver = bcrypt(false)?;
    let options = HashOptions::new();
    for stored in [
        "not-a-hash",
        "$2z$04$bad",
        "$argon2d$v=19$m=1024,t=1,p=1$bad$bad",
    ] {
        assert!(driver.info(stored)?.algo.is_none());
        assert!(
            !driver
                .check("secret".as_bytes(), Some(stored), &options)
                .await?
        );
        assert!(driver.needs_rehash(stored, &options)?);
    }
    let error = driver
        .make(
            "secret".as_bytes(),
            &HashOptions::from([("rounds".into(), "secret-invalid-option".into())]),
        )
        .await
        .err()
        .ok_or_else(|| failure("test_error_missing", "无效参数应拒绝"))?;
    assert!(!error.to_string().contains("secret"));
    Ok(())
}

#[test]
fn driver_cache_and_default_resolution_share_identity() -> Result<()> {
    let manager = HashManager::new(config())?;
    let first = manager.driver(None)?;
    assert!(Arc::ptr_eq(&first, &manager.driver(Some("bcrypt"))?));
    assert!(Arc::ptr_eq(&first, &manager.driver(Some(""))?));
    assert!(Arc::ptr_eq(&first, &manager.driver(Some("0"))?));
    assert!(
        matches!(manager.driver(Some("missing")), Err(ref error) if error.code() == "hashing_driver_unsupported")
    );
    Ok(())
}

#[test]
fn extension_precedes_builtin_and_preserves_cached_driver() -> Result<()> {
    let manager = HashManager::new(config())?;
    let calls = Arc::new(AtomicUsize::new(0));
    let custom: Arc<dyn Hasher> = Arc::new(ArgonHasher::new(argon(false))?);
    manager.extend("bcrypt", {
        let calls = Arc::clone(&calls);
        let custom = Arc::clone(&custom);
        move || {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(Arc::clone(&custom))
        }
    })?;
    let first = manager.driver(None)?;
    assert!(Arc::ptr_eq(&first, &custom));
    assert!(Arc::ptr_eq(&first, &manager.driver(None)?));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    manager.extend("bcrypt", || Ok(Arc::new(bcrypt(false)?) as Arc<dyn Hasher>))?;
    assert!(Arc::ptr_eq(&first, &manager.driver(None)?));
    Ok(())
}

#[test]
fn factory_failure_does_not_leave_creation_busy_or_cache_failure() -> Result<()> {
    let manager = HashManager::new(config())?;
    let calls = AtomicUsize::new(0);
    let calls = Arc::new(calls);
    manager.extend("retry", {
        let calls = Arc::clone(&calls);
        move || {
            if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                return Err(failure("factory_failure", "工厂失败"));
            }
            Ok(Arc::new(bcrypt(false)?) as Arc<dyn Hasher>)
        }
    })?;
    assert!(
        matches!(manager.driver(Some("retry")), Err(ref error) if error.code() == "factory_failure")
    );
    manager.driver(Some("retry"))?;
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    Ok(())
}

#[test]
fn recursive_factory_is_rejected_without_holding_state_lock() -> Result<()> {
    let manager = Arc::new(HashManager::new(config())?);
    let weak = Arc::downgrade(&manager);
    manager.extend("recursive", move || {
        weak.upgrade()
            .ok_or_else(|| failure("manager_missing", "测试管理器缺失"))?
            .driver(Some("recursive"))
    })?;
    for _ in 0..2 {
        assert!(
            matches!(manager.driver(Some("recursive")), Err(ref error) if error.code() == "hashing_driver_busy")
        );
    }
    manager.driver(Some("bcrypt"))?;
    Ok(())
}

#[test]
fn concurrent_same_name_creation_is_bounded_and_unrelated_driver_still_resolves() -> Result<()> {
    let manager = HashManager::new(config())?;
    let entered = std::sync::Barrier::new(2);
    let release = std::sync::Barrier::new(2);
    let entered = Arc::new(entered);
    let release = Arc::new(release);
    manager.extend("slow", {
        let entered = Arc::clone(&entered);
        let release = Arc::clone(&release);
        move || {
            entered.wait();
            release.wait();
            Ok(Arc::new(bcrypt(false)?) as Arc<dyn Hasher>)
        }
    })?;
    let outcome = std::thread::scope(|scope| {
        let task = scope.spawn(|| manager.driver(Some("slow")));
        entered.wait();
        let busy = manager.driver(Some("slow"));
        let independent = manager.driver(Some("bcrypt"));
        release.wait();
        (busy, independent, task.join())
    });
    assert!(matches!(outcome.0, Err(ref error) if error.code() == "hashing_driver_busy"));
    outcome.1?;
    outcome
        .2
        .map_err(|_| failure("test_thread_failed", "测试线程退出"))??;
    Ok(())
}

#[tokio::test]
async fn manager_delegates_hash_check_info_and_recognizes_nondefault_algorithm() -> Result<()> {
    let manager = HashManager::new(config())?;
    let options = HashOptions::new();
    let encoded = manager.make("value", &options).await?;
    assert!(manager.check("value", Some(&encoded), &options).await?);
    assert!(manager.is_hashed(&encoded)?);
    assert!(!manager.is_hashed("not-hashed")?);
    assert!(!manager.needs_rehash(&encoded, &options)?);
    assert!(manager.verify_configuration(&encoded)?);
    let argon = manager
        .driver(Some("argon2id"))?
        .make("value".as_bytes(), &options)
        .await?;
    assert!(manager.is_hashed(&argon)?);
    assert_eq!(manager.info(&argon)?.algo_name, "argon2id");
    Ok(())
}

#[test]
fn capacity_configuration_is_bounded_without_changing_official_hash_costs() {
    for capacity in [0, 5] {
        assert!(HashManager::with_max_concurrent_operations(config(), capacity).is_err());
    }
}

fn php_fixture() -> Result<serde_json::Value> {
    serde_json::from_str(include_str!("../../tests/fixtures/hashing_php85.json"))
        .map_err(crate::core::Error::internal)
}
fn fixture_items<'a>(
    value: &'a serde_json::Value,
    key: &str,
) -> Result<&'a Vec<serde_json::Value>> {
    value
        .get(key)
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| failure("hashing_fixture_invalid", "PHP哈希fixture数组缺失"))
}
fn fixture_text<'a>(value: &'a serde_json::Value, key: &str) -> Result<&'a str> {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| failure("hashing_fixture_invalid", "PHP哈希fixture字符串缺失"))
}
fn fixture_optional_text<'a>(value: &'a serde_json::Value, key: &str) -> Result<Option<&'a str>> {
    match value.get(key) {
        Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::String(value)) => Ok(Some(value)),
        _ => Err(failure(
            "hashing_fixture_invalid",
            "PHP哈希fixture可空字符串格式无效",
        )),
    }
}
fn fixture_driver(name: &str, verify: bool) -> Result<Arc<dyn Hasher>> {
    match name {
        "bcrypt" => Ok(Arc::new(bcrypt(verify)?)),
        "argon" => Ok(Arc::new(ArgonHasher::new(argon(verify))?)),
        "argon2id" => Ok(Arc::new(Argon2IdHasher::new(argon(verify))?)),
        _ => Err(failure("hashing_fixture_invalid", "PHP哈希fixture驱动未知")),
    }
}
#[tokio::test]
async fn php_oracle_make_success_failure_and_generated_hash_verification_match() -> Result<()> {
    let fixture = php_fixture()?;
    let options = HashOptions::new();
    for record in fixture_items(&fixture, "make")? {
        let driver = fixture_driver(fixture_text(record, "driver")?, false)?;
        // serde_json直接解码原始NUL；不把普通反斜线文本解释为控制字符。
        let value = fixture_text(record, "value")?;
        if fixture_text(record, "case")?.starts_with("nul-") {
            assert!(value.as_bytes().contains(&0));
        }
        let made = driver.make(value.as_bytes(), &options).await;
        if record["result"].get("error").is_some() {
            assert!(
                made.is_err(),
                "{} {}",
                fixture_text(record, "driver")?,
                fixture_text(record, "case")?
            );
        } else {
            let made = made?;
            assert!(
                driver
                    .check(value.as_bytes(), Some(&made), &options)
                    .await?
            );
        }
    }
    Ok(())
}

#[tokio::test]
async fn php_oracle_all_stored_hashes_and_strict_permissive_checks_match() -> Result<()> {
    let fixture = php_fixture()?;
    let options = HashOptions::new();
    for record in fixture_items(&fixture, "hashes")? {
        let value = fixture_text(record, "value")?;
        let driver = fixture_driver(fixture_text(record, "driver")?, false)?;
        assert!(
            driver
                .check(
                    value.as_bytes(),
                    Some(fixture_text(record, "hash")?),
                    &options
                )
                .await?,
            "{} {}",
            fixture_text(record, "driver")?,
            fixture_text(record, "case")?
        );
    }
    for record in fixture_items(&fixture, "check")? {
        let value = fixture_text(record, "value")?;
        let verify = record["verify"]
            .as_bool()
            .ok_or_else(|| failure("hashing_fixture_invalid", "PHP校验策略缺失"))?;
        let actual = fixture_driver(fixture_text(record, "driver")?, verify)?
            .check(
                value.as_bytes(),
                fixture_optional_text(record, "hash")?,
                &options,
            )
            .await;
        if record["result"].get("error").is_some() {
            assert!(
                matches!(actual, Err(ref error) if error.code() == "hashing_algorithm_mismatch")
            );
        } else {
            assert_eq!(
                Some(actual?),
                record["result"]["ok"].as_bool(),
                "{} {}",
                fixture_text(record, "driver")?,
                fixture_text(record, "case")?
            );
        }
    }
    Ok(())
}

#[test]
fn php_oracle_metadata_same_changed_rehash_and_configuration_match() -> Result<()> {
    let fixture = php_fixture()?;
    let options = HashOptions::new();
    for record in fixture_items(&fixture, "metadata")? {
        let name = fixture_text(record, "driver")?;
        let case = fixture_text(record, "case")?;
        let hash = fixture_items(&fixture, "hashes")?
            .iter()
            .find(|hash| hash["driver"] == name && hash["case"] == case)
            .ok_or_else(|| failure("hashing_fixture_invalid", "PHP元信息对应hash缺失"))?;
        let hash = fixture_text(hash, "hash")?;
        let driver = fixture_driver(name, false)?;
        assert_eq!(
            serde_json::to_value(driver.info(hash)?).map_err(crate::core::Error::internal)?,
            record["info"]
        );
        assert_eq!(
            Some(driver.needs_rehash(hash, &options)?),
            record["rehash_same"].as_bool()
        );
        let changed = if name == "bcrypt" {
            HashOptions::from([("rounds".into(), 5.into())])
        } else {
            HashOptions::from([("time".into(), 2.into())])
        };
        assert_eq!(
            Some(driver.needs_rehash(hash, &changed)?),
            record["rehash_changed"].as_bool()
        );
        assert_eq!(
            Some(driver.verify_configuration(hash)?),
            record["verify_configuration"].as_bool()
        );
    }
    Ok(())
}

#[tokio::test]
async fn php_oracle_foreign_versions_and_malformed_metadata_match() -> Result<()> {
    let fixture = php_fixture()?;
    let options = HashOptions::new();
    for record in fixture_items(&fixture, "variants")? {
        let hash = fixture_text(record, "hash")?;
        let value = fixture_text(record, "value")?;
        for name in ["bcrypt", "argon", "argon2id"] {
            let driver = fixture_driver(name, false)?;
            assert_eq!(
                serde_json::to_value(driver.info(hash)?).map_err(crate::core::Error::internal)?,
                record["info"]
            );
            assert_eq!(
                Some(driver.needs_rehash(hash, &options)?),
                record["needs_rehash"][name].as_bool()
            );
            for verify in [false, true] {
                let key = format!("{name}-{}", if verify { "strict" } else { "permissive" });
                let expected = &record["checks"][&key];
                let actual = fixture_driver(name, verify)?
                    .check(value.as_bytes(), Some(hash), &options)
                    .await;
                if expected.get("error").is_some() {
                    assert!(
                        matches!(actual, Err(ref error) if error.code() == "hashing_algorithm_mismatch")
                    );
                } else {
                    assert_eq!(
                        Some(actual?),
                        expected["ok"].as_bool(),
                        "{} {}",
                        fixture_text(record, "source_driver")?,
                        fixture_text(record, "variant")?
                    );
                }
            }
        }
    }
    Ok(())
}

#[tokio::test]
async fn unpolled_operation_capacity_is_released_on_cancellation() -> Result<()> {
    let pool = drivers::OperationPool::new(1)?;
    let pending = pool.run(|| Ok(1));
    assert!(
        matches!(pool.run(|| Ok(2)).await, Err(ref error) if error.code() == "password_capacity_exhausted")
    );
    drop(pending);
    assert_eq!(pool.run(|| Ok(3)).await?, 3);
    Ok(())
}

#[test]
fn unknown_hash_info_serializes_exact_php_empty_options_array() -> Result<()> {
    let info = bcrypt(false)?.info("not-a-hash")?;
    assert!(info.options.is_empty());
    assert_eq!(
        serde_json::to_value(&info).map_err(crate::core::Error::internal)?,
        serde_json::json!({"algo":null,"algoName":"unknown","options":[]})
    );
    Ok(())
}

#[test]
fn default_factory_failure_and_self_reference_are_not_cached_or_leaked() -> Result<()> {
    let manager = Arc::new(HashManager::new(HashingConfig {
        driver: "self".into(),
        ..config()
    })?);
    let weak = Arc::downgrade(&manager);
    let witness = weak.clone();
    let calls = Arc::new(AtomicUsize::new(0));
    let attempts = Arc::clone(&calls);
    manager.extend("self", move || {
        match attempts.fetch_add(1, Ordering::SeqCst) {
            0 => Err(failure("factory_failure", "工厂失败")),
            1 => {
                let same = weak
                    .upgrade()
                    .ok_or_else(|| failure("manager_missing", "测试管理器缺失"))?;
                Ok(same as Arc<dyn Hasher>)
            }
            _ => Ok(Arc::new(bcrypt(false)?) as Arc<dyn Hasher>),
        }
    })?;
    assert!(matches!(manager.driver(None), Err(error) if error.code() == "factory_failure"));
    assert!(
        matches!(manager.driver(None), Err(error) if error.code() == "hashing_driver_self_reference")
    );
    assert_eq!(Arc::strong_count(&manager), 1);
    assert!(
        matches!(manager.driver(None), Err(error) if error.code() == "hashing_driver_self_reference")
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    let repaired_attempts = Arc::clone(&calls);
    manager.extend("self", move || {
        repaired_attempts.fetch_add(1, Ordering::SeqCst);
        Ok(Arc::new(bcrypt(false)?) as Arc<dyn Hasher>)
    })?;
    let successful = manager.driver(None)?;
    assert!(Arc::ptr_eq(&successful, &manager.driver(None)?));
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    drop(successful);
    drop(manager);
    assert!(witness.upgrade().is_none());
    Ok(())
}

#[tokio::test]
async fn manager_trait_propagates_default_driver_resolution_errors_for_all_methods() -> Result<()> {
    let manager = HashManager::new(HashingConfig {
        driver: "unsupported".into(),
        ..config()
    })?;
    let hasher: &dyn Hasher = &manager;
    let options = HashOptions::new();
    assert!(
        matches!(hasher.info("unknown"), Err(error) if error.code() == "hashing_driver_unsupported")
    );
    assert!(
        matches!(hasher.verify_configuration("unknown"), Err(error) if error.code() == "hashing_driver_unsupported")
    );
    assert!(
        matches!(hasher.needs_rehash("unknown", &options), Err(error) if error.code() == "hashing_driver_unsupported")
    );
    assert!(
        matches!(hasher.make("test".as_bytes(), &options).await, Err(error) if error.code() == "hashing_driver_unsupported")
    );
    assert!(
        matches!(hasher.check("test".as_bytes(), Some("unknown"), &options).await, Err(error) if error.code() == "hashing_driver_unsupported")
    );
    Ok(())
}

#[tokio::test]
async fn public_permissive_driver_verifies_every_php_2x_fixture_with_existing_budget() -> Result<()>
{
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/hashing_legacy_2x_php85.json"
    ))
    .map_err(Error::internal)?;
    let records = fixture["vectors"]
        .as_array()
        .ok_or_else(|| failure("fixture_invalid", "测试向量缺失"))?;
    assert_eq!(records.len(), 24);
    let driver = bcrypt(false)?;
    let options = HashOptions::new();
    for record in records {
        let value = record["value"]
            .as_str()
            .ok_or_else(|| failure("fixture_invalid", "测试输入缺失"))?;
        let encoded = record["hash"]
            .as_str()
            .ok_or_else(|| failure("fixture_invalid", "测试哈希缺失"))?;
        assert_eq!(record["valid"].as_bool(), Some(true));
        assert_eq!(record["wrong"].as_bool(), Some(false));
        assert!(
            driver
                .check(value.as_bytes(), Some(encoded), &options)
                .await?
        );
        assert!(
            !driver
                .check("wrong".as_bytes(), Some(encoded), &options)
                .await?
        );
        assert_eq!(driver.info(encoded)?.algo, None);
    }
    let expensive = "$2x$17$abcdefghijklmnopqrstuuCi15uRb1eH7NAlJ/TgeJertyknQpYn2";
    let bounded = BcryptHasher::with_execution_limits(
        BcryptConfig {
            rounds: 4,
            verify: false,
            limit: None,
        },
        ExecutionLimits {
            max_bcrypt_rounds: 4,
            ..ExecutionLimits::default()
        },
    )?;
    assert!(
        matches!(bounded.check("abc".as_bytes(), Some(expensive), &options).await, Err(error) if error.code() == "hashing_execution_limit_exceeded")
    );
    Ok(())
}

#[test]
fn directly_self_returning_strong_factory_is_replaced_without_builtin_fallback() -> Result<()> {
    let manager = Arc::new(HashManager::new(config())?);
    let witness = Arc::downgrade(&manager);
    let captured = Arc::clone(&manager);
    let calls = Arc::new(AtomicUsize::new(0));
    let attempts = Arc::clone(&calls);
    manager.extend("bcrypt", move || {
        attempts.fetch_add(1, Ordering::SeqCst);
        Ok(Arc::clone(&captured) as Arc<dyn Hasher>)
    })?;
    assert_eq!(Arc::strong_count(&manager), 2);
    for _ in 0..2 {
        assert!(
            matches!(manager.driver(None), Err(error) if error.code() == "hashing_driver_self_reference")
        );
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(Arc::strong_count(&manager), 1);
    drop(manager);
    assert!(witness.upgrade().is_none());
    Ok(())
}

#[test]
fn stale_self_factory_rejection_does_not_overwrite_concurrent_extension() -> Result<()> {
    use std::sync::Barrier;
    let manager = Arc::new(HashManager::new(config())?);
    let witness = Arc::downgrade(&manager);
    let captured = Arc::clone(&manager);
    let entered = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let factory_entered = Arc::clone(&entered);
    let factory_release = Arc::clone(&release);
    manager.extend("bcrypt", move || {
        factory_entered.wait();
        factory_release.wait();
        Ok(Arc::clone(&captured) as Arc<dyn Hasher>)
    })?;
    let replacement = Arc::new(bcrypt(false)?) as Arc<dyn Hasher>;
    std::thread::scope(|scope| -> Result<()> {
        let resolving = Arc::clone(&manager);
        let worker = scope.spawn(move || resolving.driver(None));
        entered.wait();
        let published = Arc::clone(&replacement);
        let extension = manager.extend("bcrypt", move || Ok(Arc::clone(&published)));
        release.wait();
        let rejected = worker
            .join()
            .map_err(|_| failure("test_worker_panicked", "测试工厂线程失败"))?;
        extension?;
        assert!(matches!(rejected, Err(error) if error.code() == "hashing_driver_self_reference"));
        assert!(Arc::ptr_eq(&replacement, &manager.driver(None)?));
        Ok(())
    })?;
    assert_eq!(Arc::strong_count(&manager), 1);
    drop(manager);
    assert!(witness.upgrade().is_none());
    Ok(())
}
