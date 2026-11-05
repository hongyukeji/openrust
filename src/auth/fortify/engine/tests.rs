//! 固定源码的密码学、URI 与真实 Cache Repository 组合回归。
use super::*;
use crate::{
    cache::{CacheBackend, CacheDriver, CacheDriverConfig, CacheScope, CacheServiceProvider},
    core::{ProviderBuilder, ProviderRuntime},
};
use std::{
    collections::BTreeMap,
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

const SECRET: &str = "JBSWY3DPEHPK3PXP";
fn provider(seconds: u64) -> GoogleTwoFactorAuthenticationProvider {
    GoogleTwoFactorAuthenticationProvider::new(None).with_clock(Arc::new(move || Ok(seconds)))
}

#[test]
fn shared_primitive_matches_rfc6238_sha1_vectors_without_old_minimum_policy() -> Result<()> {
    for (seconds, expected) in [
        (59, "94287082"),
        (1_111_111_109, "07081804"),
        (1_111_111_111, "14050471"),
        (1_234_567_890, "89005924"),
        (2_000_000_000, "69279037"),
        (20_000_000_000, "65353130"),
    ] {
        assert_eq!(
            totp_code(b"12345678901234567890", seconds / 30, 8)?,
            expected
        );
        assert_eq!(
            totp_code(b"12345678901234567890", seconds / 30, 6)?,
            expected[2..]
        );
    }
    assert_eq!(decode_secret(SECRET)?.len(), 10);
    assert_eq!(totp_code(&decode_secret(SECRET)?, 100, 6)?, "594318");
    assert!(totp_code(&[], 0, 6).is_err());
    assert!(totp_code(b"key", 0, 9).is_err());
    Ok(())
}

#[test]
fn generation_uses_character_count_and_unpadded_base32_compatibility() -> Result<()> {
    let provider = provider(3_000);
    for length in [16, 32, 64] {
        let secret = provider.generate_secret_key(length)?;
        assert_eq!(secret.len(), length);
        assert!(secret.bytes().all(|byte| ALPHABET.contains(&byte)));
        assert_eq!(decode_secret(&secret)?.len(), length * 5 / 8);
    }
    for length in [0, 8, 15, 17, 24] {
        assert!(provider.generate_secret_key(length).is_err());
    }
    assert!(
        provider
            .generate_secret_key(1_usize << (usize::BITS - 1))
            .is_err()
    );
    assert_eq!(
        format!("{provider:?}"),
        "GoogleTwoFactorAuthenticationProvider { window: 1, cache_enabled: false, .. }"
    );
    Ok(())
}

#[test]
fn provisioning_uri_preserves_official_separate_rawurlencoding() -> Result<()> {
    assert_eq!(
        provider(0).qr_code_url("Acme +/中", "alice+test@example.com", SECRET)?,
        "otpauth://totp/Acme%20%2B%2F%E4%B8%AD:alice%2Btest%40example.com?secret=JBSWY3DPEHPK3PXP&issuer=Acme%20%2B%2F%E4%B8%AD&algorithm=SHA1&digits=6&period=30"
    );
    assert!(
        provider(0)
            .qr_code_url("<&\"", "a:b%", SECRET)?
            .starts_with("otpauth://totp/%3C%26%22:a%3Ab%25?")
    );
    Ok(())
}

#[tokio::test]
async fn windows_clock_boundaries_and_exact_code_strings_match_fixed_engine() -> Result<()> {
    let provider = provider(3_000);
    for code in ["307781", "594318", "485956"] {
        assert!(provider.verify(SECRET, code).await?);
    }
    for code in [
        "885497", " 594318", "594318\n", "59431", "5943180", "abcdef",
    ] {
        assert!(!provider.verify(SECRET, code).await?);
    }
    assert!(
        provider
            .verify(&SECRET.to_ascii_lowercase(), "594318")
            .await?
    );
    let exact = self::provider(3_000).with_window(0)?;
    assert!(exact.verify(SECRET, "594318").await?);
    assert!(!exact.verify(SECRET, "485956").await?);
    assert!(
        self::provider(29)
            .with_window(0)?
            .verify(SECRET, "282760")
            .await?
    );
    assert!(
        self::provider(30)
            .with_window(0)?
            .verify(SECRET, "996554")
            .await?
    );
    assert!(
        self::provider(870)
            .with_window(0)?
            .verify(SECRET, "067820")
            .await?
    );
    assert!(
        !self::provider(870)
            .with_window(0)?
            .verify(SECRET, "67820")
            .await?
    );
    assert!(
        !self::provider(3_000)
            .with_window(-1)?
            .verify(SECRET, "594318")
            .await?
    );
    assert!(
        !self::provider(3_000)
            .with_window(-1)?
            .verify("invalid", "594318")
            .await?
    );
    assert!(
        self::provider(3_000)
            .with_window(i64::MAX)?
            .verify(SECRET, "594318")
            .await
            .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn engine_preserves_php_low32_pack_while_primitive_keeps_full_counter() -> Result<()> {
    assert_eq!(
        totp_code(&decode_secret(SECRET)?, 1_u64 << 32, 6)?,
        "512141"
    );
    assert!(
        provider((1_u64 << 32) * 30)
            .with_window(0)?
            .verify(SECRET, "282760")
            .await?
    );
    assert!(
        !provider((1_u64 << 32) * 30)
            .with_window(0)?
            .verify(SECRET, "512141")
            .await?
    );
    // 当前步为0时，上游首个候选为-1，pack的低32位等于u32::MAX。
    assert!(provider(0).verify(SECRET, "681584").await?);
    Ok(())
}

#[tokio::test]
async fn malformed_secrets_are_typed_errors_and_cacheless_codes_can_repeat() -> Result<()> {
    let provider = provider(3_000);
    for invalid in [
        "JBSWY3DP",
        "JBSWY3DPEHPK3PXPJBSWY3DP",
        "JBSWY3DPEHPK3PX=",
        "JBSWY3DPEHPK3PX ",
        "JBSWY3DPEHPK3PX-",
        "JBSWY3DPEHPK3PX0",
    ] {
        let error = provider
            .verify(invalid, "594318")
            .await
            .err()
            .ok_or_else(time_error)?;
        assert_eq!(error.code(), "fortify_totp_secret_invalid");
        assert!(!error.to_string().contains(invalid));
    }
    assert!(provider.verify(SECRET, "594318").await?);
    assert!(provider.verify(SECRET, "594318").await?);
    let broken_clock = provider.with_clock(Arc::new(|| {
        Err(Error::custom("clock_failed", "时钟不可用"))
    }));
    assert_eq!(
        broken_clock
            .verify(SECRET, "594318")
            .await
            .err()
            .ok_or_else(time_error)?
            .code(),
        "clock_failed"
    );
    Ok(())
}

type Entry = (Vec<u8>, Option<u64>);
type CacheWrite = (String, Vec<u8>, Option<Duration>);
#[derive(Default)]
struct ProbeCache {
    entries: Mutex<BTreeMap<String, Entry>>,
    writes: Mutex<Vec<CacheWrite>>,
    now: AtomicU64,
    fail_get: AtomicBool,
    fail_set: AtomicBool,
    events: Mutex<Vec<String>>,
    pause_get: AtomicBool,
    get_entered: tokio::sync::Notify,
    get_resume: tokio::sync::Notify,
}
#[async_trait::async_trait]
impl CacheBackend for ProbeCache {
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        self.events
            .lock()
            .map_err(|_| time_error())?
            .push(format!("get:{key}"));
        if self.pause_get.swap(false, Ordering::SeqCst) {
            self.get_entered.notify_one();
            self.get_resume.notified().await;
        }
        if self.fail_get.load(Ordering::SeqCst) {
            return Err(Error::custom("probe_get_failed", "缓存读取失败"));
        }
        let now = self.now.load(Ordering::SeqCst);
        Ok(self
            .entries
            .lock()
            .map_err(|_| time_error())?
            .get(key)
            .filter(|(_, expiry)| expiry.is_none_or(|expiry| expiry > now))
            .map(|(value, _)| value.clone()))
    }
    async fn set(&self, key: &str, value: Vec<u8>, ttl: Option<Duration>) -> Result<()> {
        self.events
            .lock()
            .map_err(|_| time_error())?
            .push(format!("put:{key}"));
        if self.fail_set.load(Ordering::SeqCst) {
            return Err(Error::custom("probe_set_failed", "缓存写入失败"));
        }
        let expiry = ttl.map(|ttl| self.now.load(Ordering::SeqCst) + ttl.as_secs());
        self.writes
            .lock()
            .map_err(|_| time_error())?
            .push((key.to_owned(), value.clone(), ttl));
        self.entries
            .lock()
            .map_err(|_| time_error())?
            .insert(key.to_owned(), (value, expiry));
        Ok(())
    }
    async fn remove(&self, key: &str) -> Result<()> {
        self.entries.lock().map_err(|_| time_error())?.remove(key);
        Ok(())
    }
    fn scope(&self) -> CacheScope {
        CacheScope::Process
    }
}
struct ProbeDriver(Arc<ProbeCache>);
impl CacheDriver for ProbeDriver {
    fn name(&self) -> &'static str {
        "fortify-probe"
    }
    fn build<'a>(
        &'a self,
        _: &'a CacheDriverConfig,
    ) -> BoxFuture<'a, Result<Arc<dyn CacheBackend>>> {
        Box::pin(async move { Ok(self.0.clone() as Arc<dyn CacheBackend>) })
    }
}
async fn cached() -> Result<(Cache, Arc<ProbeCache>, ProviderRuntime)> {
    let backend = Arc::new(ProbeCache::default());
    let mut builder = ProviderBuilder::new();
    builder.provider(CacheServiceProvider::new().with_driver(ProbeDriver(backend.clone())));
    let runtime = builder.start(figment::Figment::new().merge(figment::providers::Serialized::defaults(serde_json::json!({"cache":{"default":"fortify","stores":{"fortify":{"driver":"fortify-probe","namespace":""}}}})))).await?;
    let cache = (*runtime.container().resolve::<Cache>().await?).clone();
    Ok((cache, backend, runtime))
}
fn with_cache(cache: Cache, window: i64) -> Result<GoogleTwoFactorAuthenticationProvider> {
    GoogleTwoFactorAuthenticationProvider::new(Some(cache))
        .with_clock(Arc::new(|| Ok(3_000)))
        .with_window(window)
}

