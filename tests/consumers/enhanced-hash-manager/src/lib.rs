//! 公开增强密码入口使用同一Manager内登记的原始自定义驱动。

#[cfg(test)]
mod tests {
    use openrust::{
        Result,
        auth::EncodedPasswordHash,
        hashing::{
            Argon2IdHasher, ArgonConfig, HashInfo, HashManager, HashOptions, Hasher, HashingConfig,
        },
        support::BoxFuture,
    };
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    struct Observed {
        inner: Argon2IdHasher,
        capability: AtomicUsize,
        makes: AtomicUsize,
        checks: AtomicUsize,
        rehashes: AtomicUsize,
    }
    impl Hasher for Observed {
        fn accepts_exact_input(&self, value: &[u8]) -> Result<bool> {
            self.capability.fetch_add(1, Ordering::SeqCst);
            self.inner.accepts_exact_input(value)
        }
        fn info(&self, encoded: &str) -> Result<HashInfo> {
            self.inner.info(encoded)
        }
        fn make<'a>(
            &'a self,
            value: &'a [u8],
            options: &'a HashOptions,
        ) -> BoxFuture<'a, Result<String>> {
            Box::pin(async move {
                self.makes.fetch_add(1, Ordering::SeqCst);
                self.inner.make(value, options).await
            })
        }
        fn check<'a>(
            &'a self,
            value: &'a [u8],
            encoded: Option<&'a str>,
            options: &'a HashOptions,
        ) -> BoxFuture<'a, Result<bool>> {
            Box::pin(async move {
                self.checks.fetch_add(1, Ordering::SeqCst);
                self.inner.check(value, encoded, options).await
            })
        }
        fn needs_rehash(&self, encoded: &str, options: &HashOptions) -> Result<bool> {
            self.rehashes.fetch_add(1, Ordering::SeqCst);
            self.inner.needs_rehash(encoded, options)
        }
        fn verify_configuration(&self, encoded: &str) -> Result<bool> {
            self.inner.verify_configuration(encoded)
        }
    }

    fn assert_send<T: Send>(_: &T) {}

    #[tokio::test]
    async fn original_custom_arc_and_borrowed_password_survive_typed_enhanced_entrypoints()
    -> Result<()> {
        let original = Arc::new(Observed {
            inner: Argon2IdHasher::new(ArgonConfig {
                memory: 19_456,
                time: 1,
                threads: 1,
                verify: true,
            })?,
            capability: AtomicUsize::new(0),
            makes: AtomicUsize::new(0),
            checks: AtomicUsize::new(0),
            rehashes: AtomicUsize::new(0),
        });
        let leaf: Arc<dyn Hasher> = original.clone();
        let returned = Arc::clone(&leaf);
        let manager = HashManager::new(HashingConfig {
            driver: "external-original".into(),
            ..HashingConfig::default()
        })?;
        manager.extend("external-original", move || Ok(Arc::clone(&returned)))?;
        assert!(Arc::ptr_eq(&manager.driver_for_operation()?, &leaf));
        let password = "a".repeat(73);
        let future = EncodedPasswordHash::make(&manager, &password);
        assert_send(&future);
        let hash = future.await?;
        assert!(hash.as_str().starts_with("$argon2id$"));
        assert!(hash.verify(&manager, &password).await?);
        assert!(!hash.verify(&manager, &password[..72]).await?);
        EncodedPasswordHash::validate_input(&manager, &password)?;
        assert!(!hash.needs_rehash(&manager)?);
        assert_eq!(original.capability.load(Ordering::SeqCst), 4);
        assert_eq!(original.makes.load(Ordering::SeqCst), 1);
        assert_eq!(original.checks.load(Ordering::SeqCst), 2);
        assert_eq!(original.rehashes.load(Ordering::SeqCst), 1);
        assert!(Arc::ptr_eq(&manager.driver_for_operation()?, &leaf));
        Ok(())
    }

    #[tokio::test]
    async fn invalid_basic_input_precedes_default_driver_resolution() -> Result<()> {
        let manager = HashManager::new(HashingConfig {
            driver: "must-not-run".into(),
            ..HashingConfig::default()
        })?;
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&calls);
        manager.extend("must-not-run", move || {
            observed.fetch_add(1, Ordering::SeqCst);
            Err(openrust::Error::custom(
                "consumer_factory_reached",
                "公开回归工厂已被调用",
            ))
        })?;
        let stored = EncodedPasswordHash::from_encoded("public-unused-fixture");
        for password in [String::new(), "x".repeat(1025)] {
            assert!(matches!(
                EncodedPasswordHash::validate_input(&manager, &password),
                Err(openrust::Error::BadRequest(_))
            ));
            assert!(matches!(
                EncodedPasswordHash::make(&manager, &password).await,
                Err(openrust::Error::BadRequest(_))
            ));
            assert!(matches!(
                stored.verify(&manager, &password).await,
                Err(openrust::Error::BadRequest(_))
            ));
        }
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert!(matches!(
            EncodedPasswordHash::make(&manager, "valid-public-fixture").await,
            Err(error) if error.code() == "consumer_factory_reached"
        ));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        Ok(())
    }
}
