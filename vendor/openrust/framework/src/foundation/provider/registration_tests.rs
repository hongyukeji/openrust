//! 可区分即时嵌套、统一 boot、force 失败和取消的注册回归。

use std::sync::{Arc, Mutex};

use figment::Figment;
use tokio::time::{Duration, Instant};

use crate::core::Error;
use crate::core::Result;
use crate::foundation::Application;
use crate::foundation::BootContext;
use crate::foundation::ProviderBuilder;
use crate::foundation::ProviderId;
use crate::foundation::RegisterContext;
use crate::support::{BoxFuture, ServiceProvider};

#[derive(Clone, Default)]
struct Trace(Arc<Mutex<Vec<String>>>);

impl Trace {
    fn push(&self, message: impl Into<String>) {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(message.into());
    }

    fn values(&self) -> Vec<String> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

struct Child {
    trace: Trace,
    value: u8,
    fail_register: bool,
    fail_boot: bool,
}

impl Child {
    fn new(trace: &Trace, value: u8) -> Self {
        Self {
            trace: trace.clone(),
            value,
            fail_register: false,
            fail_boot: false,
        }
    }
}

impl ServiceProvider for Child {
    fn register<'a>(&'a self, context: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            self.trace.push(format!("child.register.{}", self.value));
            context.instance(Arc::new(self.value)).await?;
            context.require::<u8>();
            context
                .config_repository()
                .set("app.name", serde_json::json!("child-name"))?;
            if self.fail_register {
                return Err(Error::custom("child_register_failed", "测试子注册失败"));
            }
            Ok(())
        })
    }

    fn boot<'a>(&'a self, context: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let value = context.resolve::<u8>().await?;
            self.trace
                .push(format!("child.boot.{}.{value}", self.value));
            if self.fail_boot {
                return Err(Error::custom("child_boot_failed", "测试子启动失败"));
            }
            Ok(())
        })
    }
}

struct Parent {
    trace: Trace,
    force: bool,
    fail: bool,
}

impl ServiceProvider for Parent {
    fn nested_providers(&self) -> Vec<ProviderId> {
        vec![ProviderId::of::<Child>()]
    }

    fn register<'a>(&'a self, context: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            self.trace.push("parent.before");
            let id = context
                .register_provider(Child::new(&self.trace, 1))
                .await?;
            assert_eq!(id, ProviderId::of::<Child>());
            context
                .register_provider(Child::new(&self.trace, 9))
                .await?;
            if self.force {
                context
                    .force_register_provider(Child::new(&self.trace, 2))
                    .await?;
                let mut rejected = Child::new(&self.trace, 3);
                rejected.fail_register = true;
                let error = context
                    .force_register_provider(rejected)
                    .await
                    .err()
                    .ok_or_else(|| Error::custom("expected_child_failure", "子注册必须失败"))?;
                assert_eq!(error.code(), "child_register_failed");
            }
            assert_eq!(
                context
                    .config_repository()
                    .section::<crate::config::AppConfig>("app")?
                    .name,
                "child-name"
            );
            self.trace.push("parent.after");
            if self.fail {
                return Err(Error::custom("parent_register_failed", "测试父注册失败"));
            }
            Ok(())
        })
    }

    fn boot<'a>(&'a self, _: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            self.trace.push("parent.boot");
            Ok(())
        })
    }
}

async fn empty_app() -> Result<Application> {
    Application::from_provider_runtime(ProviderBuilder::new().start(Figment::new()).await?)
}

async fn close(app: &Application) -> Result<()> {
    app.shutdown(Instant::now() + Duration::from_secs(2)).await
}

#[tokio::test]
async fn startup_nested_register_is_immediate_and_force_keeps_first_boot_slot() -> Result<()> {
    let trace = Trace::default();
    let mut builder = ProviderBuilder::new();
    builder.provider(Parent {
        trace: trace.clone(),
        force: true,
        fail: false,
    });
    let runtime = builder.start(Figment::new()).await?;
    assert_eq!(
        trace.values(),
        [
            "parent.before",
            "child.register.1",
            "child.register.2",
            "child.register.3",
            "parent.after",
            "child.boot.2.3",
            "parent.boot"
        ]
    );
    assert_eq!(*runtime.container().resolve::<u8>().await?, 3);
    runtime
        .shutdown(Instant::now() + Duration::from_secs(2))
        .await
}

