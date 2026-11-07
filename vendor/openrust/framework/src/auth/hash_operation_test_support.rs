//! 增强认证操作回归共用真实 Argon 探针；成本仅属于本测试 fixture。

use crate::{
    Error, Result,
    hashing::{ArgonConfig, ArgonHasher, HashInfo, HashManager, HashOptions, Hasher},
    support::BoxFuture,
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

#[derive(Default)]
pub(in crate::auth) struct Counts {
    pub selections: AtomicUsize,
    pub capabilities: AtomicUsize,
    pub makes: AtomicUsize,
    pub checks: AtomicUsize,
    pub rehashes: AtomicUsize,
    pub made_lengths: Mutex<Vec<usize>>,
}
type Action = Box<dyn FnOnce() -> Result<()> + Send>;
pub(in crate::auth) struct Probe {
    real: Arc<dyn Hasher>,
    prefix: bool,
    counts: Arc<Counts>,
    action: Mutex<Option<Action>>,
}
impl Probe {
    pub(in crate::auth) fn new(prefix: bool) -> Result<(Arc<Self>, Arc<Counts>)> {
        Ok(Self::with_real(
            prefix,
            Arc::new(ArgonHasher::new(ArgonConfig {
                memory: 1024,
                time: 1,
                threads: 1,
                verify: true,
            })?),
        ))
    }
    fn with_real(prefix: bool, real: Arc<dyn Hasher>) -> (Arc<Self>, Arc<Counts>) {
        let counts = Arc::new(Counts::default());
        (
            Arc::new(Self {
                real,
                prefix,
                counts: Arc::clone(&counts),
                action: Mutex::new(None),
            }),
            counts,
        )
    }
    pub(in crate::auth) fn on_capability(
        &self,
        action: impl FnOnce() -> Result<()> + Send + 'static,
    ) -> Result<()> {
        *self.action.lock().map_err(|_| test_error())? = Some(Box::new(action));
        Ok(())
    }
    fn input<'a>(&self, value: &'a [u8]) -> &'a [u8] {
        if self.prefix {
            &value[..value.len().min(72)]
        } else {
            value
        }
    }
}
impl Hasher for Probe {
    fn as_hash_manager(&self) -> Option<&HashManager> {
        self.counts.selections.fetch_add(1, Ordering::SeqCst);
        None
    }
    fn accepts_exact_input(&self, value: &[u8]) -> Result<bool> {
        self.counts.capabilities.fetch_add(1, Ordering::SeqCst);
        let action = self.action.lock().map_err(|_| test_error())?.take();
        if let Some(action) = action {
            action()?;
        }
        Ok(!self.prefix || value.len() <= 72)
    }
    fn info(&self, encoded: &str) -> Result<HashInfo> {
        self.real.info(encoded)
    }
    fn make<'a>(
        &'a self,
        value: &'a [u8],
        options: &'a HashOptions,
    ) -> BoxFuture<'a, Result<String>> {
        Box::pin(async move {
            self.counts.makes.fetch_add(1, Ordering::SeqCst);
            self.counts
                .made_lengths
                .lock()
                .map_err(|_| test_error())?
                .push(value.len());
            self.real.make(self.input(value), options).await
        })
    }
    fn check<'a>(
        &'a self,
        value: &'a [u8],
        encoded: Option<&'a str>,
        options: &'a HashOptions,
    ) -> BoxFuture<'a, Result<bool>> {
        Box::pin(async move {
            self.counts.checks.fetch_add(1, Ordering::SeqCst);
            self.real.check(self.input(value), encoded, options).await
        })
    }
    fn needs_rehash(&self, encoded: &str, options: &HashOptions) -> Result<bool> {
        self.counts.rehashes.fetch_add(1, Ordering::SeqCst);
        self.real.needs_rehash(encoded, options)
    }
}
pub(in crate::auth) fn test_error() -> Error {
    Error::custom("hash_operation_test_state", "哈希操作测试状态无效")
}
pub(in crate::auth) fn live() -> Result<(
    Arc<HashManager>,
    crate::config::repository::ConfigRepository,
)> {
    let repository = crate::config::repository::ConfigRepository::new(figment::Figment::new());
    repository.set("hashing.driver", serde_json::json!("a"))?;
    Ok((
        Arc::new(HashManager::from_repository(repository.clone(), 1)?),
        repository,
    ))
}
pub(in crate::auth) fn install(
    manager: &HashManager,
    name: &str,
    driver: Arc<dyn Hasher>,
) -> Result<()> {
    manager.extend(name, move || Ok(Arc::clone(&driver)))
}
pub(in crate::auth) fn count(value: &AtomicUsize) -> usize {
    value.load(Ordering::SeqCst)
}