#[tokio::test]
async fn official_global_code_key_replay_sequence_and_ttl_use_real_cache_repository() -> Result<()>
{
    let (cache, backend, runtime) = cached().await?;
    let result: Result<()> = async {
        let provider = with_cache(cache.clone(), 1)?;
        assert!(provider.verify(SECRET, "594318").await?);
        let key = "fortify.2fa_codes.74f1fce69fbd8972f510e6f37069c589";
        assert_eq!(cache.get::<i64>(key).await?, Some(100));
        assert!(!provider.verify(SECRET, "594318").await?);
        // 独立固定碰撞向量：另一密钥在当前步也生成同一验证码，缓存键仍全局共享。
        let other_secret = "AAAAAAAAAAAAQWVP";
        assert!(self::provider(3_000).verify(other_secret, "594318").await?);
        assert!(
            !with_cache(cache.clone(), 1)?
                .verify(other_secret, "594318")
                .await?
        );
        assert_eq!(
            backend
                .writes
                .lock()
                .map_err(|_| time_error())?
                .last()
                .map(|(key, _, ttl)| (key.clone(), *ttl)),
            Some((key.into(), Some(Duration::from_secs(60))))
        );
        backend.now.store(60, Ordering::SeqCst);
        assert!(provider.verify(SECRET, "594318").await?);
        let future = "fortify.2fa_codes.d20de7e7c7207af4003ef02e76635354";
        assert!(provider.verify(SECRET, "485956").await?);
        assert_eq!(cache.get::<i64>(future).await?, Some(100));
        assert!(provider.verify(SECRET, "485956").await?);
        assert_eq!(cache.get::<i64>(future).await?, Some(101));
        assert!(!provider.verify(SECRET, "485956").await?);
        let other_provider = with_cache(cache.clone(), 1)?;
        assert!(!other_provider.verify(SECRET, "485956").await?);
        Ok(())
    }
    .await;
    let shutdown = runtime
        .shutdown(tokio::time::Instant::now() + Duration::from_secs(2))
        .await;
    result?;
    shutdown
}