#[tokio::test]
async fn live_nested_child_boot_finishes_before_parent_register_continues() -> Result<()> {
    let trace = Trace::default();
    let mut app = empty_app().await?;
    let outcome = app
        .register_provider(Parent {
            trace: trace.clone(),
            force: false,
            fail: false,
        })
        .await;
    let shutdown = close(&app).await;
    outcome?;
    assert_eq!(
        trace.values(),
        [
            "parent.before",
            "child.register.1",
            "child.boot.1.1",
            "parent.after",
            "parent.boot"
        ]
    );
    shutdown
}

#[tokio::test]
async fn failed_live_parent_retains_successful_child_without_registering_it_twice() -> Result<()> {
    let trace = Trace::default();
    let mut app = empty_app().await?;
    let outcome: Result<()> = async {
        let error = app
            .register_provider(Parent {
                trace: trace.clone(),
                force: false,
                fail: true,
            })
            .await
            .err()
            .ok_or_else(|| Error::custom("expected_parent_failure", "父注册必须失败"))?;
        assert_eq!(error.code(), "parent_register_failed");
        app.register_provider(Child::new(&trace, 8)).await?;
        app.register_provider(Parent {
            trace: trace.clone(),
            force: false,
            fail: false,
        })
        .await?;
        Ok(())
    }
    .await;
    let shutdown = close(&app).await;
    outcome?;
    assert_eq!(
        trace.values(),
        [
            "parent.before",
            "child.register.1",
            "child.boot.1.1",
            "parent.after",
            "parent.before",
            "parent.after",
            "parent.boot"
        ]
    );
    shutdown
}

struct Recursive {
    recurse: bool,
    force_child: bool,
    trace: Trace,
}

impl ServiceProvider for Recursive {
    fn register<'a>(&'a self, context: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            self.trace.push("recursive.register");
            if self.recurse {
                context
                    .boxed_register_provider(
                        Box::new(Self {
                            recurse: false,
                            force_child: false,
                            trace: self.trace.clone(),
                        }),
                        self.force_child,
                    )
                    .await?;
            }
            Ok(())
        })
    }
}

#[tokio::test]
async fn recursive_registration_rejects_cycles_but_can_return_an_existing_non_forced_instance()
-> Result<()> {
    let mut app = empty_app().await?;
    let trace = Trace::default();
    let outcome: Result<()> = async {
        let error = app
            .register_provider(Recursive {
                recurse: true,
                force_child: false,
                trace: trace.clone(),
            })
            .await
            .err()
            .ok_or_else(|| Error::custom("expected_cycle", "递归注册必须失败"))?;
        assert_eq!(error.code(), "provider_registration_cycle");
        app.register_provider(Recursive {
            recurse: false,
            force_child: false,
            trace: trace.clone(),
        })
        .await?;
        app.force_register_provider(Recursive {
            recurse: true,
            force_child: false,
            trace: trace.clone(),
        })
        .await?;
        let error = app
            .force_register_provider(Recursive {
                recurse: true,
                force_child: true,
                trace: trace.clone(),
            })
            .await
            .err()
            .ok_or_else(|| Error::custom("expected_cycle", "强制递归注册必须失败"))?;
        assert_eq!(error.code(), "provider_registration_cycle");
        app.force_register_provider(Recursive {
            recurse: false,
            force_child: false,
            trace: trace.clone(),
        })
        .await?;
        Ok(())
    }
    .await;
    let shutdown = close(&app).await;
    outcome?;
    assert_eq!(trace.values().len(), 5);
    shutdown
}

struct PendingParent {
    pending: bool,
    started: Arc<tokio::sync::Notify>,
    trace: Trace,
}

impl ServiceProvider for PendingParent {
    fn register<'a>(&'a self, context: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            context
                .register_provider(Child::new(&self.trace, 1))
                .await?;
            if self.pending {
                self.started.notify_one();
                std::future::pending::<()>().await;
            }
            self.trace.push("pending-parent.complete");
            Ok(())
        })
    }
}

