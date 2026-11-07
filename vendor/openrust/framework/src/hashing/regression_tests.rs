//! 独审回归：计算前预算、格式边界、比较语义与实际阻塞任务取消。

use super::*;
use crate::core::Result;
use argon2::Params;
use std::sync::Arc;

fn cheap_bcrypt(verify: bool) -> Result<BcryptHasher> {
    BcryptHasher::new(BcryptConfig {
        rounds: 4,
        verify,
        limit: None,
    })
}
fn cheap_argon(verify: bool) -> ArgonConfig {
    ArgonConfig {
        memory: 1024,
        time: 1,
        threads: 1,
        verify,
    }
}
fn limit_error<T>(result: Result<T>) -> bool {
    matches!(result, Err(error) if error.code() == "hashing_execution_limit_exceeded")
}

#[test]
fn execution_limits_are_explicit_and_do_not_compute_kdf_at_boundaries() -> Result<()> {
    let limits = ExecutionLimits::default();
    assert!(limits.bcrypt(16).is_ok());
    assert!(limit_error(limits.bcrypt(17)));
    assert!(
        limits
            .argon(&Params::new(262_144, 10, 4, None).map_err(crate::core::Error::internal)?)
            .is_ok()
    );
    for params in [
        Params::new(262_145, 1, 1, None),
        Params::new(1024, 11, 1, None),
        Params::new(1024, 1, 5, None),
    ] {
        assert!(limit_error(
            limits.argon(&params.map_err(crate::core::Error::internal)?)
        ));
    }
    let raised = ExecutionLimits {
        max_bcrypt_rounds: 31,
        max_argon_memory: 262_145,
        ..limits
    };
    assert!(raised.bcrypt(31).is_ok());
    assert!(
        raised
            .argon(&Params::new(262_145, 1, 1, None).map_err(crate::core::Error::internal)?)
            .is_ok()
    );
    Ok(())
}

#[tokio::test]
async fn make_checks_budget_before_entering_any_expensive_algorithm() -> Result<()> {
    let options = HashOptions::new();
    let bcrypt = BcryptHasher::with_execution_limits(
        BcryptConfig {
            rounds: 4,
            ..BcryptConfig::default()
        },
        ExecutionLimits {
            max_bcrypt_rounds: 3,
            ..ExecutionLimits::default()
        },
    )?;
    assert!(limit_error(bcrypt.make("test".as_bytes(), &options).await));
    let limits = ExecutionLimits {
        max_argon_memory: 512,
        ..ExecutionLimits::default()
    };
    assert!(limit_error(
        ArgonHasher::with_execution_limits(cheap_argon(false), limits.clone())?
            .make("test".as_bytes(), &options)
            .await
    ));
    assert!(limit_error(
        Argon2IdHasher::with_execution_limits(cheap_argon(false), limits)?
            .make("test".as_bytes(), &options)
            .await
    ));
    assert!(
        Argon2IdHasher::with_execution_limits(
            cheap_argon(false),
            ExecutionLimits {
                max_argon_memory: 1024,
                ..ExecutionLimits::default()
            }
        )?
        .make("test".as_bytes(), &options)
        .await
        .is_ok()
    );
    Ok(())
}

#[tokio::test]
async fn stored_cost_budget_applies_to_strict_and_cross_algorithm_verification() -> Result<()> {
    let options = HashOptions::new();
    let bcrypt_hash = cheap_bcrypt(false)?
        .make("test".as_bytes(), &options)
        .await?;
    let bcrypt_high = bcrypt_hash.replacen("$04$", "$17$", 1);
    let drivers: Vec<Arc<dyn Hasher>> = vec![
        Arc::new(cheap_bcrypt(false)?),
        Arc::new(ArgonHasher::new(cheap_argon(false))?),
        Arc::new(Argon2IdHasher::new(cheap_argon(false))?),
    ];
    for driver in &drivers {
        assert!(limit_error(
            driver
                .check("test".as_bytes(), Some(&bcrypt_high), &options)
                .await
        ));
    }
    assert!(limit_error(
        cheap_bcrypt(true)?
            .check("test".as_bytes(), Some(&bcrypt_high), &options)
            .await
    ));
    for (maker, strict) in [
        (
            Arc::new(ArgonHasher::new(cheap_argon(false))?) as Arc<dyn Hasher>,
            Arc::new(ArgonHasher::new(cheap_argon(true))?) as Arc<dyn Hasher>,
        ),
        (
            Arc::new(Argon2IdHasher::new(cheap_argon(false))?) as Arc<dyn Hasher>,
            Arc::new(Argon2IdHasher::new(cheap_argon(true))?) as Arc<dyn Hasher>,
        ),
    ] {
        let encoded = maker.make("test".as_bytes(), &options).await?;
        for changed in [
            encoded.replacen("m=1024", "m=262145", 1),
            encoded.replacen("t=1", "t=11", 1),
            encoded.replacen("p=1", "p=5", 1),
        ] {
            for driver in &drivers {
                assert!(limit_error(
                    driver
                        .check("test".as_bytes(), Some(&changed), &options)
                        .await
                ));
            }
            assert!(limit_error(
                strict
                    .check("test".as_bytes(), Some(&changed), &options)
                    .await
            ));
        }
    }
    Ok(())
}

