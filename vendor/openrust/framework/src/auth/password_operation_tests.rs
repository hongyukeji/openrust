//! 单个增强入口必须复用同一最终叶，不能把策略检查后的 live 默认变化带入 KDF。

use super::{
    EncodedPasswordHash,
    operation_test_support::{Probe, count, install, live, test_error},
};
use crate::{
    Result,
    hashing::{HashOptions, Hasher},
};
use std::sync::Arc;

#[tokio::test]
async fn public_make_pins_nested_real_argon_and_next_operation_rejects_prefix_alias() -> Result<()>
{
    let (inner, repository) = live()?;
    let (a, ac) = Probe::new(false)?;
    let (b, bc) = Probe::new(true)?;
    install(&inner, "a", a.clone())?;
    install(&inner, "b", b)?;
    let (outer, _) = live()?;
    install(&outer, "a", inner)?;
    a.on_capability(move || repository.set("hashing.driver", serde_json::json!("b")))?;
    let value = "x".repeat(73);
    let hash = EncodedPasswordHash::make(&outer, &value).await?;
    assert!(
        a.check(value.as_bytes(), Some(hash.as_str()), &HashOptions::new())
            .await?
    );
    assert!(
        !a.check(
            &value.as_bytes()[..72],
            Some(hash.as_str()),
            &HashOptions::new()
        )
        .await?
    );
    assert_eq!(count(&ac.selections), 1);
    assert_eq!(count(&ac.makes), 1);
    assert_eq!(count(&bc.makes), 0);
    assert!(
        matches!(EncodedPasswordHash::make(&outer, &value).await, Err(error) if error.code() == "password_bcrypt_input_ambiguous")
    );
    assert_eq!(count(&bc.selections), 1);
    assert_eq!(count(&bc.makes), 0);
    assert_eq!(count(&bc.checks), 0);
    Ok(())
}

#[tokio::test]
async fn each_public_entry_selects_once_and_invalid_input_never_selects() -> Result<()> {
    let (manager, repository) = live()?;
    let (probe, counts) = Probe::new(false)?;
    install(&manager, "a", probe)?;
    EncodedPasswordHash::validate_input(&manager, "password")?;
    assert_eq!(count(&counts.selections), 1);
    let hash = EncodedPasswordHash::make(&manager, "password").await?;
    assert_eq!(count(&counts.selections), 2);
    assert!(hash.verify(&manager, "password").await?);
    assert_eq!(count(&counts.selections), 3);
    assert!(!hash.needs_rehash(&manager)?);
    assert_eq!(count(&counts.selections), 4);
    assert_eq!(count(&counts.rehashes), 1);
    repository.set("hashing.driver", serde_json::json!("unsupported"))?;
    for password in [String::new(), "x".repeat(1025)] {
        assert!(matches!(
            EncodedPasswordHash::validate_input(&manager, &password),
            Err(crate::Error::BadRequest(_))
        ));
        assert!(matches!(
            EncodedPasswordHash::make(&manager, &password).await,
            Err(crate::Error::BadRequest(_))
        ));
        assert!(matches!(
            hash.verify(&manager, &password).await,
            Err(crate::Error::BadRequest(_))
        ));
    }
    assert_eq!(count(&counts.selections), 4);
    Ok(())
}

#[tokio::test]
async fn public_verify_keeps_original_leaf_after_capability_switch() -> Result<()> {
    let (manager, repository) = live()?;
    let (a, ac) = Probe::new(false)?;
    let (b, bc) = Probe::new(true)?;
    let plain = "q".repeat(73);
    let hash =
        EncodedPasswordHash::from_encoded(a.make(plain.as_bytes(), &HashOptions::new()).await?);
    install(&manager, "a", a.clone())?;
    install(&manager, "b", b)?;
    a.on_capability(move || repository.set("hashing.driver", serde_json::json!("b")))?;
    assert!(hash.verify(&manager, &plain).await?);
    assert_eq!(count(&ac.selections), 1);
    assert_eq!(count(&ac.checks), 1);
    assert_eq!(count(&bc.checks), 0);
    assert!(
        matches!(hash.verify(&manager, &plain).await, Err(error) if error.code() == "password_bcrypt_input_ambiguous")
    );
    assert_eq!(count(&bc.checks), 0);
    Ok(())
}