#[tokio::test]
async fn cache_ttls_follow_window_zero_one_two_and_failed_codes_do_not_write() -> Result<()> {
    let (cache, backend, runtime) = cached().await?;
    let result: Result<()> = async {
        for (window, ttl) in [(0, 60), (1, 60), (2, 120)] {
            cache.remove(&cache_key("594318")).await?;
            let provider = with_cache(cache.clone(), window)?;
            assert!(provider.verify(SECRET, "594318").await?);
            assert_eq!(
                backend
                    .writes
                    .lock()
                    .map_err(|_| time_error())?
                    .last()
                    .and_then(|(_, _, ttl)| *ttl),
                Some(Duration::from_secs(ttl))
            );
            let count = backend.writes.lock().map_err(|_| time_error())?.len();
            assert!(!provider.verify(SECRET, "abcdef").await?);
            assert_eq!(
                backend.writes.lock().map_err(|_| time_error())?.len(),
                count
            );
        }
        Ok(())
    }
    .await;
    let shutdown = runtime
        .shutdown(tokio::time::Instant::now() + Duration::from_secs(2))
        .await;
    result?;
    shutdown
}

#[tokio::test]
async fn repository_read_write_and_decode_errors_never_become_success_or_cache_miss() -> Result<()>
{
    let (cache, backend, runtime) = cached().await?;
    let result: Result<()> = async {
        let provider = with_cache(cache.clone(), 1)?;
        backend.fail_get.store(true, Ordering::SeqCst);
        assert_eq!(
            provider
                .verify(SECRET, "594318")
                .await
                .err()
                .ok_or_else(time_error)?
                .code(),
            "probe_get_failed"
        );
        backend.fail_get.store(false, Ordering::SeqCst);
        backend.fail_set.store(true, Ordering::SeqCst);
        assert_eq!(
            provider
                .verify(SECRET, "594318")
                .await
                .err()
                .ok_or_else(time_error)?
                .code(),
            "probe_set_failed"
        );
        backend.fail_set.store(false, Ordering::SeqCst);
        cache.set(&cache_key("594318"), &"not-a-step", None).await?;
        assert_eq!(
            provider
                .verify(SECRET, "594318")
                .await
                .err()
                .ok_or_else(time_error)?
                .code(),
            "cache_deserialization_failed"
        );
        Ok(())
    }
    .await;
    let shutdown = runtime
        .shutdown(tokio::time::Instant::now() + Duration::from_secs(2))
        .await;
    result?;
    shutdown
}