#[tokio::test]
async fn bcrypt_check_requires_two_ascii_cost_digits_even_when_info_recognizes_it() -> Result<()> {
    let options = HashOptions::new();
    let hash = cheap_bcrypt(false)?
        .make("test".as_bytes(), &options)
        .await?;
    let altered = hash.replacen("$04$", "$+4$", 1);
    for verify in [true, false] {
        let driver = cheap_bcrypt(verify)?;
        assert_eq!(driver.info(&altered)?.algo_name, "bcrypt");
        assert!(
            driver
                .check("test".as_bytes(), Some(&hash), &options)
                .await?
        );
        assert!(
            !driver
                .check("test".as_bytes(), Some(&altered), &options)
                .await?
        );
    }
    Ok(())
}

#[tokio::test]
async fn rehash_comparison_uses_php_scalar_casts_without_make_range_checks() -> Result<()> {
    let driver = cheap_bcrypt(false)?;
    let options = HashOptions::new();
    let hash = driver.make("test".as_bytes(), &options).await?;
    for rounds in [
        serde_json::json!(1),
        serde_json::json!("1"),
        serde_json::json!(true),
        serde_json::json!(-1),
        serde_json::json!([]),
    ] {
        let changed = HashOptions::from([("rounds".into(), rounds)]);
        assert!(driver.needs_rehash(&hash, &changed)?);
        assert!(driver.make("test".as_bytes(), &changed).await.is_err());
    }
    for rounds in [
        serde_json::json!("4"),
        serde_json::json!("4suffix"),
        serde_json::json!(4.9),
    ] {
        let same = HashOptions::from([("rounds".into(), rounds)]);
        assert!(!driver.needs_rehash(&hash, &same)?);
        assert!(driver.make("test".as_bytes(), &same).await.is_ok());
    }
    let configured_invalid = BcryptHasher::new(BcryptConfig {
        rounds: 1,
        ..BcryptConfig::default()
    })?;
    assert!(configured_invalid.needs_rehash(&hash, &options)?);
    assert!(
        configured_invalid
            .make("test".as_bytes(), &options)
            .await
            .is_err()
    );
    assert!(
        configured_invalid
            .make(
                "test".as_bytes(),
                &HashOptions::from([("rounds".into(), 4.into())])
            )
            .await
            .is_ok()
    );
    assert!(configured_invalid.needs_rehash(
        "unknown",
        &HashOptions::from([("rounds".into(), serde_json::json!({"anything":1}))])
    )?);
    Ok(())
}

#[tokio::test]
async fn limit_error_precedes_invalid_make_cost_and_metadata_remains_unrestricted() -> Result<()> {
    let driver = BcryptHasher::new(BcryptConfig {
        rounds: 1,
        limit: Some(1),
        verify: false,
    })?;
    assert!(
        matches!(driver.make("too long".as_bytes(), &HashOptions::new()).await, Err(error) if error.code() == "hashing_value_too_long")
    );
    let hash = cheap_bcrypt(false)?
        .make("test".as_bytes(), &HashOptions::new())
        .await?
        .replacen("$04$", "$17$", 1);
    let bounded = cheap_bcrypt(false)?;
    assert_eq!(bounded.info(&hash)?.options.get("cost"), Some(&17.into()));
    assert!(bounded.needs_rehash(&hash, &HashOptions::new())?);
    let allowed_metadata = BcryptHasher::with_execution_limits(
        BcryptConfig {
            rounds: 17,
            ..BcryptConfig::default()
        },
        ExecutionLimits {
            max_bcrypt_rounds: 4,
            ..ExecutionLimits::default()
        },
    )?;
    assert!(allowed_metadata.verify_configuration(&hash)?);
    assert!(!allowed_metadata.needs_rehash(&hash, &HashOptions::new())?);
    Ok(())
}

