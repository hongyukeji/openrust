use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use openrust::{
    contracts::{container::Container as _, foundation::Application as _, localization::Loader},
    foundation::{Application, BootstrapError},
    http::HttpFuture,
    localization::{
        FileLoader, Line, Lines, Localizer, TranslationError, TranslationFuture, TranslationResult,
    },
    support::{ProviderResult, ServiceProvider, facades::Lang},
};
use serde_json::{Value, json};
use tokio::sync::Notify;

#[derive(Default)]
struct Updates {
    calls: AtomicUsize,
    mode: AtomicUsize,
    started: Notify,
    release: Notify,
}

struct Source {
    files: Arc<FileLoader>,
    updates: Arc<Updates>,
}

impl Loader for Source {
    fn load<'a>(
        &'a self,
        locale: &'a str,
        group: &'a str,
        namespace: Option<&'a str>,
    ) -> TranslationFuture<'a, TranslationResult<Lines>> {
        Box::pin(async move {
            self.updates.calls.fetch_add(1, Ordering::Relaxed);
            let mode = self.updates.mode.load(Ordering::Relaxed);
            if mode == 1 && group == "second" {
                return Err(TranslationError::UnexpectedType);
            }
            let mut lines = self.files.load(locale, group, namespace).await?;
            if group == "messages" {
                if mode == 2 {
                    self.updates.started.notify_one();
                    self.updates.release.notified().await;
                }
                if mode != 4 {
                    lines.insert("title".into(), Line::Text(format!("数据库 {mode}")));
                }
                lines.insert("empty".into(), Line::Text(String::new()));
            }
            Ok(lines)
        })
    }
}

struct Register(Arc<Updates>, PathBuf);

impl ServiceProvider for Register {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            let container = app.container();
            container.instance_named::<dyn Loader>(
                "lang.loader",
                Arc::new(Source {
                    files: container.make::<FileLoader>()?,
                    updates: self.0.clone(),
                }),
            )?;
            Ok(())
        })
    }

    fn boot<'a>(&'a mut self, _: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            Lang::add_path(self.1.clone())?;
            Ok(())
        })
    }
}

fn configured() -> Value {
    json!([{"locale":"fr","group":"messages"},{"locale":"en","group":"second"}])
}

fn fixture(root: &std::path::Path, updates: Arc<Updates>, resources: Value) -> Application {
    let extra = root.join("extra");
    std::fs::create_dir_all(extra.join("fr")).unwrap();
    std::fs::write(
        extra.join("fr/messages.json"),
        r#"{"title":"文件","other":"晚注册","empty":"默认"}"#,
    )
    .unwrap();
    std::fs::write(extra.join("fr/other.json"), r#"{"plain":"独立文件"}"#).unwrap();
    std::fs::create_dir_all(extra.join("en")).unwrap();
    std::fs::write(extra.join("en/messages.json"), r#"{"fallback":"英语回退"}"#).unwrap();
    Application::configure(root.to_owned())
        .with_providers(vec![Box::new(Register(updates, extra))])
        .booting(move |app| {
            app.config()?.set("lang.resources", resources.clone())?;
            Ok(())
        })
        .create()
        .unwrap()
}

fn read(localizer: &Localizer, key: &str) -> String {
    localizer.string(key, &BTreeMap::new(), Some("fr")).unwrap()
}

#[tokio::test]
async fn configured_source_refreshes_atomically_and_keeps_runtime_overrides() {
    let root = tempfile::tempdir().unwrap();
    let updates = Arc::new(Updates::default());
    let mut app = fixture(root.path(), updates.clone(), configured());
    app.boot().await.unwrap();
    let localizer = app.make::<Localizer>().await.unwrap();
    assert_eq!(read(&localizer, "messages.title"), "数据库 0");
    assert_eq!(read(&localizer, "messages.other"), "晚注册");
    assert_eq!(read(&localizer, "messages.empty"), "");
    assert_eq!(read(&localizer, "messages.fallback"), "英语回退");
    assert_eq!(read(&localizer, "other.plain"), "独立文件");
    assert_eq!(updates.calls.load(Ordering::Relaxed), 2);

    updates.mode.store(1, Ordering::Relaxed);
    assert!(matches!(
        localizer.refresh_default_source().await,
        Err(TranslationError::UnexpectedType)
    ));
    assert_eq!(read(&localizer, "messages.title"), "数据库 0");

    updates.mode.store(2, Ordering::Relaxed);
    let older = tokio::spawn({
        let localizer = localizer.clone();
        async move { localizer.refresh_default_source().await }
    });
    updates.started.notified().await;
    assert_eq!(read(&localizer, "messages.title"), "数据库 0");
    localizer
        .add_lines(
            [("messages.override".into(), Line::Text("运行时".into()))],
            "fr",
            None,
        )
        .unwrap();
    updates.mode.store(3, Ordering::Relaxed);
    localizer.refresh_default_source().await.unwrap();
    updates.release.notify_one();
    assert!(matches!(
        older.await.unwrap(),
        Err(TranslationError::CatalogChanged)
    ));
    assert_eq!(read(&localizer, "messages.title"), "数据库 3");
    assert_eq!(read(&localizer, "messages.override"), "运行时");

    updates.mode.store(4, Ordering::Relaxed);
    localizer.refresh_default_source().await.unwrap();
    assert_eq!(read(&localizer, "messages.title"), "文件");
    assert_eq!(read(&localizer, "messages.override"), "运行时");

    app.config()
        .unwrap()
        .set("lang.resources", json!([]))
        .unwrap();
    localizer.refresh_default_source().await.unwrap();
    assert_eq!(read(&localizer, "messages.empty"), "默认");
    assert_eq!(read(&localizer, "messages.override"), "运行时");
}

#[tokio::test]
async fn configured_source_boot_failure_and_empty_file_mode_are_explicit() {
    let root = tempfile::tempdir().unwrap();
    let updates = Arc::new(Updates::default());
    updates.mode.store(1, Ordering::Relaxed);
    let mut app = fixture(root.path(), updates.clone(), configured());
    assert!(matches!(
        app.boot().await,
        Err(BootstrapError::LocalizationFailed {
            source: TranslationError::UnexpectedType
        })
    ));

    let mut app = fixture(root.path(), updates.clone(), json!([]));
    let before = updates.calls.load(Ordering::Relaxed);
    app.boot().await.unwrap();
    let localizer = app.make::<Localizer>().await.unwrap();
    assert_eq!(read(&localizer, "messages.title"), "文件");
    assert_eq!(updates.calls.load(Ordering::Relaxed), before);

    let mut app = fixture(
        root.path(),
        updates,
        json!([{"locale":"en","group":"../escape"}]),
    );
    assert!(matches!(
        app.boot().await,
        Err(BootstrapError::LocalizationFailed {
            source: TranslationError::InvalidKey
        })
    ));
}
