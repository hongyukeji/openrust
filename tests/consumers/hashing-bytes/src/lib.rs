//! 外包只用公开Hasher扩展，验证object-safe字节参数不经文本转换。

#[cfg(test)]
mod tests {
    use openrust::{
        Error, Result,
        hashing::{HashInfo, HashManager, HashOptions, Hasher, HashingConfig},
        support::BoxFuture,
    };
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct Recorder(Mutex<Vec<Vec<u8>>>);
    impl Recorder {
        fn record(&self, bytes: &[u8]) -> Result<()> {
            self.0
                .lock()
                .map_err(|_| Error::custom("fixture_poisoned", "字节观察锁失败"))?
                .push(bytes.to_vec());
            Ok(())
        }
    }
    impl Hasher for Recorder {
        fn accepts_exact_input(&self, value: &[u8]) -> Result<bool> {
            self.record(value)?;
            Ok(true)
        }
        fn info(&self, _encoded: &str) -> Result<HashInfo> {
            Ok(HashInfo {
                algo: Some("test-byte-recorder".into()),
                algo_name: "test-byte-recorder".into(),
                options: HashOptions::new(),
            })
        }
        fn make<'a>(
            &'a self,
            value: &'a [u8],
            _options: &'a HashOptions,
        ) -> BoxFuture<'a, Result<String>> {
            Box::pin(async move {
                self.record(value)?;
                Ok("public-fixture-not-a-password-hash".into())
            })
        }
        fn check<'a>(
            &'a self,
            value: &'a [u8],
            encoded: Option<&'a str>,
            _options: &'a HashOptions,
        ) -> BoxFuture<'a, Result<bool>> {
            Box::pin(async move {
                self.record(value)?;
                Ok(encoded == Some("public-fixture-not-a-password-hash"))
            })
        }
        fn needs_rehash(&self, _encoded: &str, _options: &HashOptions) -> Result<bool> {
            Ok(false)
        }
    }

    #[tokio::test]
    async fn manager_and_trait_object_preserve_bytes_at_external_extension() -> Result<()> {
        let recorder = Arc::new(Recorder::default());
        let returned = Arc::clone(&recorder);
        let manager = HashManager::new(HashingConfig::default())?;
        manager.extend("bcrypt", move || {
            Ok(Arc::clone(&returned) as Arc<dyn Hasher>)
        })?;
        let input = [0xff, 0, b'a', 0xfe];
        let options = HashOptions::new();
        let owned = input.to_vec();
        let encoded = manager.make(owned, &options).await?;
        assert!(manager.check(input, Some(&encoded), &options).await?);
        let driver = manager.driver(None)?;
        assert!(driver.accepts_exact_input(&input)?);
        assert_eq!(driver.make(&input, &options).await?, encoded);
        assert!(driver.check(&input, Some(&encoded), &options).await?);
        let observed = recorder
            .0
            .lock()
            .map_err(|_| Error::custom("fixture_poisoned", "字节观察锁失败"))?;
        assert_eq!(observed.as_slice(), vec![input.to_vec(); 5].as_slice());
        Ok(())
    }

    #[tokio::test]
    async fn public_operation_selection_preserves_external_leaf_and_default_hook() -> Result<()> {
        fn assert_send<T: Send>(_: &T) {}
        let recorder = Arc::new(Recorder::default());
        let external_leaf: Arc<dyn Hasher> = recorder.clone();
        assert!(external_leaf.as_hash_manager().is_none());
        let returned = Arc::clone(&external_leaf);
        let inner = Arc::new(HashManager::new(HashingConfig::default())?);
        inner.extend("bcrypt", move || Ok(Arc::clone(&returned)))?;
        let returned_inner = Arc::clone(&inner);
        let outer = HashManager::new(HashingConfig::default())?;
        outer.extend("bcrypt", move || {
            Ok(Arc::clone(&returned_inner) as Arc<dyn Hasher>)
        })?;

        let selected = outer.driver_for_operation()?;
        assert!(Arc::ptr_eq(&selected, &external_leaf));
        assert!(Arc::ptr_eq(&inner.driver(None)?, &external_leaf));
        assert!(outer.driver(None)?.as_hash_manager().is_some());
        let input = [0xff, 0, b'a', 0xfe];
        let options = HashOptions::new();
        assert!(selected.accepts_exact_input(&input)?);
        let pending = selected.make(&input, &options);
        assert_send(&pending);
        let encoded = pending.await?;
        assert!(selected.check(&input, Some(&encoded), &options).await?);
        let observed = recorder
            .0
            .lock()
            .map_err(|_| Error::custom("fixture_poisoned", "字节观察锁失败"))?;
        assert_eq!(observed.as_slice(), vec![input.to_vec(); 3].as_slice());
        Ok(())
    }
}
