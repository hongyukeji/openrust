//! 运行发现进入生产 Provider 管线的行为回归，不执行未编译文件。

#![allow(clippy::unwrap_used, reason = "测试断言使用受控内存日志")]

use super::*;
use crate::ModuleProviderBuilderExt;
use figment::{Figment, providers::Serialized};
use openrust::config::FrameworkConfig;
use openrust::foundation::BootContext;
use openrust::foundation::ProviderBuilder;
use openrust::foundation::ProviderRuntime;
use serde_json::json;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::time::Instant;

type TestResult<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
type Trace = Arc<Mutex<Vec<&'static str>>>;

struct Selected(Trace);
impl ServiceProvider for Selected {
    fn register<'a>(&'a self, app: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            self.0.lock().unwrap().push("provider.register");
            app.instance(Arc::new(31_u16)).await
        })
    }
    fn boot<'a>(&'a self, _: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            self.0.lock().unwrap().push("provider.boot");
            Ok(())
        })
    }
}
struct Included {
    trace: Trace,
    fail: bool,
}
impl ModuleFile for Included {
    fn initialize<'a>(&'a self, app: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            self.trace.lock().unwrap().push("file.initialize");
            assert_eq!(*app.resolve::<u16>().await?, 31);
            app.instance(Arc::new(59_u32)).await?;
            if self.fail {
                return Err(Error::custom("fixture_file_failure", "测试文件初始化失败"));
            }
            Ok(())
        })
    }
}
fn write_module(root: &Path, relative: &str, value: &Value) -> TestResult {
    let path = root.join(relative);
    std::fs::create_dir_all(&path)?;
    std::fs::write(path.join("module.json"), serde_json::to_vec(value)?)?;
    std::fs::write(path.join("init.rs"), "// 运行路径凭证，代码已经显式编译\n")?;
    Ok(())
}
fn fixture() -> TestResult<tempfile::TempDir> {
    let root = tempfile::tempdir()?;
    write_module(
        root.path(),
        "modules/Blog",
        &json!({"name":"Blog","providers":[["Selected"],"Selected"],"files":["init.rs","./init.rs"]}),
    )?;
    std::fs::write(
        root.path().join("modules_statuses.json"),
        r#"{"Blog":true}"#,
    )?;
    Ok(root)
}
fn manifest(root: &Path, trace: &Trace, constructions: Arc<AtomicUsize>) -> ModuleManifest {
    let trace_provider = Arc::clone(trace);
    ModuleManifest::discover_from_config(root, &ModulesConfig::default())
        .compiled_provider("Selected", move |_| {
            constructions.fetch_add(1, Ordering::SeqCst);
            Ok(Selected(Arc::clone(&trace_provider)))
        })
        .compiled_module_file(
            "Blog",
            "init.rs",
            Included {
                trace: Arc::clone(trace),
                fail: false,
            },
        )
}
fn sources() -> Figment {
    Figment::from(Serialized::defaults(FrameworkConfig::default()))
}
async fn shutdown(runtime: ProviderRuntime) -> Result<()> {
    runtime
        .shutdown(Instant::now() + Duration::from_secs(2))
        .await
}

#[tokio::test]
async fn declaration_duplicates_construct_but_register_once_and_files_precede_boot() -> TestResult {
    let root = fixture()?;
    let trace = Trace::default();
    let constructions = Arc::new(AtomicUsize::new(0));
    let mut builder = ProviderBuilder::new();
    // 重复扫描不抹去 Manifest 声明；仓储 lookup 则按 canonical 名最后项覆盖。
    builder.module_manifest(
        manifest(root.path(), &trace, Arc::clone(&constructions)).add_location("modules"),
    );
    let runtime = builder.start(sources()).await?;
    assert_eq!(constructions.load(Ordering::SeqCst), 4);
    assert_eq!(
        *trace.lock().unwrap(),
        ["provider.register", "file.initialize", "provider.boot"]
    );
    let repository = runtime.container().resolve::<ModuleRepository>().await?;
    assert_eq!(repository.all().len(), 1);
    assert!(repository.find_or_fail("Blog")?.is_loaded());
    assert_eq!(*runtime.container().resolve::<u32>().await?, 59);
    shutdown(runtime).await?;
    Ok(())
}

