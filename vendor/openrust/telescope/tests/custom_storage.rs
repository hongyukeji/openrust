use openrust::contracts::container::Container as _;
use openrust::contracts::foundation::Application as _;
use openrust_telescope::{
    EntryResult, EntryType, EntryUpdate, IncomingEntry, Result, Telescope,
    TelescopeServiceProvider,
    contracts::{ClearableRepository, EntriesRepository, PrunableRepository, TelescopeFuture},
    storage::EntryQueryOptions,
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
#[derive(Default)]
struct Store {
    writes: AtomicUsize,
    terminations: AtomicUsize,
    initializations: AtomicUsize,
    pruned: Mutex<Vec<(jiff::Timestamp, bool)>>,
    fail: bool,
}
impl ClearableRepository for Store {
    fn clear(&self) -> TelescopeFuture<'_, Result<()>> {
        Box::pin(async { Ok(()) })
    }
}
impl PrunableRepository for Store {
    fn prune(
        &self,
        before: jiff::Timestamp,
        keep_exceptions: bool,
    ) -> TelescopeFuture<'_, Result<u64>> {
        Box::pin(async move {
            self.pruned.lock().unwrap().push((before, keep_exceptions));
            Ok(0)
        })
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
            if self.fail {
                return Err(openrust_telescope::Error::InvalidConfiguration);
            }
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
    fn load_monitored_tags(&self) -> TelescopeFuture<'_, ()> {
        Box::pin(async {
            self.initializations.fetch_add(1, Ordering::Relaxed);
        })
    }
    fn has_monitored_tag(&self, _: &[String]) -> bool {
        false
    }
    fn is_monitoring<'a>(&'a self, _: &'a [String]) -> TelescopeFuture<'a, bool> {
        Box::pin(async { false })
    }
    fn monitor<'a>(&'a self, _: &'a [String]) -> TelescopeFuture<'a, Result<()>> {
        Box::pin(async { Ok(()) })
    }
    fn stop_monitoring<'a>(&'a self, _: &'a [String]) -> TelescopeFuture<'a, Result<()>> {
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
        .with_providers(vec![Box::new(TelescopeServiceProvider)])
        .with_config(|_, _| {
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
    telescope.record_log(IncomingEntry::make(serde_json::Map::new()).unwrap());
    assert!(telescope.store().await.unwrap().is_empty());
    assert_eq!(store.writes.load(Ordering::Relaxed), 1);
    assert_eq!(store.terminations.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn prune_accepts_fractional_hours_and_uses_the_application_clock() {
    use openrust::{
        support::{Date, DateTimezone},
        testing::TestApp,
    };

    let mut app = TestApp::create(|builder| {
        builder
            .with_providers(vec![Box::new(TelescopeServiceProvider)])
            .with_config(|_, _| {
                Ok(std::collections::BTreeMap::from([(
                    "telescope".into(),
                    serde_json::json!({"driver":"custom"}),
                )]))
            })
    })
    .unwrap();
    let store = Arc::new(Store::default());
    app.app()
        .container()
        .instance::<dyn EntriesRepository>(store.clone())
        .unwrap();
    app.travel_to(Date::from_utc(
        "2026-11-26T13:00:00.654321Z".parse().unwrap(),
        DateTimezone::UTC,
    ));

    for (arguments, cutoff, keep_exceptions) in [
        (
            vec!["telescope:prune"],
            "2026-11-25T13:00:00.654321Z",
            false,
        ),
        (
            vec!["telescope:prune", "--hours=.5", "--keep-exceptions"],
            "2026-11-26T12:30:00.654321Z",
            true,
        ),
        (
            vec!["telescope:prune", "--hours=-0.5"],
            "2026-11-26T13:30:00.654321Z",
            false,
        ),
        (
            vec!["telescope:prune", "--hours=1e-1"],
            "2026-11-26T12:54:00.654321Z",
            false,
        ),
        (
            vec!["telescope:prune", "--hours=0.00010000015"],
            "2026-11-26T13:00:00.294320Z",
            false,
        ),
    ] {
        app.artisan(arguments).await.assert_successful();
        assert_eq!(
            store.pruned.lock().unwrap().pop(),
            Some((cutoff.parse().unwrap(), keep_exceptions))
        );
    }

    for option in ["--hours=NaN", "--hours=INF", "--hours=0.000000077"] {
        app.artisan(["telescope:prune", option])
            .await
            .assert_exit_code(1);
    }
    assert!(store.pruned.lock().unwrap().is_empty());
}

#[tokio::test]
async fn initialization_commands_do_not_initialize_or_write_diagnostic_storage() {
    for (arguments, ignored, fail) in [
        (vec!["init", "--dry-run"], true, false),
        (vec!["init:resources", "--extensions", "api"], true, false),
        (vec!["install:full", "--dry-run"], true, false),
        (vec!["diagnostic:probe"], false, false),
        (vec!["diagnostic:probe"], false, true),
    ] {
        let root = tempfile::tempdir().unwrap();
        let mut app = openrust::foundation::Application::configure(root.path().into())
            .with_providers(vec![Box::new(TelescopeServiceProvider)])
            .with_commands(|artisan| {
                artisan.command("diagnostic:probe", |_| {})?;
                Ok(())
            })
            .with_config(|base, _| {
                Ok(std::collections::BTreeMap::from([
                    ("telescope".into(), serde_json::json!({"driver":"custom"})),
                    ("logging".into(), serde_json::json!({"default":"single","channels":{"single":{"driver":"single","path":base.join("app.log"),"level":"debug"}}})),
                    ("cache".into(), serde_json::json!({"default":"array","stores":{"array":{"driver":"array"}}})),
                ]))
            })
            .create()
            .unwrap();
        app.boot().await.unwrap();
        let store = Arc::new(Store {
            fail,
            ..Store::default()
        });
        let storage = store.clone();
        app.container()
            .bind::<dyn EntriesRepository, _>(move |_| Ok(storage.clone()))
            .unwrap();

        // 不完整的应用会由命令自身报错，诊断采集不得抢先访问存储。
        let _result = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            app.handle_command(&arguments.into_iter().map(Into::into).collect::<Vec<_>>()),
        )
        .await
        .expect("初始化命令不应等待诊断存储");
        if ignored {
            assert_eq!(store.initializations.load(Ordering::Relaxed), 0);
            assert_eq!(store.writes.load(Ordering::Relaxed), 0);
        } else {
            assert!(store.initializations.load(Ordering::Relaxed) > 0);
            assert!(store.writes.load(Ordering::Relaxed) > 0);
            assert_eq!(store.terminations.load(Ordering::Relaxed), 1);
            assert_eq!(_result.unwrap(), 0);
        }
    }
}
