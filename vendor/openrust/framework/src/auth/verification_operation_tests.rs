//! 验证码旁路与次数记账必须先于最终驱动选择；合法码固定所选叶。

use super::{
    EncodedPasswordHash, Result, VerificationChallenge, VerificationCodeFuture,
    VerificationCodePolicy, VerificationCodeStore, VerificationCodes,
};
use crate::auth::password::operation_test_support::{Probe, count, install, live, test_error};
use crate::hashing::{HashOptions, Hasher};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct StoreState {
    challenge: Option<String>,
    attempts: Vec<bool>,
    issues: usize,
}
#[derive(Clone, Default)]
struct Store(Arc<Mutex<StoreState>>);
impl VerificationCodeStore for Store {
    fn issue<'a>(
        &'a self,
        _key: &'a str,
        _id: &'a str,
        hash: &'a EncodedPasswordHash,
        _policy: VerificationCodePolicy,
    ) -> VerificationCodeFuture<'a, Result<()>> {
        Box::pin(async move {
            let mut state = self.0.lock().map_err(|_| test_error())?;
            state.challenge = Some(hash.as_str().to_owned());
            state.issues += 1;
            Ok(())
        })
    }
    fn load_active<'a>(
        &'a self,
        _key: &'a str,
    ) -> VerificationCodeFuture<'a, Result<Option<VerificationChallenge>>> {
        Box::pin(async move {
            let hash = self.0.lock().map_err(|_| test_error())?.challenge.clone();
            hash.map(|hash| {
                VerificationChallenge::new("challenge", EncodedPasswordHash::from_encoded(hash), 6)
            })
            .transpose()
        })
    }
    fn finish_attempt<'a>(
        &'a self,
        _key: &'a str,
        id: &'a str,
        matches: bool,
    ) -> VerificationCodeFuture<'a, Result<bool>> {
        Box::pin(async move {
            assert_eq!(id, "challenge");
            self.0
                .lock()
                .map_err(|_| test_error())?
                .attempts
                .push(matches);
            Ok(matches)
        })
    }
}

#[tokio::test]
async fn missing_or_invalid_code_keeps_false_and_attempts_with_broken_default() -> Result<()> {
    let (manager, repository) = live()?;
    repository.set("hashing.driver", serde_json::json!("invalid"))?;
    let store = Store::default();
    let service = VerificationCodes::new(store.clone(), manager);
    assert!(!service.verify("email.verify", "user", "123456").await?);
    assert_eq!(
        store.0.lock().map_err(|_| test_error())?.attempts,
        [] as [bool; 0]
    );
    store.0.lock().map_err(|_| test_error())?.challenge = Some("unused-hash".into());
    for code in ["12345", "12345x", "１２３４５６"] {
        assert!(!service.verify("email.verify", "user", code).await?);
    }
    assert_eq!(
        store.0.lock().map_err(|_| test_error())?.attempts,
        vec![false, false, false]
    );
    assert!(
        matches!(service.verify("email.verify", "user", "123456").await, Err(error) if error.code() == "hashing_driver_unsupported")
    );
    assert_eq!(store.0.lock().map_err(|_| test_error())?.attempts.len(), 3);
    Ok(())
}

#[tokio::test]
async fn issue_and_valid_verify_each_select_once_and_pin_capability_switch() -> Result<()> {
    let (manager, repository) = live()?;
    let (a, ac) = Probe::new(false)?;
    let (b, bc) = Probe::new(true)?;
    install(&manager, "a", a.clone())?;
    install(&manager, "b", b)?;
    let store = Store::default();
    let service = VerificationCodes::new(store.clone(), manager);
    let issued = service
        .issue("email.verify", "user", VerificationCodePolicy::default())
        .await?;
    assert_eq!(count(&ac.selections), 1);
    assert_eq!(count(&ac.makes), 1);
    a.on_capability(move || repository.set("hashing.driver", serde_json::json!("b")))?;
    assert!(
        service
            .verify("email.verify", "user", issued.code())
            .await?
    );
    assert_eq!(count(&ac.selections), 2);
    assert_eq!(count(&ac.checks), 1);
    assert_eq!(count(&bc.selections), 0);
    assert_eq!(count(&bc.checks), 0);
    assert_eq!(
        store.0.lock().map_err(|_| test_error())?.attempts,
        vec![true]
    );
    assert!(
        service
            .verify("email.verify", "user", issued.code())
            .await?
    );
    assert_eq!(count(&bc.selections), 1);
    assert_eq!(count(&bc.checks), 1);
    Ok(())
}

struct SwitchingStore {
    repository: crate::config::repository::ConfigRepository,
    hash: String,
}
impl VerificationCodeStore for SwitchingStore {
    fn issue<'a>(
        &'a self,
        _key: &'a str,
        _id: &'a str,
        _hash: &'a EncodedPasswordHash,
        _policy: VerificationCodePolicy,
    ) -> VerificationCodeFuture<'a, Result<()>> {
        Box::pin(async { Err(test_error()) })
    }
    fn load_active<'a>(
        &'a self,
        _key: &'a str,
    ) -> VerificationCodeFuture<'a, Result<Option<VerificationChallenge>>> {
        Box::pin(async move {
            self.repository
                .set("hashing.driver", serde_json::json!("b"))?;
            Ok(Some(VerificationChallenge::new(
                "challenge",
                EncodedPasswordHash::from_encoded(self.hash.clone()),
                6,
            )?))
        })
    }
    fn finish_attempt<'a>(
        &'a self,
        _key: &'a str,
        _id: &'a str,
        matches: bool,
    ) -> VerificationCodeFuture<'a, Result<bool>> {
        Box::pin(async move { Ok(matches) })
    }
}

#[tokio::test]
async fn valid_code_uses_default_at_original_post_load_boundary() -> Result<()> {
    let (manager, repository) = live()?;
    let (a, ac) = Probe::new(false)?;
    let (b, bc) = Probe::new(false)?;
    let encoded = a.make(b"123456", &HashOptions::new()).await?;
    install(&manager, "a", a)?;
    install(&manager, "b", b)?;
    let service = VerificationCodes::new(
        SwitchingStore {
            repository,
            hash: encoded,
        },
        manager,
    );
    assert!(service.verify("email.verify", "user", "123456").await?);
    assert_eq!(count(&ac.selections), 0);
    assert_eq!(count(&bc.selections), 1);
    assert_eq!(count(&bc.checks), 1);
    Ok(())
}

#[tokio::test]
async fn issue_keeps_original_leaf_when_capability_changes_default() -> Result<()> {
    let (manager, repository) = live()?;
    let (a, ac) = Probe::new(false)?;
    let (b, bc) = Probe::new(true)?;
    install(&manager, "a", a.clone())?;
    install(&manager, "b", b)?;
    a.on_capability(move || repository.set("hashing.driver", serde_json::json!("b")))?;
    let store = Store::default();
    let service = VerificationCodes::new(store.clone(), manager);
    let issued = service
        .issue("email.verify", "user", VerificationCodePolicy::default())
        .await?;
    assert_eq!(issued.code().len(), 6);
    assert_eq!(count(&ac.selections), 1);
    assert_eq!(count(&ac.makes), 1);
    assert_eq!(count(&bc.selections), 0);
    assert_eq!(count(&bc.makes), 0);
    assert_eq!(store.0.lock().map_err(|_| test_error())?.issues, 1);
    Ok(())
}