#[test]
fn info_and_rehash_have_different_argon_missing_parameter_defaults() -> Result<()> {
    let driver = Argon2IdHasher::new(HashingConfig::default().argon)?;
    let garbage = "$argon2id$garbage";
    assert_eq!(
        driver.info(garbage)?.options.get("memory_cost"),
        Some(&65_536.into())
    );
    assert!(driver.needs_rehash(garbage, &HashOptions::new())?);
    for (encoded, expected) in [
        ("$argon2id$v=19$m=1024oops", [1024, 4, 1]),
        ("$argon2id$v=19$m=-1,t=+2,p=3x", [-1, 2, 3]),
    ] {
        let info = driver.info(encoded)?;
        assert_eq!(info.options.get("memory_cost"), Some(&expected[0].into()));
        assert_eq!(info.options.get("time_cost"), Some(&expected[1].into()));
        assert_eq!(info.options.get("threads"), Some(&expected[2].into()));
    }
    let suffix = "a".repeat(53);
    let bcrypt = cheap_bcrypt(false)?;
    for (cost, expected) in [("zz", 12), ("4x", 4), ("+4", 4), ("-1", -1)] {
        assert_eq!(
            bcrypt
                .info(&format!("$2y${cost}${suffix}"))?
                .options
                .get("cost"),
            Some(&expected.into())
        );
    }
    Ok(())
}

#[test]
fn extension_name_conversion_can_reenter_manager_before_state_lock() -> Result<()> {
    use std::sync::atomic::{AtomicBool, Ordering};
    struct Name {
        manager: Arc<HashManager>,
        observed: Arc<AtomicBool>,
    }
    impl From<Name> for String {
        fn from(name: Name) -> Self {
            name.observed.store(
                name.manager.driver(Some("bcrypt")).is_ok(),
                Ordering::SeqCst,
            );
            "custom".into()
        }
    }
    let manager = Arc::new(HashManager::new(HashingConfig::default())?);
    let observed = Arc::new(AtomicBool::new(false));
    manager.extend(
        Name {
            manager: Arc::clone(&manager),
            observed: Arc::clone(&observed),
        },
        || Ok(Arc::new(cheap_bcrypt(false)?) as Arc<dyn Hasher>),
    )?;
    assert!(observed.load(Ordering::SeqCst));
    manager.driver(Some("custom"))?;
    Ok(())
}

struct Finished(Option<tokio::sync::oneshot::Sender<()>>);
impl Drop for Finished {
    fn drop(&mut self) {
        if let Some(sender) = self.0.take() {
            let _ = sender.send(());
        }
    }
}

#[tokio::test]
async fn started_blocking_operation_keeps_capacity_after_caller_is_aborted() -> Result<()> {
    let pool = drivers::OperationPool::new(1)?;
    let worker_pool = pool.clone();
    let (entered_sender, entered) = tokio::sync::oneshot::channel();
    let (released, release) = std::sync::mpsc::channel();
    let (finished_sender, finished) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        worker_pool
            .run(move || {
                entered_sender
                    .send(())
                    .map_err(|()| failure("test_signal", "测试进入信号失效"))?;
                release
                    .recv()
                    .map_err(|_| failure("test_signal", "测试释放信号失效"))?;
                Ok(Finished(Some(finished_sender)))
            })
            .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(5), entered)
        .await
        .map_err(|_| failure("test_timeout", "阻塞工作未进入"))?
        .map_err(|_| failure("test_signal", "测试进入等待失效"))?;
    task.abort();
    let cancelled = task.await;
    let busy = pool.run(|| Ok(())).await;
    released
        .send(())
        .map_err(|_| failure("test_signal", "测试释放失败"))?;
    assert!(cancelled.is_err_and(|error| error.is_cancelled()));
    assert!(matches!(busy, Err(error) if error.code() == "password_capacity_exhausted"));
    // Finished作为blocking任务输出在permit归还后才释放，不靠sleep猜测完成。
    tokio::time::timeout(std::time::Duration::from_secs(5), finished)
        .await
        .map_err(|_| failure("test_timeout", "阻塞工作未结束"))?
        .map_err(|_| failure("test_signal", "测试完成信号失效"))?;
    assert!(pool.run(|| Ok(())).await.is_ok());
    Ok(())
}

