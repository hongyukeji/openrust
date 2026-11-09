use std::sync::{Arc, Mutex};

use figment::{Figment, providers::Serialized};

use super::*;
use crate::{ModuleDescriptor, ModuleManifest, ModuleProviderBuilderExt};
use openrust::events::{EventPropagation, EventServiceProvider, EventsServiceProvider};
use openrust::foundation::BootContext;
use openrust::foundation::ProviderBuilder;
use openrust::support::{BoxFuture, ServiceProvider};

type TestResult<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

struct Commands;
impl ServiceProvider for Commands {
    fn boot<'a>(&'a self, context: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            super::super::commands::register_commands(context);
            Ok(())
        })
    }
}

fn manifest(root: &Path) -> TestResult<ModuleManifest> {
    for name in ["blog", "taxonomy"] {
        std::fs::create_dir_all(root.join(name))?;
    }
    Ok(ModuleManifest::from_file(root.join("statuses.json"))
        .module(ModuleDescriptor::new("Blog", root.join("blog"), 10))
        .module(ModuleDescriptor::new("Taxonomy", root.join("taxonomy"), -1)))
}

async fn build(
    manifest: ModuleManifest,
    events: Option<EventsServiceProvider>,
) -> TestResult<openrust::Application> {
    let mut builder = ProviderBuilder::new();
    builder
        .provider(EventServiceProvider)
        .module_manifest(manifest)
        .provider(Commands);
    if let Some(events) = events {
        builder.provider(events);
    }
    Ok(openrust::Application::from_provider_runtime(
        builder
            .start(Figment::from(Serialized::defaults(
                openrust::config::FrameworkConfig::default(),
            )))
            .await?,
    )?)
}

fn names(modules: Vec<Module>) -> Vec<String> {
    modules
        .into_iter()
        .map(|module| module.name().to_owned())
        .collect()
}

async fn close(app: &openrust::Application) -> Result<()> {
    app.shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(2))
        .await
}

// 区分持久键的精确比较、Module的否定语义、canonical写入和已加载快照。
#[tokio::test]
async fn persistent_keys_are_exact_and_non_boolean_is_not_false_status() -> TestResult {
    let root = tempfile::tempdir()?;
    std::fs::write(
        root.path().join("statuses.json"),
        r#"{"blog":true,"Old":true,"Taxonomy":"true"}"#,
    )?;
    let app = build(manifest(root.path())?, None).await?;
    let repository = app.state().resolve::<ModuleRepository>().await?;
    let again = app.state().resolve::<ModuleRepository>().await?;
    assert!(Arc::ptr_eq(&repository, &again));
    assert_eq!(names(repository.get_by_status(false)?), ["Blog"]);
    let taxonomy = repository.find_or_fail("taxonomy")?;
    assert!(taxonomy.is_disabled()?);
    assert!(!taxonomy.has_status(false)?);
    let blog = repository.find_or_fail("blog")?;
    assert!(!blog.is_loaded());
    blog.enable().await?;
    assert!(blog.is_enabled()?);
    assert!(!blog.is_loaded());
    let snapshot = repository.snapshot()?;
    assert!(snapshot.is_enabled("blog"));
    assert!(snapshot.is_enabled("Blog"));
    assert!(snapshot.is_enabled("Old"));
    close(&app).await?;
    Ok(())
}