#[tokio::test]
async fn disabled_uncompiled_metadata_can_enable_then_next_load_fails() -> TestResult {
    let root = tempfile::tempdir()?;
    write_module(
        root.path(),
        "modules/Unknown",
        &json!({"name":"Unknown","providers":["NotCompiled"]}),
    )?;
    let mut builder = ProviderBuilder::new();
    builder.module_manifest(ModuleManifest::discover_from_config(
        root.path(),
        &ModulesConfig::default(),
    ));
    let runtime = builder.start(sources()).await?;
    let repository = runtime.container().resolve::<ModuleRepository>().await?;
    let module = repository.find_or_fail("unknown")?;
    assert!(!module.is_loaded());
    repository.enable("Unknown").await?;
    assert!(module.is_enabled()?);
    assert!(!module.is_loaded());
    shutdown(runtime).await?;
    let mut next = ProviderBuilder::new();
    next.module_manifest(ModuleManifest::discover_from_config(
        root.path(),
        &ModulesConfig::default(),
    ));
    let error = next
        .start(sources())
        .await
        .err()
        .ok_or("必须在下次实际加载拒绝未知编译绑定")?;
    assert_eq!(error.code(), "module_provider_not_compiled");
    Ok(())
}

struct ChangePath(PathBuf);
impl ServiceProvider for ChangePath {
    fn register<'a>(&'a self, app: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.config_repository()
                .set("modules.paths.modules", json!(self.0))
        })
    }
}
#[tokio::test]
async fn package_reads_prior_register_path_changes_and_repository_getter_stays_live() -> TestResult
{
    let root = tempfile::tempdir()?;
    write_module(
        root.path(),
        "alternate/Blog",
        &json!({"name":"Blog","providers":["Selected"]}),
    )?;
    std::fs::write(
        root.path().join("modules_statuses.json"),
        r#"{"Blog":true}"#,
    )?;
    let trace = Trace::default();
    let mut builder = ProviderBuilder::new();
    builder
        .provider(ChangePath("alternate".into()))
        .module_manifest(manifest(root.path(), &trace, Arc::default()));
    let runtime = builder.start(sources()).await?;
    let repository = runtime.container().resolve::<ModuleRepository>().await?;
    assert!(repository.find_or_fail("Blog")?.is_loaded());
    assert_eq!(repository.get_path()?, root.path().join("alternate"));
    runtime
        .config()
        .set("modules.paths.modules", json!("future"))?;
    assert_eq!(repository.get_path()?, root.path().join("future"));
    assert_eq!(repository.all().len(), 1);
    shutdown(runtime).await?;
    Ok(())
}

struct Depends(Trace);
impl ServiceProvider for Depends {
    fn dependencies(&self) -> Vec<ProviderId> {
        vec![ProviderId::of::<Selected>()]
    }
    fn register<'a>(&'a self, _: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            self.0.lock().unwrap().push("dependent.register");
            Ok(())
        })
    }
}
#[tokio::test]
async fn candidates_are_checked_at_real_registration_not_promoted_to_ready() -> TestResult {
    for enabled in [true, false] {
        let root = fixture()?;
        std::fs::write(
            root.path().join("modules_statuses.json"),
            serde_json::to_vec(&json!({"Blog":enabled}))?,
        )?;
        let trace = Trace::default();
        let mut builder = ProviderBuilder::new();
        builder
            .module_manifest(manifest(root.path(), &trace, Arc::default()))
            .provider(Depends(Arc::clone(&trace)));
        let result = builder.start(sources()).await;
        if enabled {
            let runtime = result?;
            assert!(trace.lock().unwrap().contains(&"dependent.register"));
            shutdown(runtime).await?;
        } else {
            let error = result.err().ok_or("禁用候选不能满足实际依赖")?;
            assert_eq!(error.code(), "provider_order_invalid");
            assert!(!trace.lock().unwrap().contains(&"dependent.register"));
        }
    }
    Ok(())
}
#[tokio::test]
async fn candidate_order_and_mask_still_fail_before_callbacks() -> TestResult {
    for masked in [false, true] {
        let root = fixture()?;
        let trace = Trace::default();
        let mut builder = ProviderBuilder::new();
        if !masked {
            builder.provider(Depends(Arc::clone(&trace)));
        }
        builder.module_manifest(manifest(root.path(), &trace, Arc::default()));
        if masked {
            builder
                .remove_provider(ProviderId::of::<Selected>())
                .provider(Depends(Arc::clone(&trace)));
        }
        let error = builder
            .start(sources())
            .await
            .err()
            .ok_or("顺序或mask必须拒绝")?;
        assert_eq!(
            error.code(),
            if masked {
                "provider_dependency_missing"
            } else {
                "provider_order_invalid"
            }
        );
        assert!(trace.lock().unwrap().is_empty());
    }
    Ok(())
}