fn repository() -> crate::config::repository::ConfigRepository {
    crate::config::repository::ConfigRepository::new(figment::Figment::new())
}
const WINDOW_KEY: &str = "fortify-options.two-factor-authentication.window";

#[test]
fn verification_metadata_validates_bounds_and_binds_the_exact_code() -> Result<()> {
    use crate::auth::fortify::TwoFactorCodeVerification;
    let rejected = TwoFactorCodeVerification::rejected();
    assert!(!rejected.is_valid());
    assert_eq!(rejected.window(), None);
    assert_eq!(rejected.current_counter(), None);
    assert_eq!(rejected.matched_counter(), None);
    assert!(rejected.require_code("not-verified").is_err());
    let verified = TwoFactorCodeVerification::verified("custom-Code-004201", 2, 100, 102)?;
    assert!(verified.is_valid());
    assert_eq!(verified.window(), Some(2));
    assert_eq!(verified.current_counter(), Some(100));
    assert_eq!(verified.matched_counter(), Some(102));
    assert_eq!(verified.require_code("custom-Code-004201")?, 102);
    assert!(verified.require_code("custom-code-004201").is_err());
    assert!(verified.require_code("different").is_err());
    assert!(!format!("{verified:?}").contains("custom-Code-004201"));
    let negative = TwoFactorCodeVerification::verified("681584", 1, 0, -1)?;
    assert_eq!(negative.require_code("681584")?, -1);
    for (code, window, current, matched) in [
        ("", 1, 100, 100),
        ("code", 1, -1, 0),
        ("code", u64::MAX, 100, 100),
        ("code", 1, i64::MAX, i64::MAX),
        ("code", 1, 100, 98),
        ("code", 1, 100, 102),
        ("code", 0, 100, 101),
    ] {
        assert!(TwoFactorCodeVerification::verified(code, window, current, matched).is_err());
    }
    Ok(())
}

