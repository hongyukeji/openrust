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
        assert_eq!(info.algo_name, name);
        assert!(driver.check(value, &stored, &options).await.unwrap());
        assert!(!driver.check(b"wrong", &stored, &options).await.unwrap());
        assert!(!driver.check(value, "", &options).await.unwrap());
        assert!(driver.verify_configuration(&stored).unwrap());
        assert!(!driver.needs_rehash(&stored, &options).unwrap());
        assert!(
            driver
                .needs_rehash(&stored, override_options.as_object().unwrap())
                .unwrap()
        );
        let changed = driver
            .make(value, override_options.as_object().unwrap())
            .await
            .unwrap();
        assert!(!driver.verify_configuration(&changed).unwrap());
        assert!(driver.needs_rehash(&changed, &options).unwrap());
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
    assert!(migration.needs_rehash(&stored, &options).unwrap());
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
        assert!(hasher.needs_rehash(hashed, &options).unwrap());
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
        assert!(!driver.needs_rehash(&encoded, &options).unwrap());
        let legacy = encoded.replace("$v=16", "");
        assert!(driver.check(b"sample", &legacy, &options).await.unwrap());
        assert!(driver.needs_rehash(&legacy, &options).unwrap());
    }
}

fn custom_manager(default: &str) -> HashManager {
    HashManager::new(Arc::new(Repository::new(BTreeMap::from([(
        "hashing".into(),
        json!({"driver":default,"bcrypt":{"rounds":4},"argon":{"memory":1024,"time":2,"threads":1}}),
    )]))))
    .unwrap()
}

#[tokio::test]
async fn custom_driver_is_lazy_cached_and_replaced_only_after_forgetting() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let count = Arc::new(AtomicUsize::new(0));
    let manager = custom_manager("registered");
    let other = custom_manager("registered");
    let visits = count.clone();
    manager.extend("registered", move |manager| {
        visits.fetch_add(1, Ordering::Relaxed);
        manager.driver(Some("bcrypt"))
    });
    assert_eq!(count.load(Ordering::Relaxed), 0);
    assert!(manager.get_drivers().is_empty());
    assert!(matches!(other.driver(None), Err(HashError::UnknownDriver)));

    let cached = manager.driver(None).unwrap();
    assert!(Arc::ptr_eq(&cached, &manager.driver(Some("0")).unwrap()));
    assert!(Arc::ptr_eq(&cached, &manager.driver(Some("")).unwrap()));
    assert_eq!(count.load(Ordering::Relaxed), 1);
    manager.extend("registered", |manager| manager.driver(Some("argon2id")));
    assert!(Arc::ptr_eq(&cached, &manager.driver(None).unwrap()));
    manager.forget_drivers();
    let options = HashOptions::new();
    let encoded = manager.make(b"test password", &options).await.unwrap();
    assert_eq!(manager.info(&encoded).unwrap().algo_name, "argon2id");
    assert!(
        manager
            .check(b"test password", &encoded, &options)
            .await
            .unwrap()
    );
    assert!(!manager.needs_rehash(&encoded, &options).unwrap());
}

#[test]
fn custom_driver_failures_and_cycles_leave_the_cache_retryable() {
    use std::sync::atomic::{AtomicBool, Ordering};

    let manager = Arc::new(custom_manager("custom"));
    let peer = Arc::new(custom_manager("custom"));
    let owner = Arc::downgrade(&manager);
    manager.extend("custom", move |_| Ok(owner.upgrade().unwrap()));
    assert!(matches!(
        manager.driver(None),
        Err(HashError::DriverSelfReference)
    ));

    let peer_ref = Arc::downgrade(&peer);
    manager.extend("custom", move |_| Ok(peer_ref.upgrade().unwrap()));
    let owner = Arc::downgrade(&manager);
    peer.extend("custom", move |_| Ok(owner.upgrade().unwrap()));
    assert!(matches!(manager.driver(None), Err(HashError::DriverBusy)));
    assert!(manager.get_drivers().is_empty());
    assert!(peer.get_drivers().is_empty());

    let attempted = AtomicBool::new(false);
    peer.extend("custom", move |manager| {
        if attempted.swap(true, Ordering::Relaxed) {
            manager.driver(Some("bcrypt"))
        } else {
            Err(HashError::InvalidOptions)
        }
    });
    assert!(matches!(
        manager.driver(None),
        Err(HashError::InvalidOptions)
    ));
    assert!(manager.get_drivers().is_empty());
    assert!(Arc::ptr_eq(
        &manager.driver(None).unwrap(),
        &peer.driver(None).unwrap()
    ));
}

#[test]
fn concurrent_custom_resolution_and_forget_do_not_publish_an_old_instance() {
    use std::sync::Barrier;

    let manager = Arc::new(custom_manager("custom"));
    let entered = Arc::new(Barrier::new(2));
    let resume = Arc::new(Barrier::new(2));
    let signals = (entered.clone(), resume.clone());
    manager.extend("custom", move |manager| {
        signals.0.wait();
        signals.1.wait();
        Ok(Arc::new(manager.create_bcrypt_driver()?))
    });
    let active = manager.clone();
    let task = std::thread::spawn(move || active.driver(None).unwrap());
    entered.wait();
    assert!(matches!(manager.driver(None), Err(HashError::DriverBusy)));
    manager.forget_drivers().extend("custom", |manager| {
        Ok(Arc::new(manager.create_argon2id_driver()?))
    });
    resume.wait();
    let old = task.join().unwrap();
    assert!(manager.get_drivers().is_empty());
    assert!(!Arc::ptr_eq(&old, &manager.driver(None).unwrap()));
}