#[tokio::test]
async fn cancelling_live_parent_releases_registration_marker_and_preserves_child_registry()
-> Result<()> {
    let trace = Trace::default();
    let started = Arc::new(tokio::sync::Notify::new());
    let mut app = empty_app().await?;
    let outcome: Result<()> = async {
        {
            let mut operation = Box::pin(app.register_provider(PendingParent {
                pending: true,
                started: Arc::clone(&started),
                trace: trace.clone(),
            }));
            tokio::select! {
                () = started.notified() => {},
                result = &mut operation => {
                    result?;
                    return Err(Error::custom("expected_pending", "父注册应停在可取消阶段"));
                },
                () = tokio::time::sleep(Duration::from_secs(2)) => {
                    return Err(Error::custom("nested_registration_timeout", "未到达可取消的父注册阶段"));
                },
            }
        }
        app.register_provider(PendingParent {
            pending: false,
            started,
            trace: trace.clone(),
        })
        .await?;
        Ok(())
    }
    .await;
    let shutdown = close(&app).await;
    outcome?;
    assert_eq!(
        trace.values(),
        [
            "child.register.1",
            "child.boot.1.1",
            "pending-parent.complete"
        ]
    );
    shutdown
}

struct MaskedParent(Trace);

impl ServiceProvider for MaskedParent {
    fn nested_providers(&self) -> Vec<ProviderId> {
        vec![ProviderId::of::<Child>()]
    }

    fn register<'a>(&'a self, context: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            context.register_provider(Child::new(&self.0, 1)).await?;
            context
                .force_register_provider(Child::new(&self.0, 2))
                .await?;
            Ok(())
        })
    }
}

#[tokio::test]
async fn startup_removal_masks_nested_force_but_explicit_live_registration_keeps_original_semantics()
-> Result<()> {
    let trace = Trace::default();
    let mut builder = ProviderBuilder::new();
    builder.provider(MaskedParent(trace.clone()));
    builder.remove_provider(ProviderId::of::<Child>());
    let mut app = Application::from_provider_runtime(builder.start(Figment::new()).await?)?;
    let outcome: Result<()> = async {
        assert!(!app.state().container().contains::<u8>());
        assert_eq!(trace.values(), Vec::<String>::new());
        app.register_provider(Child::new(&trace, 4)).await?;
        Ok(())
    }
    .await;
    let shutdown = close(&app).await;
    outcome?;
    assert_eq!(trace.values(), ["child.register.4", "child.boot.4.4"]);
    shutdown
}

struct Managed;
struct ResourceChild {
    closed: Arc<std::sync::atomic::AtomicUsize>,
    booted: Arc<std::sync::atomic::AtomicUsize>,
}

impl ServiceProvider for ResourceChild {
    fn register<'a>(&'a self, context: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let closed = Arc::clone(&self.closed);
            context
                .singleton::<Managed, _>(move |resolver| {
                    let closed = Arc::clone(&closed);
                    Box::pin(async move {
                        resolver.resources()?.on_shutdown(move || {
                            Box::pin(async move {
                                closed.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                                Ok(())
                            })
                        })?;
                        Ok(Arc::new(Managed))
                    })
                })
                .await?;
            // 现有公开 rebinding 登记会立即解析已有绑定，真实创建受管资源。
            let rebound = context
                .rebinding::<Managed, _>(|_, _| Box::pin(async { Ok(()) }))
                .await?
                .ok_or_else(|| {
                    Error::custom("fixture_binding_missing", "已有资源绑定应可立即解析")
                })?;
            let resolved = context.resolve::<Managed>().await?;
            assert!(Arc::ptr_eq(&rebound, &resolved));
            Ok(())
        })
    }

    fn boot<'a>(&'a self, _: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            self.booted
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        })
    }
}

struct FailingResourceParent {
    closed: Arc<std::sync::atomic::AtomicUsize>,
    booted: Arc<std::sync::atomic::AtomicUsize>,
}

impl ServiceProvider for FailingResourceParent {
    fn register<'a>(&'a self, context: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            context
                .register_provider(ResourceChild {
                    closed: Arc::clone(&self.closed),
                    booted: Arc::clone(&self.booted),
                })
                .await?;
            Err(Error::custom("parent_register_failed", "测试父注册失败"))
        })
    }
}

