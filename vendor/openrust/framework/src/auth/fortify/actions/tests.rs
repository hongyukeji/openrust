//! 固定动作默认、秘密边界及并发条件更新回归。

use super::*;
use crate::auth::Authenticatable;
use crate::auth::GenericUser;
use crate::encryption::EncryptionConfig;
use crate::events::EventsServiceProvider;
use crate::foundation::ProviderBuilder;
use crate::support::BoxFuture;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

const SECRET: &str = "JBSWY3DPEHPK3PXP";

type ModelPause = (Arc<tokio::sync::Notify>, Arc<tokio::sync::Notify>);

struct Model {
    state: Mutex<TwoFactorState>,
    writes: AtomicUsize,
    encrypter: Option<Encrypter>,
    load_pause: Mutex<Option<ModelPause>>,
}
impl Model {
    fn new(encrypter: Option<Encrypter>) -> Self {
        Self {
            state: Mutex::new(TwoFactorState::default()),
            writes: AtomicUsize::new(0),
            encrypter,
            load_pause: Mutex::new(None),
        }
    }
    fn snapshot(&self) -> TwoFactorState {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}
impl Authenticatable for Model {
    fn as_two_factor(&self) -> Option<&dyn TwoFactorAuthenticatable> {
        Some(self)
    }
    fn auth_identifier_name(&self) -> &'static str {
        "uuid"
    }
    fn auth_identifier(&self) -> Result<Value> {
        Ok(json!("account-uuid"))
    }
    fn auth_password_name(&self) -> &'static str {
        "password"
    }
    fn auth_password(&self) -> Result<Option<String>> {
        Ok(None)
    }
    fn remember_token(&self) -> Result<Option<String>> {
        Ok(None)
    }
    fn remember_token_for_cookie(&self) -> Result<Option<String>> {
        Ok(None)
    }
    fn set_remember_token(&self, _: Option<String>) -> crate::support::BoxFuture<'_, Result<()>> {
        Box::pin(async { Ok(()) })
    }
    fn remember_token_name(&self) -> &'static str {
        "remember_token"
    }
}
impl TwoFactorAuthenticatable for Model {
    fn two_factor_state(&self) -> BoxFuture<'_, Result<TwoFactorState>> {
        Box::pin(async move {
            let pause = self
                .load_pause
                .lock()
                .map_err(|_| Error::Unauthorized)?
                .take();
            if let Some((entered, resume)) = pause {
                entered.notify_one();
                resume.notified().await;
            }
            Ok(self.snapshot())
        })
    }
    fn compare_and_set_two_factor_state<'a>(
        &'a self,
        expected: &'a TwoFactorState,
        replacement: &'a TwoFactorState,
    ) -> BoxFuture<'a, Result<bool>> {
        Box::pin(async move {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if *state != *expected {
                return Ok(false);
            }
            *state = replacement.clone();
            self.writes.fetch_add(1, Ordering::SeqCst);
            Ok(true)
        })
    }
    fn two_factor_account_name(&self, username: &str) -> Result<Option<String>> {
        Ok(Some(format!("{username}-account")))
    }
    fn two_factor_encrypter(&self) -> Option<Encrypter> {
        self.encrypter.clone()
    }
}