#[tokio::test]
async fn cache_miss_resamples_put_clock_but_metadata_keeps_actual_verified_counters() -> Result<()>
{
    let (cache, backend, runtime) = cached().await?;
    let outcome: Result<()> = async {
        let calls = Arc::new(AtomicU64::new(0));
        let clock_calls = calls.clone();
        let clock_backend = backend.clone();
        let provider = GoogleTwoFactorAuthenticationProvider::new(Some(cache.clone())).with_clock(
            Arc::new(move || {
                let call = clock_calls.fetch_add(1, Ordering::SeqCst);
                clock_backend
                    .events
                    .lock()
                    .map_err(|_| time_error())?
                    .push(format!("clock:{call}"));
                Ok(if call == 0 { 3_000 } else { 3_030 })
            }),
        );
        let verified = provider.verify_with_context(SECRET, "485956").await?;
        assert!(verified.is_valid());
        assert_eq!(verified.window(), Some(1));
        assert_eq!(verified.current_counter(), Some(100));
        assert_eq!(verified.matched_counter(), Some(101));
        assert_eq!(verified.require_code("485956")?, 101);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        let key = cache_key("485956");
        assert_eq!(
            *backend.events.lock().map_err(|_| time_error())?,
            vec![
                format!("get:{key}"),
                "clock:0".into(),
                "clock:1".into(),
                format!("put:{key}")
            ]
        );
        assert_eq!(cache.get::<i64>(&key).await?, Some(101));
        assert_eq!(
            backend
                .writes
                .lock()
                .map_err(|_| time_error())?
                .last()
                .and_then(|(_, _, ttl)| *ttl),
            Some(Duration::from_secs(60))
        );
        Ok(())
    }
    .await;
    let shutdown = runtime
        .shutdown(tokio::time::Instant::now() + Duration::from_secs(2))
        .await;
    outcome?;
    shutdown
}

#[tokio::test]
async fn cached_hit_uses_one_clock_and_bool_facade_does_not_verify_twice() -> Result<()> {
    let (cache, backend, runtime) = cached().await?;
    let outcome: Result<()> = async {
        let key = cache_key("485956");
        cache.set(&key, &100_i64, None).await?;
        backend.events.lock().map_err(|_| time_error())?.clear();
        let calls = Arc::new(AtomicU64::new(0));
        let count = calls.clone();
        let observed = backend.clone();
        let provider = GoogleTwoFactorAuthenticationProvider::new(Some(cache.clone())).with_clock(
            Arc::new(move || {
                let call = count.fetch_add(1, Ordering::SeqCst);
                observed
                    .events
                    .lock()
                    .map_err(|_| time_error())?
                    .push(format!("clock:{call}"));
                Ok(3_000)
            }),
        );
        let verified = provider.verify_with_context(SECRET, "485956").await?;
        assert_eq!(verified.current_counter(), Some(100));
        assert_eq!(verified.matched_counter(), Some(101));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            *backend.events.lock().map_err(|_| time_error())?,
            vec![format!("get:{key}"), "clock:0".into(), format!("put:{key}")]
        );
        cache.remove(&key).await?;
        backend.events.lock().map_err(|_| time_error())?.clear();
        calls.store(0, Ordering::SeqCst);
        assert!(provider.verify(SECRET, "485956").await?);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(
            *backend.events.lock().map_err(|_| time_error())?,
            vec![
                format!("get:{key}"),
                "clock:0".into(),
                "clock:1".into(),
                format!("put:{key}")
            ]
        );
        Ok(())
    }
    .await;
    let shutdown = runtime
        .shutdown(tokio::time::Instant::now() + Duration::from_secs(2))
        .await;
    outcome?;
    shutdown
}