#[tokio::test]
async fn custom_hash_driver_resolves_its_own_application_over_http() {
    use std::{net::SocketAddr, path::Path};

    use axum::{Router, extract::State};
    use openrust::{
        config::{ConfigError, Environment, Value},
        contracts::{container::Container as _, foundation::Application as _, http::Kernel as _},
        foundation::{Application, configuration::Routing, http::Kernel},
        http::{HttpError, HttpFuture, Request, Response, StatusCode},
        support::{
            ProviderResult, ServiceProvider,
            facades::{Hash, Route},
        },
    };

    struct HashDriver(&'static str);

    impl ServiceProvider for HashDriver {
        fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
            Box::pin(async move {
                app.container().instance(Arc::new(Self(self.0)))?;
                Hash::extend("tenant", |manager| {
                    let container = manager.get_container().expect("application container");
                    let kind = container
                        .make::<HashDriver>()
                        .map_err(HashError::Unavailable)?;
                    manager.driver(Some(kind.0))
                })?;
                Ok(())
            })
        }
    }

    fn config(_: &Path, _: &Environment) -> Result<BTreeMap<String, Value>, ConfigError> {
        Ok(BTreeMap::from([(
            "hashing".into(),
            json!({
                "driver":"tenant",
                "bcrypt":{"rounds":4},
                "argon":{"memory":1024,"time":2,"threads":1}
            }),
        )]))
    }

    fn hash(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        Box::pin(async {
            let options = HashOptions::new();
            let stored = Hash::make(b"fixture password", &options).await.unwrap();
            assert!(
                Hash::check(b"fixture password", &stored, &options)
                    .await
                    .unwrap()
            );
            assert!(!Hash::needs_rehash(&stored, &options).unwrap());
            Response::json(StatusCode::OK, Hash::info(&stored).unwrap().algo_name)
        })
    }

    async fn transport(
        State(kernel): State<Arc<Kernel>>,
        incoming: axum::extract::Request,
    ) -> axum::response::Response {
        let (parts, body) = incoming.into_parts();
        let mut request = Request::from_parts(parts, body);
        let (parts, body) = kernel.handle(&mut request).await.into_parts();
        axum::response::Response::from_parts(parts, body)
    }

    async fn serve(
        path: &Path,
        selected: &'static str,
    ) -> (SocketAddr, tokio::task::JoinHandle<()>) {
        let app = Application::configure(path.to_path_buf())
            .with_config(config)
            .with_providers(vec![Box::new(HashDriver(selected))])
            .with_routing(Routing::default().web(|| {
                Route::get("/hash", hash)?;
                Ok(())
            }))
            .create()
            .unwrap();
        let mut kernel = Kernel::new(app);
        kernel.bootstrap().await.unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new()
                    .fallback(transport)
                    .with_state(Arc::new(kernel)),
            )
            .await
            .unwrap();
        });
        (address, server)
    }

    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let (first_address, first_server) = serve(first.path(), "bcrypt").await;
    let (second_address, second_server) = serve(second.path(), "argon2id").await;
    let get = |address| async move {
        reqwest::get(format!("http://{address}/hash"))
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .text()
            .await
            .unwrap()
    };
    let (bcrypt, argon) = tokio::join!(get(first_address), get(second_address));
    assert_eq!(bcrypt, r#""bcrypt""#);
    assert_eq!(argon, r#""argon2id""#);
    first_server.abort();
    second_server.abort();
}

#[tokio::test]
async fn parameter_setters_change_defaults_preserve_overrides_and_enforce_limits() {
    let options = HashOptions::new();
    let mut bcrypt = BcryptHasher::new(BcryptConfig {
        rounds: 4,
        ..Default::default()
    });
    let before = bcrypt.make(b"secret", &options).await.unwrap();
    bcrypt.set_rounds(5);
    assert!(bcrypt.needs_rehash(&before, &options).unwrap());
    let overridden = json!({"rounds":4});
    assert!(
        !bcrypt
            .needs_rehash(&before, overridden.as_object().unwrap())
            .unwrap()
    );
    let after = bcrypt.make(b"secret", &options).await.unwrap();
    assert_eq!(bcrypt.info(&after).unwrap().options["cost"], json!(5));
    bcrypt.set_rounds(17);
    assert!(matches!(
        bcrypt.make(b"secret", &options).await,
        Err(HashError::ExecutionLimit)
    ));
    let mut argon = ArgonHasher::new(ArgonConfig {
        memory: 1024,
        time: 2,
        threads: 1,
        verify: false,
    });
    let mut argon2 = Argon2IdHasher::new(ArgonConfig {
        memory: 1024,
        time: 2,
        threads: 1,
        verify: false,
    });
    argon.set_memory(2048).set_time(1).set_threads(2);
    argon2.set_memory(2048).set_time(1).set_threads(2);
    for driver in [&argon as &dyn Hasher, &argon2 as &dyn Hasher] {
        let hash = driver.make(b"secret", &options).await.unwrap();
        assert!(hash.contains("m=2048,t=1,p=2"));
        assert!(driver.check(b"secret", &hash, &options).await.unwrap());
        assert!(!driver.needs_rehash(&hash, &options).unwrap());
    }
    argon.set_memory(0);
    assert!(argon.make(b"secret", &options).await.is_err());
    argon2.set_threads(5);
    assert!(matches!(
        argon2.make(b"secret", &options).await,
        Err(HashError::ExecutionLimit)
    ));
}