#[tokio::test]
async fn shared_execution_deduplicates_real_path_across_apps_but_new_execution_repeats()
-> TestResult {
    let root = fixture()?;
    let trace = Trace::default();
    let execution = Arc::new(ModuleExecution::new());
    for shared in [
        Some(Arc::clone(&execution)),
        Some(Arc::clone(&execution)),
        None,
    ] {
        let mut manifest = manifest(root.path(), &trace, Arc::default());
        if let Some(shared) = shared {
            manifest = manifest.execution(shared);
        }
        let mut builder = ProviderBuilder::new();
        builder.module_manifest(manifest);
        shutdown(builder.start(sources()).await?).await?;
    }
    assert_eq!(
        trace
            .lock()
            .unwrap()
            .iter()
            .filter(|value| **value == "file.initialize")
            .count(),
        2
    );
    Ok(())
}

#[tokio::test]
async fn disabled_bad_json_still_fails_before_status_filter_or_factory() -> TestResult {
    let root = fixture()?;
    write_module(root.path(), "modules/Disabled", &json!({"name":"Disabled"}))?;
    std::fs::write(root.path().join("modules/Disabled/module.json"), "{broken")?;
    let trace = Trace::default();
    let mut builder = ProviderBuilder::new();
    builder.module_manifest(manifest(root.path(), &trace, Arc::default()));
    let error = builder
        .start(sources())
        .await
        .err()
        .ok_or("禁用不能隐藏坏JSON")?;
    assert_eq!(error.code(), "module_manifest_invalid");
    assert!(trace.lock().unwrap().is_empty());
    Ok(())
}

#[tokio::test]
async fn later_same_name_wins_repository_without_removing_manifest_provider_declarations()
-> TestResult {
    let root = fixture()?;
    write_module(
        root.path(),
        "modules/ZZBlog",
        &json!({"name":"blog","priority":"12","providers":"Selected"}),
    )?;
    std::fs::write(
        root.path().join("modules_statuses.json"),
        r#"{"Blog":true,"blog":true}"#,
    )?;
    let trace = Trace::default();
    let constructions = Arc::new(AtomicUsize::new(0));
    let mut builder = ProviderBuilder::new();
    builder.module_manifest(manifest(root.path(), &trace, Arc::clone(&constructions)));
    let runtime = builder.start(sources()).await?;
    let repository = runtime.container().resolve::<ModuleRepository>().await?;
    assert_eq!(repository.all().len(), 1);
    assert_eq!(
        repository.find_or_fail("BLOG")?.path(),
        root.path().join("modules/ZZBlog")
    );
    assert_eq!(repository.find_or_fail("BLOG")?.priority(), &json!("12"));
    assert_eq!(constructions.load(Ordering::SeqCst), 3);
    shutdown(runtime).await?;
    Ok(())
}