#[tokio::test]
async fn dynamic_window_int_null_missing_and_noninteger_follow_instance_sticky_value() -> Result<()>
{
    let repository = repository();
    repository.set(WINDOW_KEY, serde_json::json!(2))?;
    let provider = self::provider(3_000)
        .with_window(0)?
        .with_config_repository(repository.clone());
    let verified = provider.verify_with_context(SECRET, "485956").await?;
    assert_eq!(verified.window(), Some(2));
    for value in [
        serde_json::Value::Null,
        serde_json::json!("0"),
        serde_json::json!(false),
        serde_json::json!(1.5),
        serde_json::json!(u64::MAX),
        serde_json::json!({}),
    ] {
        repository.set(WINDOW_KEY, value)?;
        assert_eq!(
            provider
                .verify_with_context(SECRET, "485956")
                .await?
                .window(),
            Some(2)
        );
    }
    repository.set(WINDOW_KEY, serde_json::json!(0))?;
    assert!(
        !provider
            .verify_with_context(SECRET, "485956")
            .await?
            .is_valid()
    );
    assert_eq!(
        provider
            .verify_with_context(SECRET, "594318")
            .await?
            .window(),
        Some(0)
    );
    repository.set(
        "fortify-options.two-factor-authentication",
        serde_json::json!({}),
    )?;
    assert!(!provider.verify(SECRET, "485956").await?);
    repository.set(WINDOW_KEY, serde_json::json!(-1))?;
    assert!(!provider.verify("invalid-secret", "594318").await?);
    repository.set(WINDOW_KEY, serde_json::Value::Null)?;
    assert!(!provider.verify(SECRET, "594318").await?);
    repository.set(WINDOW_KEY, serde_json::json!(1))?;
    assert_eq!(
        provider
            .verify_with_context(SECRET, "485956")
            .await?
            .window(),
        Some(1)
    );
    Ok(())
}

#[tokio::test]
async fn window_snapshot_precedes_pending_cache_get_and_change_affects_next_verification()
-> Result<()> {
    let (cache, backend, runtime) = cached().await?;
    let outcome: Result<()> = async {
        let repository = repository();
        repository.set(WINDOW_KEY, serde_json::json!(0))?;
        let provider =
            Arc::new(with_cache(cache.clone(), 1)?.with_config_repository(repository.clone()));
        backend.pause_get.store(true, Ordering::SeqCst);
        // join驱动同一次调用到真实CacheBackend.get屏障，不依赖计时sleep。
        let first = provider.verify_with_context(SECRET, "485956");
        let change = async {
            tokio::time::timeout(Duration::from_secs(2), backend.get_entered.notified())
                .await
                .map_err(Error::internal)?;
            repository.set(WINDOW_KEY, serde_json::json!(2))?;
            backend.get_resume.notify_one();
            Ok::<_, Error>(())
        };
        let (verified, changed) = tokio::time::timeout(Duration::from_secs(3), async {
            tokio::join!(first, change)
        })
        .await
        .map_err(Error::internal)?;
        changed?;
        assert!(!verified?.is_valid());
        assert!(backend.writes.lock().map_err(|_| time_error())?.is_empty());
        let second = provider.verify_with_context(SECRET, "485956").await?;
        assert_eq!(second.window(), Some(2));
        assert_eq!(second.current_counter(), Some(100));
        assert_eq!(second.matched_counter(), Some(101));
        assert_eq!(
            backend
                .writes
                .lock()
                .map_err(|_| time_error())?
                .last()
                .and_then(|(_, _, ttl)| *ttl),
            Some(Duration::from_secs(120))
        );
        Ok(())
    }
    .await;
    let shutdown = runtime
        .shutdown(tokio::time::Instant::now() + Duration::from_secs(2))
        .await;
    outcome?;
    shutdown
}

