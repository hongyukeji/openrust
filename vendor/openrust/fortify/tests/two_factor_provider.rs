use openrust::{
    cache::{ArrayStore, CacheError, CacheFuture, CacheResult, Repository, Ttl, Value},
    config,
    contracts::{
        cache::{Repository as Cache, Store},
        config::Repository as _,
    },
};
use openrust_fortify::{
    FortifyError, TwoFactorAuthenticationProvider, contracts::TwoFactorAuthenticationProvider as _,
};
use serde_json::{Value as Json, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
struct RecordingCache {
    inner: Repository,
    old: Mutex<Option<Value>>,
    failure: String,
    log: Mutex<Vec<Json>>,
}
impl RecordingCache {
    fn new(old: Json, failure: &str) -> Self {
        Self {
            inner: Repository::new(Arc::new(ArrayStore::new())),
            old: Mutex::new(Some(Value::Json(old))),
            failure: failure.into(),
            log: Mutex::new(vec![]),
        }
    }
}
macro_rules! delegate { ($name:ident($($arg:ident:$ty:ty),*)->$ret:ty)=>{
 fn $name<'a>(&'a self,$($arg:$ty),*)->CacheFuture<'a,CacheResult<$ret>> {self.inner.$name($($arg),*)}
}; }
impl Cache for RecordingCache {
    fn get<'a>(&'a self, key: &'a str) -> CacheFuture<'a, CacheResult<Option<Value>>> {
        Box::pin(async move {
            self.log.lock().unwrap().push(json!(["get", key]));
            if self.failure == "get" {
                return Err(CacheError::InvalidValue);
            }
            Ok(self.old.lock().unwrap().clone())
        })
    }
    fn put<'a>(
        &'a self,
        key: &'a str,
        value: &'a Value,
        ttl: Option<Ttl>,
    ) -> CacheFuture<'a, CacheResult<bool>> {
        Box::pin(async move {
            let Some(Ttl::Seconds(seconds)) = ttl else {
                panic!("relative seconds")
            };
            let Value::Json(value) = value else {
                panic!("JSON")
            };
            self.log
                .lock()
                .unwrap()
                .push(json!(["put", key, value, seconds]));
            if self.failure == "put" {
                return Err(CacheError::InvalidValue);
            }
            *self.old.lock().unwrap() = Some(Value::Json(value.clone()));
            Ok(self.failure != "put-false")
        })
    }
    delegate!(add(key:&'a str,value:&'a Value,ttl:Option<Ttl>)->bool);
    delegate!(forever(key:&'a str,value:&'a Value)->bool);
    delegate!(touch(key:&'a str,ttl:Ttl)->bool);
    delegate!(pull(key:&'a str)->Option<Value>);
    delegate!(forget(key:&'a str)->bool);
    delegate!(increment(key:&'a str,amount:i64)->i64);
    delegate!(decrement(key:&'a str,amount:i64)->i64);
    fn get_store(&self) -> &dyn Store {
        openrust::contracts::cache::Repository::get_store(&self.inner)
    }
}
fn error_name(error: &FortifyError) -> &'static str {
    match error {
        FortifyError::InvalidCharacters => "InvalidCharactersException",
        FortifyError::IncompatibleWithGoogleAuthenticator => {
            "IncompatibleWithGoogleAuthenticatorException"
        }
        FortifyError::SecretKeyTooShort => "SecretKeyTooShortException",
        _ => panic!("unexpected {error}"),
    }
}
fn oracle() -> Json {
    serde_json::from_str(include_str!("fixtures/two_factor_oracle.json")).unwrap()
}
#[tokio::test]
async fn fixed_php_provider_decisions_and_cache_trace() {
    for (index, case) in oracle()["cases"].as_array().unwrap().iter().enumerate() {
        let config = Arc::new(config::Repository::new(Default::default()));
        config
            .set(
                "fortify-options.two-factor-authentication.window",
                case["window"].clone(),
            )
            .unwrap();
        let cache = Arc::new(RecordingCache::new(
            case["old"].clone(),
            case["failure"].as_str().unwrap(),
        ));
        let binding = case["cache"]
            .as_bool()
            .unwrap()
            .then(|| cache.clone() as Arc<dyn Cache>);
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        let seconds = case["seconds"].as_i64().unwrap();
        let next = case["next"].as_i64().unwrap();
        let provider = TwoFactorAuthenticationProvider::new(binding)
            .with_config(config)
            .withClock(move || {
                Ok(if count.fetch_add(1, Ordering::Relaxed) == 0 {
                    seconds
                } else {
                    next
                })
            });
        let outcome = match provider
            .verify(
                case["secret"].as_str().unwrap(),
                case["code"].as_str().unwrap(),
            )
            .await
        {
            Ok(value) => json!({"ok":value}),
            Err(FortifyError::Cache(_)) => {
                json!({"error":format!("cache-{}",case["failure"].as_str().unwrap())})
            }
            Err(error) => json!({"error":error_name(&error)}),
        };
        let expected = if case.get("error").is_some() {
            json!({"error":case["error"]})
        } else {
            json!({"ok":case["ok"]})
        };
        assert_eq!(outcome, expected, "case {index}: {case}");
        assert_eq!(
            json!(calls.load(Ordering::Relaxed)),
            case["calls"],
            "clock {index}"
        );
        assert_eq!(
            json!(*cache.log.lock().unwrap()),
            case["log"],
            "cache {index}"
        );
        assert_eq!(
            json!(provider.getWindow()),
            case["window_after"],
            "window {index}"
        );
    }
}
#[test]
fn fixed_php_secret_lengths_and_qr_urls() {
    let oracle = oracle();
    let provider = TwoFactorAuthenticationProvider::new(None);
    for case in oracle["qr"].as_array().unwrap() {
        let input = case["input"].as_array().unwrap();
        assert_eq!(
            provider.qrCodeUrl(
                input[0].as_str().unwrap(),
                input[1].as_str().unwrap(),
                input[2].as_str().unwrap()
            ),
            case["url"].as_str().unwrap()
        );
    }
    for case in oracle["lengths"].as_array().unwrap() {
        match provider.generateSecretKey(case["input"].as_u64().unwrap() as usize) {
            Ok(secret) => {
                assert_eq!(json!(secret.len()), case["length"]);
                assert!(
                    secret
                        .bytes()
                        .all(|byte| byte.is_ascii_uppercase() || matches!(byte, b'2'..=b'7'))
                );
            }
            Err(error) => assert_eq!(error_name(&error), case["error"].as_str().unwrap()),
        }
    }
}
#[tokio::test]
async fn replay_and_dynamic_window_use_real_cache() {
    let cache = Arc::new(Repository::new(Arc::new(ArrayStore::new())));
    let config = Arc::new(config::Repository::new(Default::default()));
    let provider = TwoFactorAuthenticationProvider::new(Some(cache))
        .with_config(config.clone())
        .withClock(|| Ok(1_234_567_890));
    assert!(provider.verify("JBSWY3DPEHPK3PXP", "742275").await.unwrap());
    assert!(!provider.verify("JBSWY3DPEHPK3PXP", "742275").await.unwrap());
    config
        .set("fortify-options.two-factor-authentication.window", json!(2))
        .unwrap();
    assert!(!provider.verify("JBSWY3DPEHPK3PXP", "bad").await.unwrap());
    assert_eq!(provider.getWindow(), 2);
    config
        .set(
            "fortify-options.two-factor-authentication.window",
            json!("0"),
        )
        .unwrap();
    assert!(!provider.verify("JBSWY3DPEHPK3PXP", "bad").await.unwrap());
    assert_eq!(provider.getWindow(), 2);
}
#[tokio::test]
async fn malformed_cache_and_clock_overflow_fail_closed() {
    let cache = Arc::new(RecordingCache::new(json!("invalid"), ""));
    let provider = TwoFactorAuthenticationProvider::new(Some(cache))
        .withClock(|| panic!("cache precedes clock"));
    assert!(matches!(
        provider.verify("invalid", "123456").await,
        Err(FortifyError::InvalidCacheTimestamp)
    ));
    let provider = TwoFactorAuthenticationProvider::new(None)
        .withWindow(i64::MAX)
        .withClock(|| Ok(60));
    assert!(matches!(
        provider.verify("invalid", "123456").await,
        Err(FortifyError::InvalidTime)
    ));
}