struct GatedFile {
    entered: Arc<tokio::sync::Notify>,
    release: Arc<tokio::sync::Notify>,
    fail: bool,
}
impl ModuleFile for GatedFile {
    fn initialize<'a>(&'a self, _: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            self.entered.notify_one();
            self.release.notified().await;
            if self.fail {
                Err(Error::custom("fixture_file_failure", "测试初始化失败"))
            } else {
                Ok(())
            }
        })
    }
}
fn gated_builder(
    root: &Path,
    execution: Arc<ModuleExecution>,
    entered: Arc<tokio::sync::Notify>,
    release: Arc<tokio::sync::Notify>,
    fail: bool,
) -> ProviderBuilder {
    let mut builder = ProviderBuilder::new();
    builder.module_manifest(
        ModuleManifest::discover_from_config(root, &ModulesConfig::default())
            .compiled_provider("Selected", |_| Ok(Selected(Trace::default())))
            .compiled_module_file(
                "Blog",
                "init.rs",
                GatedFile {
                    entered,
                    release,
                    fail,
                },
            )
            .execution(execution),
    );
    builder
}
#[tokio::test]
async fn concurrent_execution_waits_and_propagates_success_failure_or_cancellation() -> TestResult {
    for outcome in ["success", "failure", "cancelled"] {
        let root = fixture()?;
        let execution = Arc::new(ModuleExecution::new());
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let first_builder = gated_builder(
            root.path(),
            Arc::clone(&execution),
            Arc::clone(&entered),
            Arc::clone(&release),
            outcome == "failure",
        );
        let first = tokio::spawn(async move { first_builder.start(sources()).await });
        tokio::time::timeout(Duration::from_secs(2), entered.notified()).await?;
        // 文件实现已由共享执行中的首个 owner 绑定；等待方无需重复登记初始化回调。
        let mut second_builder = ProviderBuilder::new();
        second_builder.module_manifest(
            ModuleManifest::discover_from_config(root.path(), &ModulesConfig::default())
                .compiled_provider("Selected", |_| Ok(Selected(Trace::default())))
                .execution(Arc::clone(&execution)),
        );
        let mut second = tokio::spawn(async move { second_builder.start(sources()).await });
        assert!(
            tokio::time::timeout(Duration::from_millis(25), &mut second)
                .await
                .is_err(),
            "等待方越过了未完成初始化"
        );
        if outcome == "cancelled" {
            first.abort();
            assert!(
                first
                    .await
                    .err()
                    .ok_or("abort必须返回JoinError")?
                    .is_cancelled()
            );
        } else {
            release.notify_one();
            let result = first.await?;
            if outcome == "success" {
                shutdown(result?).await?;
            } else {
                assert_eq!(
                    result.err().ok_or("首个初始化必须失败")?.code(),
                    "fixture_file_failure"
                );
            }
        }
        let result = tokio::time::timeout(Duration::from_secs(2), second).await??;
        match outcome {
            "success" => shutdown(result?).await?,
            "failure" => assert_eq!(
                result.err().ok_or("等待方必须接收失败")?.code(),
                "module_file_initialization_failed"
            ),
            _ => assert_eq!(
                result.err().ok_or("等待方必须接收取消")?.code(),
                "module_file_initialization_cancelled"
            ),
        }
    }
    Ok(())
}

#[tokio::test]
async fn shared_manifest_empty_cache_does_not_become_hot_activation_but_empty_catalog_can_rescan()
-> TestResult {
    let root = tempfile::tempdir()?;
    let execution = Arc::new(ModuleExecution::new());
    let trace = Trace::default();
    let mut first = ProviderBuilder::new();
    first.module_manifest(
        manifest(root.path(), &trace, Arc::default()).execution(Arc::clone(&execution)),
    );
    let runtime = first.start(sources()).await?;
    assert!(
        runtime
            .container()
            .resolve::<ModuleRepository>()
            .await?
            .all()
            .is_empty()
    );
    shutdown(runtime).await?;
    write_module(
        root.path(),
        "modules/Blog",
        &json!({"name":"Blog","providers":["Selected"]}),
    )?;
    std::fs::write(
        root.path().join("modules_statuses.json"),
        r#"{"Blog":true}"#,
    )?;
    let mut second = ProviderBuilder::new();
    second.module_manifest(manifest(root.path(), &trace, Arc::default()).execution(execution));
    let runtime = second.start(sources()).await?;
    let repository = runtime.container().resolve::<ModuleRepository>().await?;
    assert_eq!(repository.all().len(), 1);
    assert!(!repository.find_or_fail("Blog")?.is_loaded());
    assert!(repository.find_or_fail("Blog")?.is_enabled()?);
    assert!(trace.lock().unwrap().is_empty());
    shutdown(runtime).await?;
    Ok(())
}