#[tokio::test]
async fn failed_startup_parent_closes_registered_child_resource_without_booting() -> Result<()> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let closed = Arc::new(AtomicUsize::new(0));
    let booted = Arc::new(AtomicUsize::new(0));
    let mut builder = ProviderBuilder::new();
    builder.provider(FailingResourceParent {
        closed: Arc::clone(&closed),
        booted: Arc::clone(&booted),
    });
    let error = builder
        .start(Figment::new())
        .await
        .err()
        .ok_or_else(|| Error::custom("expected_parent_failure", "启动父注册必须失败"))?;
    assert_eq!(error.code(), "parent_register_failed");
    assert_eq!(booted.load(Ordering::SeqCst), 0);
    assert_eq!(closed.load(Ordering::SeqCst), 1);
    Ok(())
}

#[tokio::test]
async fn failed_live_parent_keeps_child_resource_until_normal_application_shutdown() -> Result<()> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let closed = Arc::new(AtomicUsize::new(0));
    let booted = Arc::new(AtomicUsize::new(0));
    let mut app = empty_app().await?;
    let outcome: Result<()> = async {
        let error = app
            .register_provider(FailingResourceParent {
                closed: Arc::clone(&closed),
                booted: Arc::clone(&booted),
            })
            .await
            .err()
            .ok_or_else(|| Error::custom("expected_parent_failure", "运行期父注册必须失败"))?;
        assert_eq!(error.code(), "parent_register_failed");
        assert_eq!(booted.load(Ordering::SeqCst), 1);
        assert_eq!(closed.load(Ordering::SeqCst), 0);
        app.register_provider(ResourceChild {
            closed: Arc::clone(&closed),
            booted: Arc::clone(&booted),
        })
        .await?;
        Ok(())
    }
    .await;
    let shutdown = close(&app).await;
    outcome?;
    shutdown?;
    assert_eq!(booted.load(Ordering::SeqCst), 1);
    assert_eq!(closed.load(Ordering::SeqCst), 1);
    Ok(())
}

struct ForceFailedBootParent(Trace);

impl ServiceProvider for ForceFailedBootParent {
    fn register<'a>(&'a self, context: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let mut child = Child::new(&self.0, 2);
            child.fail_boot = true;
            context.force_register_provider(child).await?;
            Err(Error::custom(
                "continued_after_boot_failure",
                "运行期子boot失败后不应继续",
            ))
        })
    }
}

#[tokio::test]
async fn live_nested_boot_failure_keeps_new_child_slot_and_stops_parent_continuation() -> Result<()>
{
    let trace = Trace::default();
    let mut app = empty_app().await?;
    let outcome: Result<()> = async {
        app.register_provider(Child::new(&trace, 1)).await?;
        let error = app
            .register_provider(ForceFailedBootParent(trace.clone()))
            .await
            .err()
            .ok_or_else(|| Error::custom("expected_child_failure", "运行期子boot必须失败"))?;
        assert_eq!(error.code(), "child_boot_failed");
        app.register_provider(Child::new(&trace, 9)).await?;
        assert_eq!(*app.state().resolve::<u8>().await?, 2);
        app.force_register_provider(Child::new(&trace, 4)).await?;
        Ok(())
    }
    .await;
    let shutdown = close(&app).await;
    outcome?;
    assert_eq!(
        trace.values(),
        [
            "child.register.1",
            "child.boot.1.1",
            "child.register.2",
            "child.boot.2.2",
            "child.register.4",
            "child.boot.4.4"
        ]
    );
    shutdown
}

struct ChildDependent(Trace);

impl ServiceProvider for ChildDependent {
    fn dependencies(&self) -> Vec<ProviderId> {
        vec![ProviderId::of::<Child>()]
    }

    fn register<'a>(&'a self, _: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            self.0.push("dependent.register");
            Ok(())
        })
    }

    fn boot<'a>(&'a self, _: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            self.0.push("dependent.boot");
            Ok(())
        })
    }
}

