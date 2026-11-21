use openrust::contracts::container::Container as _;
use openrust::contracts::foundation::Application as _;
use openrust_telescope::{
    EntryResult, EntryType, EntryUpdate, IncomingEntry, Result, Telescope,
    TelescopeServiceProvider,
    contracts::{ClearableRepository, EntriesRepository, PrunableRepository, TelescopeFuture},
    storage::EntryQueryOptions,
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
#[derive(Default)]
struct Store {
    writes: AtomicUsize,
    terminations: AtomicUsize,
}
impl ClearableRepository for Store {
    fn clear(&self) -> TelescopeFuture<'_, Result<()>> {
        Box::pin(async { Ok(()) })
    }
}
impl PrunableRepository for Store {
    fn prune(&self, _: jiff::Timestamp, _: bool) -> TelescopeFuture<'_, Result<u64>> {
        Box::pin(async { Ok(0) })
    }
}
impl EntriesRepository for Store {
    fn find<'a>(&'a self, _: &'a str) -> TelescopeFuture<'a, Result<EntryResult>> {
        Box::pin(async { Err(openrust_telescope::Error::NotFound) })
    }
    fn get(
        &self,
        _: Option<EntryType>,
        _: EntryQueryOptions,
    ) -> TelescopeFuture<'_, Result<Vec<EntryResult>>> {
        Box::pin(async { Ok(vec![]) })
    }
    fn store<'a>(&'a self, entries: &'a [IncomingEntry]) -> TelescopeFuture<'a, Result<()>> {
        Box::pin(async move {
            self.writes.fetch_add(entries.len(), Ordering::Relaxed);
            Ok(())
        })
    }
    fn update<'a>(
        &'a self,
        updates: &'a [EntryUpdate],
    ) -> TelescopeFuture<'a, Result<Vec<EntryUpdate>>> {
        Box::pin(async move { Ok(updates.to_vec()) })
    }
    fn monitoring(&self) -> TelescopeFuture<'_, Result<Vec<String>>> {
        Box::pin(async { Ok(vec![]) })
    }
    fn loadMonitoredTags(&self) -> TelescopeFuture<'_, ()> {
        Box::pin(async {})
    }
    fn hasMonitoredTag(&self, _: &[String]) -> bool {
        false
    }
    fn isMonitoring<'a>(&'a self, _: &'a [String]) -> TelescopeFuture<'a, bool> {
        Box::pin(async { false })
    }
    fn monitor<'a>(&'a self, _: &'a [String]) -> TelescopeFuture<'a, Result<()>> {
        Box::pin(async { Ok(()) })
    }
    fn stopMonitoring<'a>(&'a self, _: &'a [String]) -> TelescopeFuture<'a, Result<()>> {
        Box::pin(async { Ok(()) })
    }
    fn terminate(&self) {
        self.terminations.fetch_add(1, Ordering::Relaxed);
    }
}
#[tokio::test]
async fn application_can_replace_storage_contract_without_resolving_the_database_driver() {
    let root = tempfile::tempdir().unwrap();
    let mut app = openrust::foundation::Application::configure(root.path().into())
        .withProviders(vec![Box::new(TelescopeServiceProvider)])
        .withConfig(|_, _| {
            Ok(std::collections::BTreeMap::from([(
                "telescope".into(),
                serde_json::json!({"driver":"custom"}),
            )]))
        })
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let store = Arc::new(Store::default());
    app.container()
        .instance::<dyn EntriesRepository>(store.clone())
        .unwrap();
    let telescope = app.make::<Telescope>().await.unwrap();
    telescope.recordLog(IncomingEntry::make(serde_json::Map::new()).unwrap());
    assert!(telescope.store().await.unwrap().is_empty());
    assert_eq!(store.writes.load(Ordering::Relaxed), 1);
    assert_eq!(store.terminations.load(Ordering::Relaxed), 1);
}