#[tokio::test]
async fn class_aliases_fail_explicitly_and_uncompiled_file_fails_after_provider_register()
-> TestResult {
    for aliases in [true, false] {
        let root = fixture()?;
        write_module(
            root.path(),
            "modules/Blog",
            &if aliases {
                json!({"name":"Blog","aliases":{"Alias":"Type"},"providers":["Selected"]})
            } else {
                json!({"name":"Blog","providers":["Selected"],"files":["init.rs"]})
            },
        )?;
        let trace = Trace::default();
        let selected = Arc::clone(&trace);
        let mut builder = ProviderBuilder::new();
        builder.module_manifest(
            ModuleManifest::discover_from_config(root.path(), &ModulesConfig::default())
                .compiled_provider("Selected", move |_| Ok(Selected(Arc::clone(&selected)))),
        );
        let error = builder
            .start(sources())
            .await
            .err()
            .ok_or("未实现或未编译声明不能静默忽略")?;
        assert_eq!(
            error.code(),
            if aliases {
                "module_class_alias_unsupported"
            } else {
                "module_file_not_compiled"
            }
        );
        if aliases {
            assert!(trace.lock().unwrap().is_empty());
        } else {
            assert_eq!(*trace.lock().unwrap(), ["provider.register"]);
        }
    }
    Ok(())
}

struct RecursiveFile {
    execution: std::sync::Weak<ModuleExecution>,
    root: PathBuf,
}
impl ModuleFile for RecursiveFile {
    fn initialize<'a>(&'a self, _: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let execution = self
                .execution
                .upgrade()
                .ok_or_else(|| Error::custom("fixture_execution_gone", "测试执行会话已释放"))?;
            let mut nested = ProviderBuilder::new();
            nested.module_manifest(
                ModuleManifest::discover_from_config(&self.root, &ModulesConfig::default())
                    .compiled_provider("Selected", |_| Ok(Selected(Trace::default())))
                    .compiled_module_file(
                        "Blog",
                        "init.rs",
                        RecursiveFile {
                            execution: Arc::downgrade(&execution),
                            root: self.root.clone(),
                        },
                    )
                    .execution(execution),
            );
            let runtime = nested.start(sources()).await?;
            shutdown(runtime).await
        })
    }
}
#[tokio::test]
async fn same_execution_recursive_include_returns_cycle_instead_of_waiting_for_itself() -> TestResult
{
    let root = fixture()?;
    let execution = Arc::new(ModuleExecution::new());
    let mut builder = ProviderBuilder::new();
    builder.module_manifest(
        ModuleManifest::discover_from_config(root.path(), &ModulesConfig::default())
            .compiled_provider("Selected", |_| Ok(Selected(Trace::default())))
            .compiled_module_file(
                "Blog",
                "init.rs",
                RecursiveFile {
                    execution: Arc::downgrade(&execution),
                    root: root.path().to_path_buf(),
                },
            )
            .execution(execution),
    );
    let error = tokio::time::timeout(Duration::from_secs(2), builder.start(sources()))
        .await?
        .err()
        .ok_or("递归初始化必须拒绝")?;
    assert_eq!(error.code(), "module_file_initialization_cycle");
    Ok(())
}

#[tokio::test]
async fn shared_nonempty_manifest_keeps_loaded_snapshot_and_new_execution_revalidates_json()
-> TestResult {
    let root = fixture()?;
    let execution = Arc::new(ModuleExecution::new());
    let trace = Trace::default();
    let mut first = ProviderBuilder::new();
    first.module_manifest(
        manifest(root.path(), &trace, Arc::default()).execution(Arc::clone(&execution)),
    );
    shutdown(first.start(sources()).await?).await?;
    std::fs::write(
        root.path().join("modules/Blog/module.json"),
        "{broken after first execution snapshot",
    )?;
    std::fs::write(
        root.path().join("modules_statuses.json"),
        r#"{"Blog":false}"#,
    )?;
    let mut second = ProviderBuilder::new();
    second.module_manifest(manifest(root.path(), &trace, Arc::default()).execution(execution));
    let runtime = second.start(sources()).await?;
    let repository = runtime.container().resolve::<ModuleRepository>().await?;
    let module = repository.find_or_fail("Blog")?;
    assert!(module.is_loaded());
    assert!(!module.is_enabled()?);
    shutdown(runtime).await?;
    let mut fresh = ProviderBuilder::new();
    fresh.module_manifest(manifest(root.path(), &trace, Arc::default()));
    let error = fresh
        .start(sources())
        .await
        .err()
        .ok_or("新的执行必须重新校验磁盘JSON")?;
    assert_eq!(error.code(), "module_manifest_invalid");
    Ok(())
}