#[tokio::test]
async fn declared_child_satisfies_later_static_dependency_after_its_real_registration() -> Result<()>
{
    let trace = Trace::default();
    let mut builder = ProviderBuilder::new();
    builder.provider(Parent {
        trace: trace.clone(),
        force: false,
        fail: false,
    });
    builder.provider(ChildDependent(trace.clone()));
    let runtime = builder.start(Figment::new()).await?;
    let shutdown = runtime
        .shutdown(Instant::now() + Duration::from_secs(2))
        .await;
    assert_eq!(
        trace.values(),
        [
            "parent.before",
            "child.register.1",
            "parent.after",
            "dependent.register",
            "child.boot.1.1",
            "parent.boot",
            "dependent.boot"
        ]
    );
    shutdown
}

#[tokio::test]
async fn declared_child_keeps_early_order_and_missing_dependency_failures_before_callbacks()
-> Result<()> {
    for missing in [false, true] {
        let trace = Trace::default();
        let mut builder = ProviderBuilder::new();
        builder.provider(ChildDependent(trace.clone()));
        builder.provider(Parent {
            trace: trace.clone(),
            force: false,
            fail: false,
        });
        if missing {
            builder.remove_provider(ProviderId::of::<Child>());
        }
        let error =
            builder.start(Figment::new()).await.err().ok_or_else(|| {
                Error::custom("expected_dependency_failure", "子类型依赖预检应失败")
            })?;
        assert_eq!(
            error.code(),
            if missing {
                "provider_dependency_missing"
            } else {
                "provider_order_invalid"
            }
        );
        assert_eq!(trace.values(), Vec::<String>::new());
    }
    Ok(())
}

struct IncompleteDeclaration(Trace);

impl ServiceProvider for IncompleteDeclaration {
    fn nested_providers(&self) -> Vec<ProviderId> {
        vec![ProviderId::of::<Child>()]
    }

    fn register<'a>(&'a self, context: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            self.0.push("incomplete.register");
            context.instance(Arc::new(23_u32)).await
        })
    }
}

#[tokio::test]
async fn child_declaration_without_real_registration_cannot_create_a_phantom_dependency()
-> Result<()> {
    let trace = Trace::default();
    let mut builder = ProviderBuilder::new();
    builder.provider(IncompleteDeclaration(trace.clone()));
    builder.provider(ChildDependent(trace.clone()));
    let error = builder.start(Figment::new()).await.err().ok_or_else(|| {
        Error::custom("expected_declaration_failure", "子类型声明不能代替实际注册")
    })?;
    assert_eq!(error.code(), "provider_nested_not_registered");
    assert_eq!(trace.values(), ["incomplete.register"]);
    Ok(())
}

#[tokio::test]
async fn pending_registration_releases_registry_for_an_independent_provider() -> Result<()> {
    let trace = Trace::default();
    let started = Arc::new(tokio::sync::Notify::new());
    let runtime = ProviderBuilder::new().start(Figment::new()).await?;
    let mut pending_config = runtime.config().clone();
    let mut pending_required = Vec::new();
    let mut pending = super::register(
        &runtime.providers,
        Box::new(PendingParent {
            pending: true,
            started: Arc::clone(&started),
            trace: trace.clone(),
        }),
        false,
        runtime.container(),
        &mut pending_config,
        &mut pending_required,
        super::BootMode::Startup,
    );
    tokio::select! {
        () = started.notified() => {},
        result = &mut pending => {
            result?;
            return Err(Error::custom("expected_pending", "父注册应等待取消"));
        },
        () = tokio::time::sleep(Duration::from_secs(2)) => {
            return Err(Error::custom("nested_registration_timeout", "未到达可取消的父注册阶段"));
        },
    }
    let mut config = runtime.config().clone();
    let mut required = Vec::new();
    let selected = tokio::time::timeout(
        Duration::from_secs(2),
        super::register(
            &runtime.providers,
            Box::new(ChildDependent(trace.clone())),
            false,
            runtime.container(),
            &mut config,
            &mut required,
            super::BootMode::Startup,
        ),
    )
    .await
    .map_err(Error::internal)??;
    assert_eq!(selected, ProviderId::of::<ChildDependent>());
    assert_eq!(
        runtime
            .providers
            .boot_snapshot()
            .iter()
            .map(|entry| entry.id)
            .collect::<Vec<_>>(),
        [
            ProviderId::of::<Child>(),
            ProviderId::of::<ChildDependent>()
        ]
    );
    drop(pending);
    runtime
        .shutdown(Instant::now() + Duration::from_secs(2))
        .await
}