// 防止空默认Events Provider吞掉应用监听，或直接setActive误发完整启停事件。
#[tokio::test]
async fn configured_events_are_preserved_and_direct_set_has_no_lifecycle_events() -> TestResult {
    let root = tempfile::tempdir()?;
    std::fs::write(root.path().join("statuses.json"), r#"{"Blog":false}"#)?;
    let calls = Arc::new(Mutex::new(Vec::new()));
    let observed = Arc::clone(&calls);
    let mut events = EventsServiceProvider::new();
    events.listen_named::<ModuleLifecycleEvent, _>("modules.*", move |event, context| {
        let observed = Arc::clone(&observed);
        Box::pin(async move {
            assert_eq!(context.event_name(), event.event_name().as_str());
            observed
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push((event.event_name(), event.module.is_enabled()?));
            Ok(EventPropagation::Continue)
        })
    });
    let app = build(manifest(root.path())?, Some(events)).await?;
    let repository = app.state().resolve::<ModuleRepository>().await?;
    let blog = repository.find_or_fail("BLOG")?;
    blog.enable().await?;
    assert_eq!(
        *calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
        [
            ("modules.blog.enabling".to_owned(), false),
            ("modules.blog.enabled".to_owned(), true),
        ]
    );
    blog.set_active(false)?;
    assert_eq!(
        calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len(),
        2
    );
    close(&app).await?;
    Ok(())
}

#[tokio::test]
async fn before_error_does_not_write_and_after_error_does_not_undo_commit() -> TestResult {
    for after in [false, true] {
        let root = tempfile::tempdir()?;
        let original = r#"{"Blog":false,"keep":null}"#;
        std::fs::write(root.path().join("statuses.json"), original)?;
        let mut events = EventsServiceProvider::new();
        events.listen_named::<ModuleLifecycleEvent, _>("modules.blog.*", move |event, _| {
            Box::pin(async move {
                if event.phase
                    == if after {
                        ModuleLifecyclePhase::Enabled
                    } else {
                        ModuleLifecyclePhase::Enabling
                    }
                {
                    return Err(Error::custom("listener_rejected", "测试监听拒绝"));
                }
                Ok(EventPropagation::Continue)
            })
        });
        let app = build(manifest(root.path())?, Some(events)).await?;
        let repository = app.state().resolve::<ModuleRepository>().await?;
        let error = repository
            .enable("Blog")
            .await
            .err()
            .ok_or("listener did not reject")?;
        assert_eq!(error.code(), "listener_rejected");
        assert_eq!(repository.find_or_fail("Blog")?.is_enabled()?, after);
        if !after {
            assert_eq!(
                std::fs::read_to_string(root.path().join("statuses.json"))?,
                original
            );
        }
        close(&app).await?;
    }
    Ok(())
}

#[tokio::test]
async fn stopped_event_propagation_does_not_cancel_activation() -> TestResult {
    let root = tempfile::tempdir()?;
    let calls = Arc::new(Mutex::new(Vec::new()));
    let first = Arc::clone(&calls);
    let mut events = EventsServiceProvider::new();
    events.listen_named::<ModuleLifecycleEvent, _>("modules.blog.*", move |event, _| {
        let first = Arc::clone(&first);
        Box::pin(async move {
            first
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(event.phase);
            Ok(EventPropagation::Stop)
        })
    });
    events.listen_named::<ModuleLifecycleEvent, _>("*", |_, _| {
        Box::pin(async { Err(Error::custom("unexpected_listener", "停止后不应调用")) })
    });
    let app = build(manifest(root.path())?, Some(events)).await?;
    let repository = app.state().resolve::<ModuleRepository>().await?;
    repository.enable("Blog").await?;
    assert!(repository.find_or_fail("Blog")?.is_enabled()?);
    assert_eq!(
        *calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
        [
            ModuleLifecyclePhase::Enabling,
            ModuleLifecyclePhase::Enabled
        ]
    );
    close(&app).await?;
    Ok(())
}

#[tokio::test]
async fn command_prevalidates_all_names_and_keeps_exception_partial_progress() -> TestResult {
    let root = tempfile::tempdir()?;
    let original = r#"{"Blog":false,"Taxonomy":false}"#;
    std::fs::write(root.path().join("statuses.json"), original)?;
    let calls = Arc::new(Mutex::new(Vec::new()));
    let observed = Arc::clone(&calls);
    let mut events = EventsServiceProvider::new();
    events.listen_named::<ModuleLifecycleEvent, _>("modules.*", move |event, _| {
        let observed = Arc::clone(&observed);
        Box::pin(async move {
            observed
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(event.event_name());
            Ok(EventPropagation::Continue)
        })
    });
    events.listen_named::<ModuleLifecycleEvent, _>("modules.taxonomy.enabling", |_, _| {
        Box::pin(async { Err(Error::custom("taxonomy_rejected", "测试拒绝第二项")) })
    });
    let app = build(manifest(root.path())?, Some(events)).await?;
    let repository = app.state().resolve::<ModuleRepository>().await?;
    let unknown = app
        .run_command("module:enable", vec!["Blog".into(), "Missing".into()])
        .await;
    assert!(unknown.is_err());
    assert!(
        calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_empty()
    );
    assert_eq!(
        std::fs::read_to_string(root.path().join("statuses.json"))?,
        original
    );
    let missing = app
        .run_command("module:enable", vec!["--no-interaction".into()])
        .await;
    assert!(missing.is_err());
    let error = app
        .run_command(
            "module:enable",
            vec!["Blog".into(), "Taxonomy".into(), "Blog".into()],
        )
        .await
        .err()
        .ok_or("second action did not fail")?;
    assert_eq!(error.code(), "taxonomy_rejected");
    assert_eq!(
        *calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
        ["modules.blog.enabling", "modules.blog.enabled",]
    );
    assert!(repository.find_or_fail("Blog")?.is_enabled()?);
    assert!(!repository.find_or_fail("Taxonomy")?.is_enabled()?);
    app.run_command(
        "module:disable",
        vec!["--all".into(), "Missing".into(), "-n".into()],
    )
    .await?;
    assert!(repository.get_by_status(true)?.is_empty());
    close(&app).await?;
    Ok(())
}

// only未用排序时可选direction的null不应提前失败；priority才触发上游类型边界。
#[tokio::test]
async fn list_optional_direction_is_only_required_for_priority_sorting() -> TestResult {
    let root = tempfile::tempdir()?;
    std::fs::write(
        root.path().join("statuses.json"),
        r#"{"Blog":true,"Taxonomy":true}"#,
    )?;
    let app = build(manifest(root.path())?, None).await?;
    let repository = app.state().resolve::<ModuleRepository>().await?;
    assert_eq!(names(repository.get_ordered("asc")?), ["Taxonomy", "Blog"]);
    assert_eq!(names(repository.get_ordered("desc")?), ["Blog", "Taxonomy"]);
    assert_eq!(
        names(repository.get_ordered("other")?),
        ["Taxonomy", "Blog"]
    );
    for only in ["enabled", "disabled", "unknown"] {
        app.run_command(
            "module:list",
            vec![format!("--only={only}"), "--direction".into()],
        )
        .await?;
    }
    app.run_command("module:list", vec!["--direction".into()])
        .await?;
    assert!(
        app.run_command(
            "module:list",
            vec!["--only=priority".into(), "--direction".into()]
        )
        .await
        .is_err()
    );
    close(&app).await?;
    Ok(())
}

// 相同名称的每次动作都必须触发事件；空目录的 --all 是成功空操作。
#[tokio::test]
async fn duplicate_names_repeat_actions_and_empty_all_succeeds() -> TestResult {
    let root = tempfile::tempdir()?;
    let calls = Arc::new(Mutex::new(Vec::new()));
    let observed = Arc::clone(&calls);
    let mut events = EventsServiceProvider::new();
    events.listen_named::<ModuleLifecycleEvent, _>("modules.*", move |event, _| {
        let observed = Arc::clone(&observed);
        Box::pin(async move {
            observed
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(event.event_name());
            Ok(EventPropagation::Continue)
        })
    });
    let app = build(manifest(root.path())?, Some(events)).await?;
    app.run_command("module:enable", vec!["blog".into(), "Blog".into()])
        .await?;
    assert_eq!(
        *calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
        [
            "modules.blog.enabling",
            "modules.blog.enabled",
            "modules.blog.enabling",
            "modules.blog.enabled"
        ]
    );
    close(&app).await?;
    let empty = build(
        ModuleManifest::from_file(root.path().join("empty.json")),
        None,
    )
    .await?;
    empty
        .run_command("module:enable", vec!["--all".into(), "Unknown".into()])
        .await?;
    assert!(!root.path().join("empty.json").exists());
    close(&empty).await?;
    Ok(())
}

// 只读清单即使没有模块，也必须在 --all 的空循环前拒绝写命令。
#[tokio::test]
async fn registered_empty_all_rejects_status_commands() -> TestResult {
    let app = build(ModuleManifest::new(), None).await?;
    for command in ["module:enable", "module:disable"] {
        let error = app
            .run_command(command, vec!["--all".into()])
            .await
            .err()
            .ok_or("readonly empty catalog accepted mutation")?;
        assert_eq!(error.code(), "module_activator_read_only");
    }
    close(&app).await?;
    Ok(())
}

// 只读拒绝必须发生在真实事件派发前，避免监听器已产生不可回滚副作用。
#[tokio::test]
async fn registered_status_changes_never_dispatch_before_events() -> TestResult {
    let root = tempfile::tempdir()?;
    let calls = Arc::new(Mutex::new(Vec::new()));
    let observed = Arc::clone(&calls);
    let mut events = EventsServiceProvider::new();
    events.listen_named::<ModuleLifecycleEvent, _>("modules.*", move |event, _| {
        let observed = Arc::clone(&observed);
        Box::pin(async move {
            observed
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(event.phase);
            Ok(EventPropagation::Continue)
        })
    });
    let app = build(
        ModuleManifest::new()
            .module(ModuleDescriptor::new("Blog", root.path(), 0))
            .enable("Blog"),
        Some(events),
    )
    .await?;
    let repository = app.state().resolve::<ModuleRepository>().await?;
    let blog = repository.find_or_fail("Blog")?;
    for result in [
        blog.enable().await,
        blog.disable().await,
        blog.set_active(false),
        repository.enable("Blog").await,
        repository.disable("Blog").await,
        app.run_command("module:enable", vec!["Blog".into()]).await,
        app.run_command("module:disable", vec!["--all".into()])
            .await,
    ] {
        assert_eq!(
            result
                .err()
                .ok_or("readonly catalog accepted mutation")?
                .code(),
            "module_activator_read_only"
        );
    }
    assert!(
        calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_empty()
    );
    assert!(blog.is_enabled()?);
    close(&app).await?;
    Ok(())
}

#[tokio::test]
async fn programmatic_alias_is_canonical_but_temporary_disable_is_only_loaded_mask() -> TestResult {
    let root = tempfile::tempdir()?;
    let mut manifest = ModuleManifest::new()
        .module(ModuleDescriptor::new("BlogPost", root.path(), 0))
        .enable("blog_post");
    manifest.disable("BLOGPOST");
    let app = build(manifest, None).await?;
    let repository = app.state().resolve::<ModuleRepository>().await?;
    let module = repository.find_or_fail("blog-post")?;
    assert!(module.is_enabled()?);
    assert!(!module.is_loaded());
    assert_eq!(
        module
            .set_active(false)
            .err()
            .ok_or("readonly driver wrote")?
            .code(),
        "module_activator_read_only"
    );
    close(&app).await?;
    Ok(())
}

#[tokio::test]
async fn module_regular_listener_replacement_keeps_shared_dispatcher() -> TestResult {
    let root = tempfile::tempdir()?;
    let mut app = build(manifest(root.path())?, None).await?;
    let repository = app.state().resolve::<ModuleRepository>().await?;
    let blog = repository.find_or_fail("Blog")?;
    blog.enable().await?;
    let calls = Arc::new(Mutex::new(Vec::new()));
    let observed = Arc::clone(&calls);
    let mut replacement = EventsServiceProvider::new();
    replacement.listen_named::<ModuleLifecycleEvent, _>("modules.blog.*", move |event, _| {
        let observed = Arc::clone(&observed);
        Box::pin(async move {
            observed
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(event.phase);
            Ok(EventPropagation::Continue)
        })
    });
    let dispatcher = app.state().resolve::<Dispatcher>().await?;
    app.force_register_provider(replacement).await?;
    assert!(Arc::ptr_eq(
        &dispatcher,
        &app.state().resolve::<Dispatcher>().await?
    ));
    blog.disable().await?;
    assert_eq!(
        *calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
        [
            ModuleLifecyclePhase::Disabling,
            ModuleLifecyclePhase::Disabled
        ]
    );
    close(&app).await?;
    Ok(())
}

#[tokio::test]
async fn dropping_pending_before_event_never_reaches_file_commit() -> TestResult {
    let root = tempfile::tempdir()?;
    let original = r#"{"Blog":false}"#;
    std::fs::write(root.path().join("statuses.json"), original)?;
    let mut events = EventsServiceProvider::new();
    events.listen_named::<ModuleLifecycleEvent, _>("modules.blog.enabling", |_, _| {
        Box::pin(std::future::pending())
    });
    let app = build(manifest(root.path())?, Some(events)).await?;
    let repository = app.state().resolve::<ModuleRepository>().await?;
    let blog = repository.find_or_fail("Blog")?;
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(10), blog.enable())
            .await
            .is_err()
    );
    assert_eq!(
        std::fs::read_to_string(root.path().join("statuses.json"))?,
        original
    );
    close(&app).await?;
    Ok(())
}

