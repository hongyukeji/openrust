use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
};

use openrust::{
    console::{ConsoleError, ConsoleFuture, Output},
    contracts::{container::Container as _, foundation::Application as _},
    database::eloquent::{ModelPruner, ModelPruners},
    foundation::Application,
    support::facades::{App, Artisan, Facade as _},
};

#[derive(Default)]
struct Archive(AtomicU32);

struct ArchivePruner;

impl ModelPruner for ArchivePruner {
    fn count<'a>(&'a self, app: &'a Application) -> ConsoleFuture<'a, Result<u64, ConsoleError>> {
        Box::pin(async move {
            let rows = app
                .make::<Archive>()
                .await
                .map_err(ConsoleError::from_source)?;
            Ok(u64::from(rows.0.load(Ordering::Relaxed)))
        })
    }

    fn prune_batch<'a>(
        &'a self,
        app: &'a Application,
        chunk: u32,
    ) -> ConsoleFuture<'a, Result<u64, ConsoleError>> {
        Box::pin(async move {
            let rows = app
                .make::<Archive>()
                .await
                .map_err(ConsoleError::from_source)?;
            let mut before = rows.0.load(Ordering::Relaxed);
            while let Err(observed) = rows.0.compare_exchange_weak(
                before,
                before.saturating_sub(chunk),
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                before = observed;
            }
            Ok(u64::from(before.min(chunk)))
        })
    }

    fn chunk_size(&self, requested: u32) -> u32 {
        requested.min(2)
    }
}

async fn application(root: &std::path::Path) -> Application {
    let mut app = Application::configure(root.into()).create().unwrap();
    app.boot().await.unwrap();
    app
}

async fn prune(app: &Application, options: &[&str]) -> String {
    App::set_facade_application(Some(app), async {
        let mut output = Output::default();
        let options: Vec<std::ffi::OsString> =
            options.iter().map(std::ffi::OsString::from).collect();
        assert_eq!(
            Artisan::call("model:prune", &options, &mut output)
                .await
                .unwrap(),
            0
        );
        output.stdout
    })
    .await
}

#[tokio::test]
async fn compiled_application_pruners_are_discovered_only_in_their_owner() {
    let root = tempfile::tempdir().unwrap();
    let app = application(root.path()).await;
    let isolated = application(root.path()).await;
    let rows = Arc::new(Archive(AtomicU32::new(3)));
    app.container().instance(rows.clone()).unwrap();
    let pruners = app.make::<ModelPruners>().await.unwrap();
    pruners
        .register("Archive", "app/models/archive.rs", &ArchivePruner)
        .unwrap();
    assert!(
        pruners
            .register("Archive", "app/models/other.rs", &ArchivePruner)
            .is_err()
    );
    assert!(
        prune(&isolated, &["--model=Archive"])
            .await
            .contains("没有找到可清理模型")
    );
    assert!(
        prune(&app, &["--except=Archive"])
            .await
            .contains("没有找到可清理模型")
    );
    assert!(prune(&app, &["--pretend"]).await.contains("可清理 3 条"));
    assert_eq!(rows.0.load(Ordering::Relaxed), 3);
    let output = prune(&app, &[]).await;
    assert!(output.contains("已清理 2 条") && output.contains("已清理 3 条"));
    assert_eq!(rows.0.load(Ordering::Relaxed), 0);
}