#[tokio::test]
async fn file_directory_conversion_is_deferred_until_selected_files_are_used() -> TestResult {
    for (enabled, files, expected_error) in [
        (false, json!(["init.rs"]), None),
        (true, json!([]), None),
        (
            true,
            json!(["init.rs"]),
            Some("module_file_directory_invalid"),
        ),
    ] {
        let root = fixture()?;
        write_module(
            root.path(),
            "modules/Blog",
            &json!({"name":"Blog","module_directory":{"not":"a scalar"},"providers":["Selected"],"files":files}),
        )?;
        std::fs::write(
            root.path().join("modules_statuses.json"),
            serde_json::to_vec(&json!({"Blog":enabled}))?,
        )?;
        let trace = Trace::default();
        let mut builder = ProviderBuilder::new();
        builder.module_manifest(manifest(root.path(), &trace, Arc::default()));
        let result = builder.start(sources()).await;
        if let Some(code) = expected_error {
            assert_eq!(result.err().ok_or("仅实际包含才拒绝数组目录")?.code(), code);
            assert_eq!(*trace.lock().unwrap(), ["provider.register"]);
        } else {
            let runtime = result?;
            assert_eq!(
                runtime
                    .container()
                    .resolve::<ModuleRepository>()
                    .await?
                    .find_or_fail("Blog")?
                    .path(),
                root.path().join("modules/Blog")
            );
            shutdown(runtime).await?;
        }
    }
    Ok(())
}

#[tokio::test]
async fn file_entries_use_php_scalar_casts_then_canonical_once_identity() -> TestResult {
    let root = fixture()?;
    write_module(
        root.path(),
        "modules/Blog",
        &json!({"name":"Blog","providers":["Selected"],"files":[true,1,1.0,"1"]}),
    )?;
    std::fs::write(
        root.path().join("modules/Blog/1"),
        "// 已编译文件的真实路径凭证\n",
    )?;
    let trace = Trace::default();
    let mut builder = ProviderBuilder::new();
    builder.module_manifest(
        manifest(root.path(), &trace, Arc::default()).compiled_module_file(
            "Blog",
            "1",
            Included {
                trace: Arc::clone(&trace),
                fail: false,
            },
        ),
    );
    let runtime = builder.start(sources()).await?;
    assert_eq!(
        *trace.lock().unwrap(),
        ["provider.register", "file.initialize", "provider.boot"]
    );
    shutdown(runtime).await?;
    Ok(())
}

#[tokio::test]
async fn factories_observe_repository_last_name_winner_not_manifest_file_override() -> TestResult {
    let root = fixture()?;
    write_module(
        root.path(),
        "modules/ZLast",
        &json!({"name":"blog","module_directory":"unrelated/include/only","providers":[],"files":[]}),
    )?;
    let expected = root.path().join("modules/ZLast");
    let seen = Arc::new(Mutex::new(Vec::<PathBuf>::new()));
    let capture = Arc::clone(&seen);
    let mut builder = ProviderBuilder::new();
    builder.module_manifest(
        ModuleManifest::discover_from_config(root.path(), &ModulesConfig::default())
            .compiled_provider("Selected", move |context| {
                capture
                    .lock()
                    .unwrap()
                    .push(context.directory().to_path_buf());
                Ok(Selected(Trace::default()))
            })
            .compiled_module_file(
                "Blog",
                "init.rs",
                Included {
                    trace: Trace::default(),
                    fail: false,
                },
            ),
    );
    let runtime = builder.start(sources()).await?;
    assert_eq!(*seen.lock().unwrap(), [expected.clone(), expected.clone()]);
    assert_eq!(
        runtime
            .container()
            .resolve::<ModuleRepository>()
            .await?
            .find_or_fail("Blog")?
            .path(),
        expected
    );
    shutdown(runtime).await?;
    Ok(())
}