#[tokio::test]
async fn closing_app_in_before_event_prevents_late_file_commit() -> TestResult {
    let root = tempfile::tempdir()?;
    let original = r#"{"Blog":false}"#;
    std::fs::write(root.path().join("statuses.json"), original)?;
    let owner = Arc::new(std::sync::OnceLock::<std::sync::Weak<openrust::Application>>::new());
    let observed = Arc::clone(&owner);
    let mut events = EventsServiceProvider::new();
    events.listen_named::<ModuleLifecycleEvent, _>("modules.blog.enabling", move |_, _| {
        let observed = Arc::clone(&observed);
        Box::pin(async move {
            let app = observed
                .get()
                .and_then(std::sync::Weak::upgrade)
                .ok_or_else(|| Error::custom("owner_missing", "测试应用不存在"))?;
            close(&app).await?;
            Ok(EventPropagation::Continue)
        })
    });
    let app = Arc::new(build(manifest(root.path())?, Some(events)).await?);
    owner
        .set(Arc::downgrade(&app))
        .map_err(|_| "duplicate owner")?;
    let repository = app.state().resolve::<ModuleRepository>().await?;
    assert!(repository.enable("Blog").await.is_err());
    assert_eq!(
        std::fs::read_to_string(root.path().join("statuses.json"))?,
        original
    );
    close(&app).await?;
    Ok(())
}

#[tokio::test]
async fn singleton_module_handle_survives_root_scoped_reset() -> TestResult {
    let root = tempfile::tempdir()?;
    let app = build(manifest(root.path())?, None).await?;
    let repository = app.state().resolve::<ModuleRepository>().await?;
    let blog = repository.find_or_fail("Blog")?;
    blog.enable().await?;
    app.state()
        .container()
        .forget_scoped_instances(tokio::time::Instant::now() + std::time::Duration::from_secs(2))
        .await?;
    blog.disable().await?;
    assert!(blog.is_disabled()?);
    assert!(Arc::ptr_eq(
        &repository,
        &app.state().resolve::<ModuleRepository>().await?
    ));
    close(&app).await?;
    Ok(())
}