fn assert_send<T: Send>(_: &T) {}
#[tokio::test]
async fn public_futures_borrow_manager_and_preserve_original_custom_arc() -> Result<()> {
    let (manager, _) = live()?;
    let (custom, _) = Probe::new(false)?;
    let driver: Arc<dyn Hasher> = custom;
    install(&manager, "a", Arc::clone(&driver))?;
    assert!(Arc::ptr_eq(&manager.driver_for_operation()?, &driver));
    let plain = String::from("borrowed-password");
    let make = EncodedPasswordHash::make(&manager, &plain);
    assert_send(&make);
    let hash = make.await?;
    let verify = hash.verify(&manager, &plain);
    assert_send(&verify);
    assert!(verify.await?);
    assert!(Arc::ptr_eq(&manager.driver_for_operation()?, &driver));
    Ok(())
}

struct PermitDriver {
    real: Arc<dyn Hasher>,
    permits: Arc<tokio::sync::Semaphore>,
    wait_first: std::sync::atomic::AtomicBool,
    entered: tokio::sync::Notify,
    release: tokio::sync::Notify,
}
impl Hasher for PermitDriver {
    fn info(&self, encoded: &str) -> Result<crate::hashing::HashInfo> {
        self.real.info(encoded)
    }
    fn make<'a>(
        &'a self,
        value: &'a [u8],
        options: &'a HashOptions,
    ) -> crate::support::BoxFuture<'a, Result<String>> {
        Box::pin(async move {
            let _permit = Arc::clone(&self.permits).try_acquire_owned().map_err(|_| {
                crate::Error::custom("password_capacity_exhausted", "自定义驱动容量已占用")
            })?;
            if self
                .wait_first
                .swap(false, std::sync::atomic::Ordering::SeqCst)
            {
                self.entered.notify_one();
                self.release.notified().await;
            }
            self.real.make(value, options).await
        })
    }
    fn check<'a>(
        &'a self,
        value: &'a [u8],
        encoded: Option<&'a str>,
        options: &'a HashOptions,
    ) -> crate::support::BoxFuture<'a, Result<bool>> {
        self.real.check(value, encoded, options)
    }
    fn needs_rehash(&self, encoded: &str, options: &HashOptions) -> Result<bool> {
        self.real.needs_rehash(encoded, options)
    }
}

#[tokio::test]
async fn same_extended_custom_driver_keeps_shared_permit_across_calls_and_cancellation()
-> Result<()> {
    let (manager, _) = live()?;
    let (real, _) = Probe::new(false)?;
    let custom = Arc::new(PermitDriver {
        real,
        permits: Arc::new(tokio::sync::Semaphore::new(1)),
        wait_first: std::sync::atomic::AtomicBool::new(true),
        entered: tokio::sync::Notify::new(),
        release: tokio::sync::Notify::new(),
    });
    let weak = Arc::downgrade(&custom);
    let candidate: Arc<dyn Hasher> = custom.clone();
    install(&manager, "a", Arc::clone(&candidate))?;
    assert!(Arc::ptr_eq(&manager.driver_for_operation()?, &candidate));
    let worker_manager = Arc::clone(&manager);
    let task =
        tokio::spawn(
            async move { EncodedPasswordHash::make(&worker_manager, "first-password").await },
        );
    let entered =
        tokio::time::timeout(std::time::Duration::from_secs(5), custom.entered.notified()).await;
    if entered.is_err() {
        task.abort();
        let _cancelled = task.await;
        return Err(test_error());
    }
    let busy = EncodedPasswordHash::make(&manager, "other-password").await;
    task.abort();
    let cancelled = task.await;
    assert!(matches!(busy, Err(error) if error.code() == "password_capacity_exhausted"));
    assert!(cancelled.is_err_and(|error| error.is_cancelled()));
    assert_eq!(custom.permits.available_permits(), 1);
    let hash = EncodedPasswordHash::make(&manager, "after-cancel").await?;
    assert!(hash.verify(&manager, "after-cancel").await?);
    assert!(Arc::ptr_eq(&manager.driver_for_operation()?, &candidate));
    drop(candidate);
    drop(custom);
    drop(manager);
    assert!(weak.upgrade().is_none());
    Ok(())
}

#[test]
fn basic_input_bounds_still_accept_one_and_1024_bytes() -> Result<()> {
    let (manager, _) = live()?;
    let (probe, counts) = Probe::new(false)?;
    install(&manager, "a", probe)?;
    EncodedPasswordHash::validate_input(&manager, "x")?;
    EncodedPasswordHash::validate_input(&manager, &"x".repeat(1024))?;
    assert_eq!(count(&counts.selections), 2);
    assert_eq!(count(&counts.makes), 0);
    Ok(())
}