#[tokio::test]
async fn get_failure_negative_window_and_old_step_overflow_preserve_operation_order() -> Result<()>
{
    let (cache, backend, runtime) = cached().await?;
    let outcome: Result<()> = async {
        let calls = Arc::new(AtomicU64::new(0));
        let count = calls.clone();
        let provider = with_cache(cache.clone(), -1)?.with_clock(Arc::new(move || {
            count.fetch_add(1, Ordering::SeqCst);
            Ok(3_000)
        }));
        backend.fail_get.store(true, Ordering::SeqCst);
        assert_eq!(
            provider
                .verify_with_context("invalid", "594318")
                .await
                .err()
                .ok_or_else(time_error)?
                .code(),
            "probe_get_failed"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert!(backend.writes.lock().map_err(|_| time_error())?.is_empty());
        backend.fail_get.store(false, Ordering::SeqCst);
        assert!(
            !provider
                .verify_with_context("invalid", "594318")
                .await?
                .is_valid()
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(backend.writes.lock().map_err(|_| time_error())?.is_empty());
        let key = cache_key("594318");
        cache.set(&key, &i64::MAX, None).await?;
        backend.writes.lock().map_err(|_| time_error())?.clear();
        assert_eq!(
            with_cache(cache.clone(), 0)?
                .verify_with_context(SECRET, "594318")
                .await
                .err()
                .ok_or_else(time_error)?
                .code(),
            "fortify_totp_time_invalid"
        );
        assert!(backend.writes.lock().map_err(|_| time_error())?.is_empty());
        Ok(())
    }
    .await;
    let shutdown = runtime
        .shutdown(tokio::time::Instant::now() + Duration::from_secs(2))
        .await;
    outcome?;
    shutdown
}

#[tokio::test]
async fn miss_second_clock_failure_never_writes_or_succeeds_even_without_cache() -> Result<()> {
    let (cache, backend, runtime) = cached().await?;
    let outcome: Result<()> = async {
        for selected in [None, Some(cache.clone())] {
            let calls = Arc::new(AtomicU64::new(0));
            let count = calls.clone();
            let provider = GoogleTwoFactorAuthenticationProvider::new(selected).with_clock(
                Arc::new(move || {
                    if count.fetch_add(1, Ordering::SeqCst) == 0 {
                        Ok(3_000)
                    } else {
                        Err(Error::custom("second_clock_failed", "成功后时钟读取失败"))
                    }
                }),
            );
            assert_eq!(
                provider
                    .verify_with_context(SECRET, "594318")
                    .await
                    .err()
                    .ok_or_else(time_error)?
                    .code(),
                "second_clock_failed"
            );
            assert_eq!(calls.load(Ordering::SeqCst), 2);
            assert!(backend.writes.lock().map_err(|_| time_error())?.is_empty());
        }
        // 实际window的TTL乘法溢出可在首候选立即匹配；不执行巨大窗口枚举。
        // 反推clock使current=window，首候选0，固定step0向量立即命中。
        let window = u64::MAX / 60 + 1;
        let seconds = window.checked_mul(30).ok_or_else(time_error)?;
        let provider = GoogleTwoFactorAuthenticationProvider::new(Some(cache.clone()))
            .with_window(i64::try_from(window).map_err(|_| time_error())?)?
            .with_clock(Arc::new(move || Ok(seconds)));
        assert_eq!(
            provider
                .verify_with_context(SECRET, "282760")
                .await
                .err()
                .ok_or_else(time_error)?
                .code(),
            "fortify_totp_time_invalid"
        );
        assert!(backend.writes.lock().map_err(|_| time_error())?.is_empty());
        Ok(())
    }
    .await;
    let shutdown = runtime
        .shutdown(tokio::time::Instant::now() + Duration::from_secs(2))
        .await;
    outcome?;
    shutdown
}