#[derive(Default)]
struct Provider {
    generations: AtomicUsize,
    verifications: AtomicUsize,
    fail: std::sync::atomic::AtomicBool,
    pause: Option<(Arc<tokio::sync::Notify>, Arc<tokio::sync::Notify>)>,
}
impl TwoFactorAuthenticationProvider for Provider {
    fn generate_secret_key(&self, length: usize) -> Result<String> {
        assert_eq!(length, 16);
        self.generations.fetch_add(1, Ordering::SeqCst);
        Ok(SECRET.into())
    }
    fn qr_code_url(&self, company: &str, account: &str, secret: &str) -> Result<String> {
        Ok(format!("{company}/{account}/{secret}"))
    }
    fn verify_with_context<'a>(
        &'a self,
        secret: &'a str,
        code: &'a str,
    ) -> BoxFuture<'a, Result<super::super::TwoFactorCodeVerification>> {
        Box::pin(async move {
            self.verifications.fetch_add(1, Ordering::SeqCst);
            if self.fail.load(Ordering::SeqCst) {
                return Err(Error::custom("provider_context_failed", "测试验证器失败"));
            }
            assert_eq!(secret, SECRET);
            if let Some((entered, resume)) = &self.pause {
                entered.notify_one();
                resume.notified().await;
            }
            if code == "004201" {
                super::super::TwoFactorCodeVerification::verified(code, 1, 100, 100)
            } else {
                Ok(super::super::TwoFactorCodeVerification::rejected())
            }
        })
    }
}
fn encryptor(byte: u8) -> Result<Encrypter> {
    Encrypter::new(EncryptionConfig::new(
        "test",
        std::collections::BTreeMap::from([("test".into(), STANDARD.encode([byte; 32]))]),
    ))
}
async fn events() -> Result<(Events, Arc<AtomicUsize>, Arc<AtomicUsize>)> {
    let enabled = Arc::new(AtomicUsize::new(0));
    let disabled = Arc::new(AtomicUsize::new(0));
    let mut provider = EventsServiceProvider::new();
    let count = enabled.clone();
    provider.listen::<TwoFactorAuthenticationEnabled, _>(move |_, _| {
        let count = count.clone();
        Box::pin(async move {
            count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
    });
    let count = disabled.clone();
    provider.listen::<TwoFactorAuthenticationDisabled, _>(move |_, _| {
        let count = count.clone();
        Box::pin(async move {
            count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
    });
    let mut builder = ProviderBuilder::new();
    builder.provider(provider);
    let runtime = builder.start(figment::Figment::new()).await?;
    Ok((
        (*runtime.container().resolve::<Events>().await?).clone(),
        enabled,
        disabled,
    ))
}
fn account(model: &Arc<Model>) -> AuthUser {
    model.clone()
}

#[tokio::test]
async fn enable_is_idempotent_force_preserves_confirmation_and_dispatches_only_writes() -> Result<()>
{
    let (events, enabled, _) = events().await?;
    let provider = Arc::new(Provider::default());
    let actions = TwoFactorActions::new(provider.clone(), encryptor(1)?, events);
    let model = Arc::new(Model::new(None));
    let user = account(&model);
    actions.enable(&user, false).await?;
    assert_eq!(actions.recovery_codes(&user).await?.len(), 8);
    actions.confirm(&user, "004201").await?;
    let confirmed = model.snapshot();
    actions.enable(&user, false).await?;
    assert_eq!(model.snapshot(), confirmed);
    assert_eq!(provider.generations.load(Ordering::SeqCst), 1);
    actions.enable(&user, true).await?;
    assert_eq!(model.snapshot().confirmed_at, confirmed.confirmed_at);
    assert_ne!(model.snapshot().secret, confirmed.secret);
    assert_eq!(provider.generations.load(Ordering::SeqCst), 2);
    assert_eq!(enabled.load(Ordering::SeqCst), 2);
    Ok(())
}

#[tokio::test]
async fn recovery_regeneration_keeps_secret_and_confirmation_and_remains_readable() -> Result<()> {
    let (events, _, _) = events().await?;
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let actions = TwoFactorActions::new(Arc::new(Provider::default()), encryptor(2)?, events)
        .with_recovery_code_generator(Arc::new(move || {
            Ok(format!(
                "custom-recovery-{}",
                counter.fetch_add(1, Ordering::SeqCst)
            ))
        }));
    let model = Arc::new(Model::new(None));
    let user = account(&model);
    actions.enable(&user, false).await?;
    actions.confirm(&user, "004201").await?;
    let before = model.snapshot();
    let old = actions.recovery_codes(&user).await?;
    assert_eq!(&*old, &*actions.recovery_codes(&user).await?);
    actions.generate_new_recovery_codes(&user).await?;
    let after = model.snapshot();
    let new = actions.recovery_codes(&user).await?;
    assert_eq!(after.secret, before.secret);
    assert_eq!(after.confirmed_at, before.confirmed_at);
    assert_eq!(old.len(), 8);
    assert_eq!(new.len(), 8);
    assert_ne!(*old, *new);
    assert_eq!(calls.load(Ordering::SeqCst), 16);
    Ok(())
}

#[tokio::test]
async fn confirmation_failures_keep_named_bag_and_never_write() -> Result<()> {
    let (events, _, _) = events().await?;
    let actions = TwoFactorActions::new(Arc::new(Provider::default()), encryptor(3)?, events);
    let model = Arc::new(Model::new(None));
    let user = account(&model);
    let missing = actions.confirm(&user, "004201").await;
    assert!(missing.is_err());
    if let Err(error) = missing {
        assert_eq!(
            error.validation_error_bag(),
            Some("confirmTwoFactorAuthentication")
        );
    }
    actions.enable(&user, false).await?;
    let before = model.snapshot();
    for code in ["", "bad-code"] {
        let result = actions.confirm(&user, code).await;
        assert!(result.is_err());
        if let Err(error) = result {
            assert_eq!(
                error.validation_error_bag(),
                Some("confirmTwoFactorAuthentication")
            );
        }
        assert_eq!(model.snapshot(), before);
    }
    assert_eq!(model.writes.load(Ordering::SeqCst), 1);
    actions.confirm(&user, "004201").await?;
    assert!(model.snapshot().confirmed_at.is_some());
    Ok(())
}

#[tokio::test]
async fn disable_clears_all_fields_once_and_empty_account_is_noop() -> Result<()> {
    let (events, _, disabled) = events().await?;
    let actions = TwoFactorActions::new(Arc::new(Provider::default()), encryptor(4)?, events);
    let model = Arc::new(Model::new(None));
    let user = account(&model);
    actions.disable(&user).await?;
    assert_eq!(model.writes.load(Ordering::SeqCst), 0);
    actions.enable(&user, false).await?;
    actions.confirm(&user, "004201").await?;
    actions.disable(&user).await?;
    let writes = model.writes.load(Ordering::SeqCst);
    actions.disable(&user).await?;
    assert_eq!(model.snapshot(), TwoFactorState::default());
    assert_eq!(disabled.load(Ordering::SeqCst), 1);
    assert_eq!(model.writes.load(Ordering::SeqCst), writes);
    Ok(())
}

#[tokio::test]
async fn encryption_priority_is_explicit_then_model_then_default() -> Result<()> {
    for mode in 0..3 {
        let default = encryptor(10)?;
        let model_key = encryptor(11)?;
        let explicit = encryptor(12)?;
        let model = Arc::new(Model::new((mode != 0).then_some(model_key.clone())));
        let user = account(&model);
        let (events, _, _) = events().await?;
        let mut actions =
            TwoFactorActions::new(Arc::new(Provider::default()), default.clone(), events);
        if mode == 2 {
            actions = actions.with_encrypter(explicit.clone());
        }
        actions.enable(&user, false).await?;
        let state = model.snapshot();
        let cipher = state
            .secret
            .as_deref()
            .ok_or_else(|| Error::custom("test_missing", "missing secret"))?;
        let selected = match mode {
            0 => &default,
            1 => &model_key,
            _ => &explicit,
        };
        assert_eq!(selected.decrypt(cipher)?, SECRET.as_bytes());
        for (index, key) in [&default, &model_key, &explicit].into_iter().enumerate() {
            if index != mode {
                assert!(key.decrypt(cipher).is_err());
            }
        }
        assert_eq!(actions.secret_key(&user).await?.as_str(), SECRET);
        assert_eq!(actions.recovery_codes(&user).await?.len(), 8);
        actions.confirm(&user, "004201").await?;
    }
    Ok(())
}

#[tokio::test]
async fn concurrent_disable_cannot_be_undone_by_inflight_confirmation() -> Result<()> {
    let entered = Arc::new(tokio::sync::Notify::new());
    let resume = Arc::new(tokio::sync::Notify::new());
    let provider = Arc::new(Provider {
        generations: AtomicUsize::new(0),
        pause: Some((entered.clone(), resume.clone())),
        ..Provider::default()
    });
    let (events, _, _) = events().await?;
    let actions = TwoFactorActions::new(provider, encryptor(5)?, events);
    let model = Arc::new(Model::new(None));
    let user = account(&model);
    actions.enable(&user, false).await?;
    let pending_actions = actions.clone();
    let pending_user = user.clone();
    let pending =
        tokio::spawn(async move { pending_actions.confirm(&pending_user, "004201").await });
    tokio::time::timeout(std::time::Duration::from_secs(5), entered.notified())
        .await
        .map_err(Error::internal)?;
    actions.disable(&user).await?;
    resume.notify_one();
    let result = tokio::time::timeout(std::time::Duration::from_secs(5), pending)
        .await
        .map_err(Error::internal)?
        .map_err(Error::internal)?;
    assert!(matches!(result, Err(Error::Conflict(_))));
    assert_eq!(model.snapshot(), TwoFactorState::default());
    Ok(())
}

#[tokio::test]
async fn recovery_generator_error_does_not_partially_write_account() -> Result<()> {
    let (events, enabled, _) = events().await?;
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let actions = TwoFactorActions::new(Arc::new(Provider::default()), encryptor(6)?, events)
        .with_recovery_code_generator(Arc::new(move || {
            if count.fetch_add(1, Ordering::SeqCst) == 3 {
                Err(Error::custom("entropy_failed", "entropy unavailable"))
            } else {
                Ok("private-recovery-value".into())
            }
        }));
    let model = Arc::new(Model::new(None));
    let user = account(&model);
    assert!(actions.enable(&user, false).await.is_err());
    assert_eq!(model.snapshot(), TwoFactorState::default());
    assert_eq!(model.writes.load(Ordering::SeqCst), 0);
    assert_eq!(enabled.load(Ordering::SeqCst), 0);
    Ok(())
}

#[tokio::test]
async fn ordinary_user_without_capability_is_rejected_and_debug_redacts_secrets() -> Result<()> {
    let (events, _, _) = events().await?;
    let actions = TwoFactorActions::new(Arc::new(Provider::default()), encryptor(7)?, events);
    let user: AuthUser = Arc::new(GenericUser::new(serde_json::Map::from_iter([(
        "id".into(),
        json!(1),
    )])));
    assert!(actions.enable(&user, false).await.is_err());
    assert!(actions.confirm(&user, "004201").await.is_err());
    assert!(actions.disable(&user).await.is_err());
    let code = RecoveryCode::new("private-recovery-value".into());
    assert_eq!(code.expose(), "private-recovery-value");
    let event = RecoveryCodeReplaced { user, code };
    let debug = format!("{event:?} {actions:?}");
    assert!(!debug.contains("private-recovery-value"));
    assert!(!debug.contains(SECRET));
    assert!(debug.contains("REDACTED"));
    Ok(())
}

#[test]
fn default_recovery_code_has_exact_official_shape() -> Result<()> {
    for _ in 0..32 {
        let code = generate_recovery_code()?;
        assert_eq!(code.len(), 21);
        assert_eq!(code.as_bytes()[10], b'-');
        assert!(
            code.bytes()
                .enumerate()
                .all(|(index, byte)| index == 10 || byte.is_ascii_alphanumeric())
        );
    }
    Ok(())
}

#[tokio::test]
async fn falsey_confirm_code_skips_context_provider_and_context_errors_do_not_write() -> Result<()>
{
    let (events, _, _) = events().await?;
    let provider = Arc::new(Provider::default());
    let actions = TwoFactorActions::new(provider.clone(), encryptor(7)?, events);
    let model = Arc::new(Model::new(None));
    let user = account(&model);
    actions.enable(&user, false).await?;
    let before = model.snapshot();
    for code in ["", "0"] {
        assert!(actions.confirm(&user, code).await.is_err());
    }
    assert_eq!(provider.verifications.load(Ordering::SeqCst), 0);
    {
        let mut state = model
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.secret = Some("malformed-ciphertext-for-falsey-input".into());
    }
    for code in ["", "0"] {
        assert!(matches!(actions.confirm(&user, code).await,
            Err(Error::ValidationInBag { bag, .. }) if bag == "confirmTwoFactorAuthentication"));
    }
    assert_eq!(provider.verifications.load(Ordering::SeqCst), 0);
    *model
        .state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = before.clone();
    provider.fail.store(true, Ordering::SeqCst);
    assert!(
        actions
            .confirm(&user, "004201")
            .await
            .err()
            .is_some_and(|error| error.code() == "provider_context_failed")
    );
    assert_eq!(provider.verifications.load(Ordering::SeqCst), 1);
    assert_eq!(model.snapshot(), before);
    Ok(())
}

#[tokio::test]
async fn zero_secret_is_php_empty_for_enable_and_confirmation_without_provider_or_decrypt()
-> Result<()> {
    let (events, enabled, _) = events().await?;
    let provider = Arc::new(Provider::default());
    provider.fail.store(true, Ordering::SeqCst);
    let crypto = encryptor(11)?;
    let actions = TwoFactorActions::new(provider.clone(), crypto.clone(), events);
    let model = Arc::new(Model::new(None));
    model.state.lock().map_err(|_| Error::Unauthorized)?.secret = Some("0".into());
    let user = account(&model);
    let error = actions
        .confirm(&user, "004201")
        .await
        .err()
        .ok_or(Error::Unauthorized)?;
    assert_eq!(
        error.validation_error_bag(),
        Some("confirmTwoFactorAuthentication")
    );
    assert_eq!(provider.verifications.load(Ordering::SeqCst), 0);
    assert_eq!(model.writes.load(Ordering::SeqCst), 0);
    assert_eq!(model.snapshot().secret.as_deref(), Some("0"));
    actions.enable(&user, false).await?;
    assert_eq!(provider.generations.load(Ordering::SeqCst), 1);
    assert_eq!(model.writes.load(Ordering::SeqCst), 1);
    assert_eq!(enabled.load(Ordering::SeqCst), 1);
    let state = model.snapshot();
    assert_eq!(
        crypto
            .decrypt(state.secret.as_deref().ok_or(Error::Unauthorized)?)?
            .len(),
        16
    );
    Ok(())
}

#[derive(Default)]
struct LengthProvider {
    lengths: Mutex<Vec<usize>>,
}
impl TwoFactorAuthenticationProvider for LengthProvider {
    fn generate_secret_key(&self, length: usize) -> Result<String> {
        self.lengths
            .lock()
            .map_err(|_| Error::Unauthorized)?
            .push(length);
        super::super::GoogleTwoFactorAuthenticationProvider::new(None).generate_secret_key(length)
    }
    fn qr_code_url(&self, company: &str, account: &str, secret: &str) -> Result<String> {
        super::super::GoogleTwoFactorAuthenticationProvider::new(None)
            .qr_code_url(company, account, secret)
    }
    fn verify_with_context<'a>(
        &'a self,
        _secret: &'a str,
        _code: &'a str,
    ) -> BoxFuture<'a, Result<super::super::TwoFactorCodeVerification>> {
        Box::pin(async { Ok(super::super::TwoFactorCodeVerification::rejected()) })
    }
}
const SECRET_LENGTH_KEY: &str = "fortify-options.two-factor-authentication.secret-length";
fn length_repository() -> Result<crate::config::repository::ConfigRepository> {
    let repository = crate::config::repository::ConfigRepository::new(figment::Figment::new());
    repository.set("fortify.features", json!(["two-factor-authentication"]))?;
    repository.set(SECRET_LENGTH_KEY, json!(16))?;
    Ok(repository)
}
async fn resolved_actions(actions: TwoFactorActions) -> Result<Arc<TwoFactorActions>> {
    let mut builder = crate::container::ContainerBuilder::new();
    builder.instance(Arc::new(actions));
    let container = builder.build();
    let resolved = container.resolve::<TwoFactorActions>().await?;
    assert!(Arc::ptr_eq(
        &resolved,
        &container.resolve::<TwoFactorActions>().await?
    ));
    Ok(resolved)
}

#[tokio::test]
async fn resolved_actions_read_current_php_integer_length_only_for_enable_or_force() -> Result<()> {
    let (events, _, _) = events().await?;
    let provider = Arc::new(LengthProvider::default());
    let repository = length_repository()?;
    let actions = resolved_actions(
        TwoFactorActions::new(provider.clone(), encryptor(12)?, events)
            .with_feature_enabled(false)
            .with_options(TwoFactorOptions {
                secret_length: 64,
                ..TwoFactorOptions::default()
            })
            .with_config_repository(repository.clone()),
    )
    .await?;
    assert!(actions.feature_enabled()?);
    repository.set("fortify.features", json!([]))?;
    assert!(!actions.feature_enabled()?);
    repository.set("fortify.features", json!(["two-factor-authentication"]))?;
    let model = Arc::new(Model::new(None));
    let user = account(&model);
    // 已resolve对象不重建，数字前缀按PHP(int)转换，配置覆盖手工fallback64。
    repository.set(SECRET_LENGTH_KEY, json!("32tail"))?;
    actions.enable(&user, false).await?;
    assert_eq!(
        *provider.lengths.lock().map_err(|_| Error::Unauthorized)?,
        vec![32]
    );
    let before = model.snapshot();
    // 负长度无法转成Rust usize；noop必须连转换也不触发。
    repository.set(SECRET_LENGTH_KEY, json!(-16))?;
    actions.enable(&user, false).await?;
    assert_eq!(model.snapshot(), before);
    assert!(actions.enable(&user, true).await.is_err());
    assert_eq!(
        *provider.lengths.lock().map_err(|_| Error::Unauthorized)?,
        vec![32]
    );
    repository.set(SECRET_LENGTH_KEY, json!("not-a-number"))?;
    actions.enable(&user, false).await?;
    assert_eq!(model.snapshot(), before);
    assert_eq!(model.writes.load(Ordering::SeqCst), 1);
    assert_eq!(
        *provider.lengths.lock().map_err(|_| Error::Unauthorized)?,
        vec![32]
    );
    // 真正force才转换/校验无效长度，错误不能部分写入。
    assert!(actions.enable(&user, true).await.is_err());
    assert_eq!(model.snapshot(), before);
    repository.set(SECRET_LENGTH_KEY, json!("64 remaining"))?;
    actions.enable(&user, true).await?;
    assert_eq!(model.writes.load(Ordering::SeqCst), 2);
    assert_eq!(
        *provider.lengths.lock().map_err(|_| Error::Unauthorized)?,
        vec![32, 0, 64]
    );
    Ok(())
}

#[tokio::test]
async fn resolved_actions_read_length_after_model_await_for_enable_and_force() -> Result<()> {
    let (events, _, _) = events().await?;
    let provider = Arc::new(LengthProvider::default());
    let repository = length_repository()?;
    let actions = resolved_actions(
        TwoFactorActions::new(provider.clone(), encryptor(13)?, events)
            .with_config_repository(repository.clone()),
    )
    .await?;
    let model = Arc::new(Model::new(None));
    let user = account(&model);
    for (force, length) in [(false, 32_usize), (true, 64_usize)] {
        repository.set(SECRET_LENGTH_KEY, json!(16))?;
        let entered = Arc::new(tokio::sync::Notify::new());
        let resume = Arc::new(tokio::sync::Notify::new());
        *model.load_pause.lock().map_err(|_| Error::Unauthorized)? =
            Some((entered.clone(), resume.clone()));
        let enable = Box::pin(actions.enable(&user, force));
        let configure = async {
            entered.notified().await;
            repository.set(SECRET_LENGTH_KEY, json!(format!("{length} tail")))?;
            resume.notify_one();
            Ok::<_, Error>(())
        };
        let (enabled, changed) = tokio::time::timeout(std::time::Duration::from_secs(2), async {
            tokio::join!(enable, configure)
        })
        .await
        .map_err(Error::internal)?;
        changed?;
        enabled?;
    }
    assert_eq!(
        *provider.lengths.lock().map_err(|_| Error::Unauthorized)?,
        vec![32, 64]
    );
    assert_eq!(model.writes.load(Ordering::SeqCst), 2);
    Ok(())
}

#[tokio::test]
async fn public_confirmation_policy_helper_does_not_parse_negative_secret_length() -> Result<()> {
    let (events, _, _) = events().await?;
    let repository = length_repository()?;
    repository.set(SECRET_LENGTH_KEY, json!(-16))?;
    repository.set(
        "fortify-options.two-factor-authentication.confirm",
        json!(true),
    )?;
    let actions = resolved_actions(
        TwoFactorActions::new(Arc::new(Provider::default()), encryptor(14)?, events)
            .with_config_repository(repository.clone()),
    )
    .await?;
    assert!(actions.confirms_two_factor_authentication()?);
    repository.set(
        "fortify-options.two-factor-authentication.confirm",
        json!(false),
    )?;
    assert!(!actions.confirms_two_factor_authentication()?);
    repository.set("fortify.features", json!([]))?;
    assert!(!actions.confirms_two_factor_authentication()?);
    Ok(())
}