#[tokio::test]
async fn exact_input_capability_does_not_change_official_bcrypt_alias_verification() -> Result<()> {
    let driver = cheap_bcrypt(false)?;
    let options = HashOptions::new();
    let boundary = "a".repeat(72);
    let extended = format!("{boundary}x");
    assert!(driver.accepts_exact_input(boundary.as_bytes())?);
    assert!(!driver.accepts_exact_input(extended.as_bytes())?);
    assert!(!driver.accepts_exact_input("abc\0tail".as_bytes())?);
    let long_hash = driver.make(extended.as_bytes(), &options).await?;
    assert!(
        driver
            .check(boundary.as_bytes(), Some(&long_hash), &options)
            .await?
    );
    assert!(
        driver
            .check(extended.as_bytes(), Some(&long_hash), &options)
            .await?
    );
    let short_hash = driver.make("abc".as_bytes(), &options).await?;
    assert!(
        driver
            .check("abc\0tail".as_bytes(), Some(&short_hash), &options)
            .await?
    );
    for hasher in [
        Arc::new(ArgonHasher::new(cheap_argon(false))?) as Arc<dyn Hasher>,
        Arc::new(Argon2IdHasher::new(cheap_argon(false))?) as Arc<dyn Hasher>,
    ] {
        assert!(!hasher.accepts_exact_input(extended.as_bytes())?);
        assert!(!hasher.accepts_exact_input("abc\0tail".as_bytes())?);
        assert!(hasher.accepts_exact_input(boundary.as_bytes())?);
        assert!(
            hasher
                .check(extended.as_bytes(), Some(&long_hash), &options)
                .await?
        );
        assert!(
            hasher
                .check("abc\0tail".as_bytes(), Some(&short_hash), &options)
                .await?
        );
    }
    Ok(())
}

#[test]
fn strict_argon_capability_preserves_full_bytes_for_both_algorithms() -> Result<()> {
    for hasher in [
        Arc::new(ArgonHasher::new(cheap_argon(true))?) as Arc<dyn Hasher>,
        Arc::new(Argon2IdHasher::new(cheap_argon(true))?) as Arc<dyn Hasher>,
    ] {
        assert!(hasher.accepts_exact_input("abc\0tail".as_bytes())?);
        assert!(hasher.accepts_exact_input("测".repeat(25).as_bytes())?);
        assert!(hasher.accepts_exact_input("a".repeat(73).as_bytes())?);
    }
    Ok(())
}

struct DefaultInputCapability(BcryptHasher);
impl Hasher for DefaultInputCapability {
    fn info(&self, encoded: &str) -> Result<HashInfo> {
        self.0.info(encoded)
    }
    fn make<'a>(
        &'a self,
        value: &'a [u8],
        options: &'a HashOptions,
    ) -> crate::BoxFuture<'a, Result<String>> {
        self.0.make(value, options)
    }
    fn check<'a>(
        &'a self,
        value: &'a [u8],
        encoded: Option<&'a str>,
        options: &'a HashOptions,
    ) -> crate::BoxFuture<'a, Result<bool>> {
        self.0.check(value, encoded, options)
    }
    fn needs_rehash(&self, encoded: &str, options: &HashOptions) -> Result<bool> {
        self.0.needs_rehash(encoded, options)
    }
}

#[test]
fn custom_hasher_keeps_default_input_capability_and_manager_propagates_errors() -> Result<()> {
    let custom = DefaultInputCapability(cheap_bcrypt(false)?);
    assert!(custom.accepts_exact_input("abc\0tail".as_bytes())?);
    assert!(custom.accepts_exact_input("a".repeat(73).as_bytes())?);
    let manager = HashManager::new(HashingConfig {
        driver: "missing".into(),
        ..HashingConfig::default()
    })?;
    assert!(
        matches!(manager.accepts_exact_input("abc"), Err(error) if error.code() == "hashing_driver_unsupported")
    );
    let manager_trait: &dyn Hasher = &manager;
    assert!(
        matches!(manager_trait.accepts_exact_input("abc".as_bytes()), Err(error) if error.code() == "hashing_driver_unsupported")
    );
    Ok(())
}
