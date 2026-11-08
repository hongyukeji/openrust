//! 公开 typed 装饰契约：发现内部测试无法暴露的 factory 保留、生命周期及异步通知缺陷。

use std::future::Future;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::Duration;

use figment::{Figment, providers::Serialized};
use openrust::config::FrameworkConfig;
use openrust::container::{
    ClassDescriptor, ConstructorParam, ContainerBuilder, ParameterValue, Resolver, Scope,
    ScopeFactory, ServiceKey,
};
use openrust::core::Error;
use openrust::core::Result;
use openrust::foundation::BootContext;
use openrust::foundation::ProviderBuilder;
use openrust::foundation::RegisterContext;
use openrust::foundation::resources::ResourceOwner;
use openrust::support::{BoxFuture, ServiceProvider};
use tokio::sync::{Barrier, Notify};
use tokio::time::Instant;

async fn bounded<F: Future>(future: F) -> Result<F::Output> {
    tokio::time::timeout(Duration::from_secs(2), future)
        .await
        .map_err(|_| Error::custom("contract_timeout", "容器契约等待超过期限"))
}

fn observed(values: &Mutex<Vec<usize>>) -> Vec<usize> {
    values
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

fn add(
    amount: usize,
) -> impl Fn(Arc<usize>, Resolver) -> BoxFuture<'static, Result<Arc<usize>>> + Send + Sync {
    move |value, _| Box::pin(async move { Ok(Arc::new(*value + amount)) })
}

trait ReportView: Send + Sync {
    fn sequence(&self) -> usize;
}

struct Report(usize);
impl ReportView for Report {
    fn sequence(&self) -> usize {
        self.0
    }
}

struct InterruptedReport(Arc<AtomicUsize>);
impl ReportView for InterruptedReport {
    fn sequence(&self) -> usize {
        0
    }
}
impl Drop for InterruptedReport {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
        std::panic::resume_unwind(Box::new("projection_drop_probe"));
    }
}

#[tokio::test]
async fn projected_binding_scope_close_contains_rejected_value_drop_panic() -> Result<()> {
    for scoped in [false, true] {
        check_projected_scope_close_drop(scoped).await?;
    }
    Ok(())
}

async fn check_projected_scope_close_drop(scoped: bool) -> Result<()> {
    use std::error::Error as _;

    let key = ServiceKey::named("report.interrupted");
    let closed_scope = Arc::new(OnceLock::<Weak<Scope>>::new());
    let drops = Arc::new(AtomicUsize::new(0));
    let mut builder = ContainerBuilder::with_class_descriptors(vec![
        ClassDescriptor::concrete::<InterruptedReport>("app::InterruptedReport", vec![], |_| {
            Err(Error::custom(
                "unexpected_constructor",
                "必须先选择显式工厂",
            ))
        })
        .with_projection(openrust::class_projection!(InterruptedReport => dyn ReportView)),
    ])?;
    let factory = {
        let closed_scope = Arc::clone(&closed_scope);
        let drops = Arc::clone(&drops);
        move |_| -> BoxFuture<'static, Result<Arc<InterruptedReport>>> {
            let closed_scope = Arc::clone(&closed_scope);
            let drops = Arc::clone(&drops);
            Box::pin(async move {
                let scope = closed_scope
                    .get()
                    .and_then(Weak::upgrade)
                    .ok_or_else(|| Error::custom("test_scope_missing", "未安装作用域"))?;
                scope.close();
                Ok(Arc::new(InterruptedReport(drops)))
            })
        }
    };
    if scoped {
        builder.scoped_key(key.clone(), factory);
    } else {
        builder.bind_key(key.clone(), factory);
    }
    let container = builder.build();
    let scope = Arc::new(container.scope()?);
    assert!(closed_scope.set(Arc::downgrade(&scope)).is_ok());
    let Err(error) = scope.resolve_key::<dyn ReportView>(key.clone()).await else {
        return Err(Error::custom(
            "test_result_delivered",
            "已关闭作用域交付了视图",
        ));
    };
    assert_eq!(error.code(), "resource_task_interrupted");
    let mut source = error.source();
    let mut scope_closed = false;
    while let Some(current) = source {
        scope_closed |= current
            .downcast_ref::<Error>()
            .is_some_and(|primary| primary.code() == "scope_closed");
        source = current.source();
    }
    assert!(scope_closed);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    assert!(!container.resolved_key(&key)?);
    Ok(())
}

#[tokio::test]
async fn detached_scope_cannot_deliver_a_projected_value_after_root_shutdown() -> Result<()> {
    let key = ServiceKey::named("report.detached");
    let release = Arc::new(Notify::new());
    let mut builder = ContainerBuilder::with_class_descriptors(vec![
        ClassDescriptor::concrete::<Report>("app::Report", vec![], |_| {
            Err(Error::custom(
                "unexpected_constructor",
                "必须先选择显式工厂",
            ))
        })
        .with_projection(openrust::class_projection!(Report => dyn ReportView)),
    ])?;
    builder.bind_key(key.clone(), {
        let release = Arc::clone(&release);
        move |_| {
            let release = Arc::clone(&release);
            Box::pin(async move {
                release.notified().await;
                Ok(Arc::new(Report(5)))
            })
        }
    });
    let container = builder.build();
    let scope = container.scope()?;
    let mut request = Box::pin(scope.resolve_key::<dyn ReportView>(key));
    assert!(futures_util::poll!(request.as_mut()).is_pending());
    container
        .shutdown_root(Instant::now() + Duration::from_secs(2))
        .await?;
    release.notify_one();
    assert!(matches!(request.await, Err(error) if error.code() == "container_closed"));
    Ok(())
}

#[tokio::test]
async fn named_scoped_factory_projects_after_preflight_and_reuses_its_concrete_cache() -> Result<()>
{
    let key = ServiceKey::named("report.source");
    let alias = ServiceKey::named("report.view");
    let calls = Arc::new(AtomicUsize::new(0));
    let mut builder = ContainerBuilder::with_class_descriptors(vec![
        ClassDescriptor::concrete::<Report>("app::Report", vec![], |_| {
            Err(Error::custom(
                "unexpected_constructor",
                "必须先选择显式工厂",
            ))
        })
        .with_projection(openrust::class_projection!(Report => dyn ReportView)),
    ])?;
    builder.alias(key.clone(), alias.clone())?;
    builder.extend_key::<Report, _>(&alias, |report, _| {
        Box::pin(async move { Ok(Arc::new(Report(report.0 + 10))) })
    })?;
    builder.scoped_key(key.clone(), {
        let calls = Arc::clone(&calls);
        move |_| {
            let sequence = calls.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move { Ok(Arc::new(Report(sequence))) })
        }
    });
    let container = builder.build();
    let first = container.scope()?;
    let second = container.scope()?;
    assert!(matches!(
        first.resolve_key::<String>(alias.clone()).await,
        Err(error) if error.code() == "service_type_mismatch"
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let first_view = first.resolve_key::<dyn ReportView>(alias.clone()).await?;
    let first_concrete: Arc<dyn ReportView> = first.resolve_key::<Report>(key).await?;
    assert!(Arc::ptr_eq(&first_view, &first_concrete));
    assert_eq!(first_view.sequence(), 10);
    let second_view = second.resolve_key::<dyn ReportView>(alias).await?;
    assert_ne!(first_view.sequence(), second_view.sequence());
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    first
        .shutdown(Instant::now() + Duration::from_secs(2))
        .await?;
    second
        .shutdown(Instant::now() + Duration::from_secs(2))
        .await
}

#[tokio::test]
async fn alias_cycles_fail_on_traversal_and_original_key_registration_can_repair_them() -> Result<()>
{
    let first = ServiceKey::named("ports.alpha");
    let second = ServiceKey::named("ports.beta");
    let fallback = ServiceKey::named("Ports.Alpha");
    let container = ContainerBuilder::new().build();
    assert!(matches!(
        container.alias(first.clone(), first.clone()),
        Err(error) if error.code() == "service_alias_self"
    ));
    container.alias(second.clone(), first.clone())?;
    container.alias(first.clone(), second.clone())?;
    assert!(container.contains_key(&first));
    assert!(container.is_alias(&second));
    assert!(!container.is_shared_key(&first));
    assert!(matches!(
        container.resolved_key(&second),
        Err(error) if error.code() == "service_alias_cycle"
    ));
    assert!(matches!(
        container.resolve_key::<u16>(first.clone()).await,
        Err(error) if error.code() == "service_alias_cycle"
    ));

    container
        .instance_key(first.clone(), Arc::new(443u16))
        .await?;
    assert!(!container.is_alias(&first));
    assert_eq!(container.get_alias(&second)?, first);
    assert_eq!(*container.resolve_key::<u16>(second).await?, 443);
    assert!(!container.contains_key(&fallback));
    Ok(())
}

trait Driver: Send + Sync {
    fn label(&self) -> String;
}
struct Original;
impl Driver for Original {
    fn label(&self) -> String {
        "original".into()
    }
}
struct Wrapper(Arc<dyn Driver>, usize);
impl Driver for Wrapper {
    fn label(&self) -> String {
        format!("{}:{}", self.0.label(), self.1)
    }
}

// 先 extend 后绑定、覆盖 factory 与 dyn wrapper 必须保留任意原工厂和有序链。
#[tokio::test]
async fn typed_trait_extenders_preserve_factory_override_dependencies_and_order() -> Result<()> {
    let mut builder = ContainerBuilder::new();
    builder.extend::<dyn Driver, _>(|old, resolver| {
        Box::pin(async move {
            Ok(Arc::new(Wrapper(old, *resolver.resolve::<usize>().await?)) as Arc<dyn Driver>)
        })
    })?;
    assert!(!builder.contains::<dyn Driver>());
    builder.bind::<dyn Driver, _>(|_| {
        Box::pin(async { Err(Error::custom("stale_factory", "必须覆盖")) })
    });
    let calls = Arc::new(AtomicUsize::new(0));
    builder.bind::<dyn Driver, _>({
        let calls = Arc::clone(&calls);
        move |_| {
            let calls = Arc::clone(&calls);
            Box::pin(async move {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(Arc::new(Original) as Arc<dyn Driver>)
            })
        }
    });
    builder.extend::<dyn Driver, _>(|old, _| {
        Box::pin(async move { Ok(Arc::new(Wrapper(old, 9)) as Arc<dyn Driver>) })
    })?;
    builder.instance(Arc::new(7usize));
    let container = builder.build();
    assert_eq!(container.len(), 2);
    assert_eq!(
        container.resolve::<dyn Driver>().await?.label(),
        "original:7:9"
    );
    assert_eq!(
        container.resolve::<dyn Driver>().await?.label(),
        "original:7:9"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    Ok(())
}

// 显式 instance 绕过旧持久链；后绑定丢弃被覆盖 instance 的即时链。
#[tokio::test]
async fn explicit_instances_skip_persistent_extenders_and_overrides_drop_pending_operations()
-> Result<()> {
    let mut builder = ContainerBuilder::new();
    builder.extend::<usize, _>(add(10))?;
    builder.instance(Arc::new(1usize));
    builder.extend::<usize, _>(add(100))?;
    assert!(builder.get_instance::<usize>().is_none());
    builder.instance(Arc::new(2usize));
    assert_eq!(builder.get_instance::<usize>().as_deref(), Some(&2));
    assert_eq!(*builder.build().resolve::<usize>().await?, 2);
    let mut builder = ContainerBuilder::new();
    builder.extend::<usize, _>(add(10))?;
    builder.instance(Arc::new(1usize));
    builder.extend::<usize, _>(add(100))?;
    builder.singleton::<usize, _>(|_| Box::pin(async { Ok(Arc::new(3)) }));
    assert_eq!(*builder.build().resolve::<usize>().await?, 13);
    let mut builder = ContainerBuilder::new();
    builder.extend::<usize, _>(add(10))?;
    builder
        .instance(Arc::new(1usize))
        .extend::<usize, _>(add(100))?;
    builder.forget_extenders::<usize>()?;
    assert_eq!(*builder.build().resolve::<usize>().await?, 101);
    let mut missing = ContainerBuilder::new();
    missing.extend::<usize, _>(add(1))?;
    let container = missing.build();
    assert!(container.is_empty());
    assert_eq!(
        container
            .resolve::<usize>()
            .await
            .err()
            .map(|error| error.code().to_owned())
            .as_deref(),
        Some("service_not_bound")
    );
    Ok(())
}

// 两次 instance 即时扩展须分别通知中间值，回调同型 resolve 不得死锁或跳到后续队列。
#[tokio::test]
async fn pending_instance_operations_publish_each_value_before_each_rebinding_callback()
-> Result<()> {
    let values = Arc::new(Mutex::new(Vec::new()));
    let mut builder = ContainerBuilder::new();
    builder.instance(Arc::new(1usize));
    builder.rebinding::<usize, _>({
        let values = Arc::clone(&values);
        move |resolver, value| {
            let values = Arc::clone(&values);
            Box::pin(async move {
                assert!(Arc::ptr_eq(&value, &resolver.resolve::<usize>().await?));
                values
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push(*value);
                Ok(())
            })
        }
    })?;
    builder
        .extend::<usize, _>(add(1))?
        .extend::<usize, _>(add(10))?;
    let container = builder.build();
    assert!(container.get_instance::<usize>().is_none());
    assert!(container.get_ready::<usize>().is_none());
    assert_eq!(*bounded(container.resolve::<usize>()).await??, 12);
    assert_eq!(observed(&values), vec![2, 12]);
    assert_eq!(container.get_instance::<usize>().as_deref(), Some(&12));
    assert_eq!(*container.resolve::<usize>().await?, 12);
    assert_eq!(observed(&values), vec![2, 12]);
    Ok(())
}

// 第二操作失败只重试它，不撤销或重复第一操作已经提交的缓存和通知。
#[tokio::test]
async fn pending_instance_failure_retries_only_uncommitted_operation() -> Result<()> {
    let callbacks = Arc::new(Mutex::new(Vec::new()));
    let attempts = Arc::new(AtomicUsize::new(0));
    let mut builder = ContainerBuilder::new();
    builder
        .instance(Arc::new(1usize))
        .extend::<usize, _>(add(1))?;
    builder.extend::<usize, _>({
        let attempts = Arc::clone(&attempts);
        move |old, _| {
            let attempts = Arc::clone(&attempts);
            Box::pin(async move {
                if attempts.fetch_add(1, Ordering::SeqCst) == 0 {
                    return Err(Error::custom("private_failure", "SECRET"));
                }
                Ok(Arc::new(*old + 10))
            })
        }
    })?;
    builder.rebinding::<usize, _>({
        let callbacks = Arc::clone(&callbacks);
        move |_, value| {
            let callbacks = Arc::clone(&callbacks);
            Box::pin(async move {
                callbacks
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push(*value);
                Ok(())
            })
        }
    })?;
    let container = builder.build();
    let error = container.resolve::<usize>().await.err();
    assert_eq!(
        error.as_ref().map(Error::code),
        Some("service_initialization_failed")
    );
    assert!(
        !error
            .map(|error| error.to_string())
            .unwrap_or_default()
            .contains("SECRET")
    );
    assert!(container.get_ready::<usize>().is_none());
    assert_eq!(observed(&callbacks), vec![2]);
    assert_eq!(*container.resolve::<usize>().await?, 12);
    assert_eq!(observed(&callbacks), vec![2, 12]);
    assert_eq!(attempts.load(Ordering::SeqCst), 2);
    Ok(())
}

struct RegisterInstance(Arc<AtomicBool>);
impl ServiceProvider for RegisterInstance {
    fn register<'a>(
        &'a self,
        app: &'a mut RegisterContext<'_>,
    ) -> openrust::support::BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.instance(Arc::new(1usize)).await?;
            app.extend::<usize, _>(|old, resolver| {
                Box::pin(async move {
                    Ok(Arc::new(
                        *old + usize::from(*resolver.resolve::<bool>().await?),
                    ))
                })
            })
            .await?;
            Ok(())
        })
    }
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            assert_eq!(*app.resolve::<usize>().await?, 2);
            self.0.store(true, Ordering::SeqCst);
            app.extend::<usize, _>(add(1)).await?;
            assert_eq!(*app.resolve::<usize>().await?, 3);
            Ok(())
        })
    }
}
struct RegisterLater;
impl ServiceProvider for RegisterLater {
    fn register<'a>(
        &'a self,
        app: &'a mut RegisterContext<'_>,
    ) -> openrust::support::BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.instance(Arc::new(true)).await?;
            Ok(())
        })
    }
}

struct DescriptorService(Arc<usize>, Arc<u8>);

fn descriptor_service() -> ClassDescriptor {
    ClassDescriptor::concrete::<DescriptorService>(
        "contract::DescriptorService",
        vec![
            ConstructorParam::class::<usize>("value"),
            ConstructorParam::scalar::<u8>("limit").with_default(|| Ok(ParameterValue::new(3u8))),
        ],
        |args| {
            Ok(Arc::new(DescriptorService(
                args.service::<usize>(0)?,
                args.value(1)?.downcast::<u8>()?,
            )))
        },
    )
}

struct ReadDescriptorDuringRegister(Arc<AtomicUsize>);

impl ServiceProvider for ReadDescriptorDuringRegister {
    fn register<'a>(&'a self, app: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            self.0.fetch_add(1, Ordering::SeqCst);
            assert!(!app.is_bound::<DescriptorService>());
            let first = app.resolve::<DescriptorService>().await?;
            let second = app.resolve::<DescriptorService>().await?;
            assert!(!Arc::ptr_eq(&first, &second));
            assert_eq!((*first.0, *second.0), (9, 9));
            assert_eq!((*first.1, *second.1), (3, 3));
            assert!(!app.is_bound::<usize>());
            assert!(!app.is_bound::<DescriptorService>());
            app.instance(Arc::new(DescriptorService(Arc::new(19), Arc::new(8))))
                .await
        })
    }
}

#[tokio::test]
async fn provider_can_resolve_declared_concrete_during_register_and_override_it() -> Result<()> {
    let registrations = Arc::new(AtomicUsize::new(0));
    let mut builder = ProviderBuilder::new();
    builder
        .provider(ReadDescriptorDuringRegister(Arc::clone(&registrations)))
        .class_descriptors([ClassDescriptor::concrete::<usize>(
            "contract::Value",
            vec![],
            |_| Ok(Arc::new(9)),
        )])
        .class_descriptors([descriptor_service()]);
    let runtime = builder
        .start(Figment::from(Serialized::defaults(
            FrameworkConfig::default(),
        )))
        .await?;
    assert_eq!(registrations.load(Ordering::SeqCst), 1);
    assert!(runtime.container().contains::<DescriptorService>());
    assert_eq!(
        *runtime.container().resolve::<DescriptorService>().await?.0,
        19
    );
    runtime
        .shutdown(Instant::now() + Duration::from_secs(2))
        .await
}

#[tokio::test]
async fn duplicate_descriptors_fail_before_provider_register_or_constructor() -> Result<()> {
    let registrations = Arc::new(AtomicUsize::new(0));
    let mut builder = ProviderBuilder::new();
    builder
        .class_descriptors([descriptor_service()])
        .provider(ReadDescriptorDuringRegister(Arc::clone(&registrations)))
        .class_descriptors([ClassDescriptor::concrete::<DescriptorService>(
            "contract::DuplicateService",
            vec![],
            |_| {
                Err(Error::custom(
                    "unexpected_constructor",
                    "重复清单不得执行构造器",
                ))
            },
        )]);
    let result = builder
        .start(Figment::from(Serialized::defaults(
            FrameworkConfig::default(),
        )))
        .await;
    assert!(matches!(result, Err(error) if error.code() == "container_class_identity_duplicate"));
    assert_eq!(registrations.load(Ordering::SeqCst), 0);
    Ok(())
}

// Provider 的 live extend 会当场装饰已绑定 instance；依赖必须先登记，失败时不得 boot。
// ContainerBuilder 的延迟操作另由 pending_instance_operations_* 用例覆盖。
#[tokio::test]
async fn provider_live_extender_requires_pre_registered_dependency_before_boot() -> Result<()> {
    let booted = Arc::new(AtomicBool::new(false));
    let mut builder = ProviderBuilder::new();
    builder
        .provider(RegisterInstance(Arc::clone(&booted)))
        .provider(RegisterLater);
    let error = builder
        .start(Figment::from(Serialized::defaults(
            FrameworkConfig::default(),
        )))
        .await
        .err()
        .ok_or_else(|| Error::custom("unexpected_provider_ready", "缺失依赖时意外启动"))?;
    assert_eq!(error.code(), "service_not_bound");
    assert!(!booted.load(Ordering::SeqCst));

    let mut builder = ProviderBuilder::new();
    builder
        .provider(RegisterLater)
        .provider(RegisterInstance(Arc::clone(&booted)));
    let runtime = builder
        .start(Figment::from(Serialized::defaults(
            FrameworkConfig::default(),
        )))
        .await?;
    assert!(booted.load(Ordering::SeqCst));
    drop(runtime);
    Ok(())
}

// scoped 的即时变换只属于当前 scope，根 reset 及新 scope 不能继承该缓存变换。
#[tokio::test]
async fn scoped_immediate_extensions_are_isolated_and_root_reset_drops_current_view() -> Result<()>
{
    let mut builder = ContainerBuilder::new();
    builder.scoped::<usize, _>(|_| Box::pin(async { Ok(Arc::new(1)) }));
    let container = builder.build();
    let first = container.scope()?;
    let second = container.scope()?;
    assert_eq!(*first.resolve::<usize>().await?, 1);
    assert_eq!(*second.resolve::<usize>().await?, 1);
    first.extend::<usize, _>(add(10)).await?;
    assert_eq!(*first.resolve::<usize>().await?, 11);
    assert_eq!(*second.resolve::<usize>().await?, 1);
    assert_eq!(*container.resolve::<usize>().await?, 1);
    container.extend::<usize, _>(add(20)).await?;
    assert_eq!(*container.resolve::<usize>().await?, 21);
    assert_eq!(*first.resolve::<usize>().await?, 11);
    assert_eq!(*container.scope()?.resolve::<usize>().await?, 1);
    container
        .forget_scoped_instances(Instant::now() + Duration::from_secs(1))
        .await?;
    assert_eq!(*container.resolve::<usize>().await?, 1);
    first
        .shutdown(Instant::now() + Duration::from_secs(1))
        .await?;
    second
        .shutdown(Instant::now() + Duration::from_secs(1))
        .await?;
    Ok(())
}

// 未缓存的局部链只影响本 scope；全局新链只作用未来构造，不广播替换活跃兄弟缓存。
#[tokio::test]
async fn uncached_global_and_local_extenders_do_not_broadcast_existing_scoped_caches() -> Result<()>
{
    let mut builder = ContainerBuilder::new();
    builder.scoped::<usize, _>(|_| Box::pin(async { Ok(Arc::new(1)) }));
    let container = builder.build();
    let old = container.scope()?;
    assert_eq!(*old.resolve::<usize>().await?, 1);
    let local = container.scope()?;
    local.extend::<usize, _>(add(10)).await?;
    container.extend::<usize, _>(add(100)).await?;
    assert_eq!(*old.resolve::<usize>().await?, 1);
    assert_eq!(*local.resolve::<usize>().await?, 111);
    assert_eq!(*container.scope()?.resolve::<usize>().await?, 101);
    assert_eq!(*container.resolve::<usize>().await?, 101);
    Ok(())
}

// 并发 extend 应串行保留更新，期间同步读取与 resolve 不能观察旧缓存，原 Arc 仍有效。
#[tokio::test]
async fn concurrent_runtime_extensions_serialize_and_hide_unpublished_cache() -> Result<()> {
    let mut builder = ContainerBuilder::new();
    let factories = Arc::new(AtomicUsize::new(0));
    builder.singleton::<usize, _>({
        let factories = Arc::clone(&factories);
        move |_| {
            let factories = Arc::clone(&factories);
            Box::pin(async move {
                factories.fetch_add(1, Ordering::SeqCst);
                Ok(Arc::new(1))
            })
        }
    });
    let container = builder.build();
    let old = container.resolve::<usize>().await?;
    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let first = tokio::spawn({
        let container = container.clone();
        let entered = Arc::clone(&entered);
        let release = Arc::clone(&release);
        async move {
            container
                .extend::<usize, _>(move |old, _| {
                    let entered = Arc::clone(&entered);
                    let release = Arc::clone(&release);
                    Box::pin(async move {
                        entered.notify_one();
                        release.notified().await;
                        Ok(Arc::new(*old + 1))
                    })
                })
                .await
        }
    });
    bounded(entered.notified()).await?;
    assert!(container.get_ready::<usize>().is_none());
    let second = tokio::spawn({
        let container = container.clone();
        async move { container.extend::<usize, _>(add(10)).await }
    });
    let waiting = tokio::spawn({
        let container = container.clone();
        async move { container.resolve::<usize>().await }
    });
    tokio::task::yield_now().await;
    assert!(!waiting.is_finished());
    release.notify_one();
    bounded(first)
        .await?
        .map_err(|error| Error::custom("join_failed", error.to_string()))??;
    bounded(second)
        .await?
        .map_err(|error| Error::custom("join_failed", error.to_string()))??;
    let seen = bounded(waiting)
        .await?
        .map_err(|error| Error::custom("join_failed", error.to_string()))??;
    assert!(*seen == 2 || *seen == 12);
    assert_eq!(*container.resolve::<usize>().await?, 12);
    assert_eq!(*old, 1);
    assert_eq!(factories.load(Ordering::SeqCst), 1);
    Ok(())
}

// 即时扩展错误及 owner 取消都不能丢掉原缓存，也不能把失败装饰保留给未来解析。
#[tokio::test]
async fn runtime_extension_failure_and_cancellation_preserve_previous_cache() -> Result<()> {
    let mut builder = ContainerBuilder::new();
    builder.instance(Arc::new(1usize));
    let container = builder.build();
    let error = container
        .extend::<usize, _>(|_, _| Box::pin(async { Err(Error::custom("secret", "SECRET")) }))
        .await
        .err();
    assert_eq!(
        error.as_ref().map(Error::code),
        Some("service_initialization_failed")
    );
    assert_eq!(container.get_instance::<usize>().as_deref(), Some(&1));
    let entered = Arc::new(Notify::new());
    let owner = tokio::spawn({
        let container = container.clone();
        let entered = Arc::clone(&entered);
        async move {
            container
                .extend::<usize, _>(move |_, _| {
                    let entered = Arc::clone(&entered);
                    Box::pin(async move {
                        entered.notify_one();
                        std::future::pending().await
                    })
                })
                .await
        }
    });
    bounded(entered.notified()).await?;
    assert!(container.get_instance::<usize>().is_none());
    owner.abort();
    assert!(bounded(owner).await?.is_err());
    assert_eq!(*bounded(container.resolve::<usize>()).await??, 1);
    assert_eq!(container.get_instance::<usize>().as_deref(), Some(&1));
    container.extend::<usize, _>(add(1)).await?;
    assert_eq!(*container.resolve::<usize>().await?, 2);
    Ok(())
}

// 已解析 transient 的监听触发重新构造，未解析时不提前构造，forget 不撤销当前缓存。
#[tokio::test]
async fn transient_rebinding_is_lazy_and_forget_extenders_only_affects_future_construction()
-> Result<()> {
    let calls = Arc::new(AtomicUsize::new(0));
    let callbacks = Arc::new(Mutex::new(Vec::new()));
    let mut builder = ContainerBuilder::new();
    builder.bind::<usize, _>({
        let calls = Arc::clone(&calls);
        move |_| {
            let calls = Arc::clone(&calls);
            Box::pin(async move {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(Arc::new(1))
            })
        }
    });
    builder.rebinding::<usize, _>({
        let callbacks = Arc::clone(&callbacks);
        move |_, value| {
            let callbacks = Arc::clone(&callbacks);
            Box::pin(async move {
                callbacks
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push(*value);
                Ok(())
            })
        }
    })?;
    let container = builder.build();
    container.extend::<usize, _>(add(10)).await?;
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(observed(&callbacks), Vec::<usize>::new());
    assert_eq!(*container.resolve::<usize>().await?, 11);
    container.extend::<usize, _>(add(100)).await?;
    assert_eq!(observed(&callbacks), vec![111]);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    container.forget_extenders::<usize>().await?;
    assert_eq!(*container.resolve::<usize>().await?, 1);
    assert_eq!(observed(&callbacks), vec![111]);
    let mut builder = ContainerBuilder::new();
    builder
        .singleton::<usize, _>(|_| Box::pin(async { Ok(Arc::new(1)) }))
        .extend::<usize, _>(add(10))?;
    let singleton = builder.build();
    assert_eq!(*singleton.resolve::<usize>().await?, 11);
    singleton.forget_extenders::<usize>().await?;
    assert_eq!(*singleton.resolve::<usize>().await?, 11);
    Ok(())
}

// 回调失败发生在新缓存发布之后；回调递归 extend 必须报环而不是永久等待。
#[tokio::test]
async fn rebinding_failure_keeps_published_value_and_recursive_extension_has_cycle_boundary()
-> Result<()> {
    let mut builder = ContainerBuilder::new();
    builder.instance(Arc::new(1usize));
    builder.rebinding::<usize, _>(|resolver, value| {
        Box::pin(async move {
            assert_eq!(*resolver.resolve::<usize>().await?, *value);
            Err(Error::custom("callback_failed", "通知失败"))
        })
    })?;
    let container = builder.build();
    assert_eq!(
        bounded(container.extend::<usize, _>(add(1)))
            .await?
            .err()
            .as_ref()
            .map(Error::code),
        Some("callback_failed")
    );
    assert_eq!(*container.resolve::<usize>().await?, 2);
    let mut builder = ContainerBuilder::new();
    builder.instance(Arc::new(1usize));
    let target = Arc::new(Mutex::new(None));
    builder.rebinding::<usize, _>({
        let target = Arc::clone(&target);
        move |_, _| {
            let container: Option<openrust::container::Container> = target
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone();
            Box::pin(async move {
                let container = container
                    .ok_or_else(|| Error::custom("missing_container", "测试句柄不存在"))?;
                container.extend::<usize, _>(add(10)).await
            })
        }
    })?;
    let container = builder.build();
    *target
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(container.clone());
    assert_eq!(
        bounded(container.extend::<usize, _>(add(1)))
            .await?
            .err()
            .as_ref()
            .map(Error::code),
        Some("service_cycle")
    );
    assert_eq!(*container.resolve::<usize>().await?, 2);
    let _ = target
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take();
    Ok(())
}

// 新装饰路径必须沿用自依赖环和 singleton 捕获 scoped 的原安全边界。
#[tokio::test]
async fn extenders_preserve_self_cycle_and_captive_dependency_errors() -> Result<()> {
    let mut builder = ContainerBuilder::new();
    builder.singleton::<usize, _>(|_| Box::pin(async { Ok(Arc::new(1)) }));
    builder.extend::<usize, _>(|_, resolver| {
        Box::pin(async move { resolver.resolve::<usize>().await })
    })?;
    assert_eq!(
        bounded(builder.build().resolve::<usize>())
            .await?
            .err()
            .as_ref()
            .map(Error::code),
        Some("service_cycle")
    );
    let mut builder = ContainerBuilder::new();
    builder.instance(Arc::new(1usize));
    builder.scoped::<String, _>(|_| Box::pin(async { Ok(Arc::new(String::new())) }));
    let container = builder.build();
    let error = container
        .extend::<usize, _>(|old, resolver| {
            Box::pin(async move {
                resolver.resolve::<String>().await?;
                Ok(old)
            })
        })
        .await
        .err();
    assert_eq!(
        error.as_ref().map(Error::code),
        Some("captive_scope_dependency")
    );
    assert_eq!(*container.resolve::<usize>().await?, 1);
    Ok(())
}

// 两个已缓存服务同时装饰且互相解析时，新增等待边必须检出跨任务循环。
#[tokio::test]
async fn concurrent_cross_service_extensions_use_existing_wait_graph() -> Result<()> {
    let mut builder = ContainerBuilder::new();
    builder.instance(Arc::new(1u8)).instance(Arc::new(2u16));
    let container = builder.build();
    let barrier = Arc::new(Barrier::new(2));
    let first = container.extend::<u8, _>({
        let barrier = Arc::clone(&barrier);
        move |old, resolver| {
            let barrier = Arc::clone(&barrier);
            Box::pin(async move {
                barrier.wait().await;
                resolver.resolve::<u16>().await?;
                Ok(old)
            })
        }
    });
    let second = container.extend::<u16, _>(move |old, resolver| {
        let barrier = Arc::clone(&barrier);
        Box::pin(async move {
            barrier.wait().await;
            resolver.resolve::<u8>().await?;
            Ok(old)
        })
    });
    let (first, second) = bounded(async { tokio::join!(first, second) }).await?;
    assert!(
        first
            .as_ref()
            .err()
            .is_some_and(|error| error.code() == "service_cycle")
            || second
                .as_ref()
                .err()
                .is_some_and(|error| error.code() == "service_cycle")
    );
    assert_eq!(*container.resolve::<u8>().await?, 1);
    assert_eq!(*container.resolve::<u16>().await?, 2);
    Ok(())
}

// 运行期装饰等待正在初始化的原 factory，成功后只装饰一次已完成缓存。
#[tokio::test]
async fn runtime_extension_waits_inflight_factory_then_decorates_its_published_cache() -> Result<()>
{
    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let calls = Arc::new(AtomicUsize::new(0));
    let mut builder = ContainerBuilder::new();
    builder.singleton::<usize, _>({
        let entered = Arc::clone(&entered);
        let release = Arc::clone(&release);
        let calls = Arc::clone(&calls);
        move |_| {
            let entered = Arc::clone(&entered);
            let release = Arc::clone(&release);
            let calls = Arc::clone(&calls);
            Box::pin(async move {
                calls.fetch_add(1, Ordering::SeqCst);
                entered.notify_one();
                release.notified().await;
                Ok(Arc::new(1))
            })
        }
    });
    let container = builder.build();
    let resolving = tokio::spawn({
        let container = container.clone();
        async move { container.resolve::<usize>().await }
    });
    bounded(entered.notified()).await?;
    let extending = tokio::spawn({
        let container = container.clone();
        async move { container.extend::<usize, _>(add(10)).await }
    });
    tokio::task::yield_now().await;
    assert!(!extending.is_finished());
    release.notify_one();
    assert_eq!(
        *bounded(resolving)
            .await?
            .map_err(|error| Error::custom("join_failed", error.to_string()))??,
        1
    );
    bounded(extending)
        .await?
        .map_err(|error| Error::custom("join_failed", error.to_string()))??;
    assert_eq!(*container.resolve::<usize>().await?, 11);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    Ok(())
}

// 原初始化失败不能被误报为 extender 错误，无缓存时新装饰仍应登记供后续成功重试。
#[tokio::test]
async fn failed_inflight_factory_leaves_new_extender_registered_for_retry() -> Result<()> {
    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let calls = Arc::new(AtomicUsize::new(0));
    let mut builder = ContainerBuilder::new();
    builder.singleton::<usize, _>({
        let entered = Arc::clone(&entered);
        let release = Arc::clone(&release);
        let calls = Arc::clone(&calls);
        move |_| {
            let entered = Arc::clone(&entered);
            let release = Arc::clone(&release);
            let calls = Arc::clone(&calls);
            Box::pin(async move {
                if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                    entered.notify_one();
                    release.notified().await;
                    return Err(Error::custom("factory_failed", "原工厂失败"));
                }
                Ok(Arc::new(1))
            })
        }
    });
    let container = builder.build();
    let resolving = tokio::spawn({
        let container = container.clone();
        async move { container.resolve::<usize>().await }
    });
    bounded(entered.notified()).await?;
    let extending = tokio::spawn({
        let container = container.clone();
        async move { container.extend::<usize, _>(add(10)).await }
    });
    tokio::task::yield_now().await;
    release.notify_one();
    assert!(
        bounded(resolving)
            .await?
            .map_err(|error| Error::custom("join_failed", error.to_string()))?
            .is_err()
    );
    bounded(extending)
        .await?
        .map_err(|error| Error::custom("join_failed", error.to_string()))??;
    assert_eq!(*container.resolve::<usize>().await?, 11);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    Ok(())
}

// transient 原 factory 的并发能力不能被装饰协调误改为单例或串行互斥。
#[tokio::test]
async fn transient_factories_remain_concurrent_with_extender_coordination() -> Result<()> {
    let barrier = Arc::new(Barrier::new(2));
    let next = Arc::new(AtomicUsize::new(0));
    let mut builder = ContainerBuilder::new();
    builder.bind::<usize, _>(move |_| {
        let barrier = Arc::clone(&barrier);
        let next = Arc::clone(&next);
        Box::pin(async move {
            barrier.wait().await;
            Ok(Arc::new(next.fetch_add(1, Ordering::SeqCst)))
        })
    });
    builder.extend::<usize, _>(add(10))?;
    let container = builder.build();
    let (first, second) =
        bounded(async { tokio::join!(container.resolve::<usize>(), container.resolve::<usize>()) })
            .await?;
    let mut values = vec![*first?, *second?];
    values.sort_unstable();
    assert_eq!(values, vec![10, 11]);
    Ok(())
}

// factory 与 wrapper 新登记的清理均归原生命周期；失败 wrapper 的资源也必须在 owner 关闭回收。
#[tokio::test]
async fn extender_resources_keep_original_owner_including_failed_wrapper_registration() -> Result<()>
{
    let owner = ResourceOwner::new();
    let closed = Arc::new(AtomicUsize::new(0));
    let mut builder = ContainerBuilder::new();
    builder.context(
        openrust::config::repository::ConfigRepository::new(Figment::new()),
        owner.registrar(),
    );
    builder.singleton::<usize, _>({
        let closed = Arc::clone(&closed);
        move |resolver| {
            let closed = Arc::clone(&closed);
            Box::pin(async move {
                resolver.resources()?.on_shutdown(move || {
                    Box::pin(async move {
                        closed.fetch_add(1, Ordering::SeqCst);
                        Ok(())
                    })
                })?;
                Ok(Arc::new(1))
            })
        }
    });
    let container = builder.build();
    assert_eq!(*container.resolve::<usize>().await?, 1);
    let error = container
        .extend::<usize, _>({
            let closed = Arc::clone(&closed);
            move |_, resolver| {
                let closed = Arc::clone(&closed);
                Box::pin(async move {
                    resolver.resources()?.on_shutdown(move || {
                        Box::pin(async move {
                            closed.fetch_add(1, Ordering::SeqCst);
                            Ok(())
                        })
                    })?;
                    Err(Error::custom("wrapper_failed", "装饰失败"))
                })
            }
        })
        .await
        .err();
    assert_eq!(
        error.as_ref().map(Error::code),
        Some("service_initialization_failed")
    );
    assert_eq!(closed.load(Ordering::SeqCst), 0);
    assert_eq!(*container.resolve::<usize>().await?, 1);
    owner
        .shutdown(Instant::now() + Duration::from_secs(1))
        .await?;
    assert_eq!(closed.load(Ordering::SeqCst), 2);
    owner
        .shutdown(Instant::now() + Duration::from_secs(1))
        .await?;
    assert_eq!(closed.load(Ordering::SeqCst), 2);
    Ok(())
}

struct ReenterOnDrop(ScopeFactory, Arc<AtomicBool>);
impl Drop for ReenterOnDrop {
    fn drop(&mut self) {
        if let Ok(scope) = self.0.scope() {
            scope.close();
            self.1.store(true, Ordering::SeqCst);
        }
    }
}

// 替换最后一个缓存 Arc 时用户 Drop 可以重入容器，证明发布路径未在 std 锁内析构用户值。
#[tokio::test]
async fn replaced_cached_service_drop_can_reenter_container_without_locking_deadlock() -> Result<()>
{
    let dropped = Arc::new(AtomicBool::new(false));
    let mut builder = ContainerBuilder::new();
    builder.singleton::<ReenterOnDrop, _>({
        let dropped = Arc::clone(&dropped);
        move |resolver| {
            let dropped = Arc::clone(&dropped);
            Box::pin(async move { Ok(Arc::new(ReenterOnDrop(resolver.scope_factory(), dropped))) })
        }
    });
    let container = builder.build();
    drop(container.resolve::<ReenterOnDrop>().await?);
    bounded(container.extend::<ReenterOnDrop, _>(|old, resolver| {
        Box::pin(async move {
            let new = ReenterOnDrop(resolver.scope_factory(), Arc::new(AtomicBool::new(false)));
            drop(old);
            Ok(Arc::new(new))
        })
    }))
    .await??;
    assert!(dropped.load(Ordering::SeqCst));
    Ok(())
}

// root reset 或 scope close 必须封住旧代正在执行的装饰，不能迟到发布到新代或重新开放已关闭 scope。
#[tokio::test]
async fn scope_close_and_root_reset_prevent_late_extension_publication() -> Result<()> {
    let mut builder = ContainerBuilder::new();
    builder.scoped::<usize, _>(|_| Box::pin(async { Ok(Arc::new(1)) }));
    let container = builder.build();
    assert_eq!(*container.resolve::<usize>().await?, 1);
    let entered = Arc::new(Notify::new());
    let extending = tokio::spawn({
        let container = container.clone();
        let entered = Arc::clone(&entered);
        async move {
            container
                .extend::<usize, _>(move |_, _| {
                    let entered = Arc::clone(&entered);
                    Box::pin(async move {
                        entered.notify_one();
                        std::future::pending().await
                    })
                })
                .await
        }
    });
    bounded(entered.notified()).await?;
    container
        .forget_scoped_instances(Instant::now() + Duration::from_secs(1))
        .await?;
    assert_eq!(
        bounded(extending)
            .await?
            .map_err(|error| Error::custom("join_failed", error.to_string()))?
            .err()
            .as_ref()
            .map(Error::code),
        Some("scope_closed")
    );
    assert_eq!(*container.resolve::<usize>().await?, 1);
    let scope = Arc::new(container.scope()?);
    assert_eq!(*scope.resolve::<usize>().await?, 1);
    let entered = Arc::new(Notify::new());
    let extending = tokio::spawn({
        let scope = Arc::clone(&scope);
        let entered = Arc::clone(&entered);
        async move {
            scope
                .extend::<usize, _>(move |_, _| {
                    let entered = Arc::clone(&entered);
                    Box::pin(async move {
                        entered.notify_one();
                        std::future::pending().await
                    })
                })
                .await
        }
    });
    bounded(entered.notified()).await?;
    scope
        .shutdown(Instant::now() + Duration::from_secs(1))
        .await?;
    assert_eq!(
        bounded(extending)
            .await?
            .map_err(|error| Error::custom("join_failed", error.to_string()))?
            .err()
            .as_ref()
            .map(Error::code),
        Some("scope_closed")
    );
    assert_eq!(
        scope
            .resolve::<usize>()
            .await
            .err()
            .as_ref()
            .map(Error::code),
        Some("scope_closed")
    );
    Ok(())
}

// 惰性装饰链失败时不得缓存原 factory 的部分结果，下次必须重跑原 factory 与整条持久链。
#[tokio::test]
async fn lazy_extension_failure_restarts_factory_and_entire_chain_without_partial_cache()
-> Result<()> {
    let factories = Arc::new(AtomicUsize::new(0));
    let first = Arc::new(AtomicUsize::new(0));
    let second = Arc::new(AtomicUsize::new(0));
    let mut builder = ContainerBuilder::new();
    builder.singleton::<usize, _>({
        let factories = Arc::clone(&factories);
        move |_| {
            let factories = Arc::clone(&factories);
            Box::pin(async move {
                factories.fetch_add(1, Ordering::SeqCst);
                Ok(Arc::new(1))
            })
        }
    });
    builder.extend::<usize, _>({
        let first = Arc::clone(&first);
        move |old, _| {
            let first = Arc::clone(&first);
            Box::pin(async move {
                first.fetch_add(1, Ordering::SeqCst);
                Ok(Arc::new(*old + 1))
            })
        }
    })?;
    builder.extend::<usize, _>({
        let second = Arc::clone(&second);
        move |old, _| {
            let second = Arc::clone(&second);
            Box::pin(async move {
                if second.fetch_add(1, Ordering::SeqCst) == 0 {
                    return Err(Error::custom("lazy_failed", "后置装饰失败"));
                }
                Ok(Arc::new(*old + 10))
            })
        }
    })?;
    let container = builder.build();
    assert_eq!(
        container
            .resolve::<usize>()
            .await
            .err()
            .as_ref()
            .map(Error::code),
        Some("service_initialization_failed")
    );
    assert!(container.get_ready::<usize>().is_none());
    assert_eq!(*container.resolve::<usize>().await?, 12);
    assert_eq!(factories.load(Ordering::SeqCst), 2);
    assert_eq!(first.load(Ordering::SeqCst), 2);
    assert_eq!(second.load(Ordering::SeqCst), 2);
    Ok(())
}

// 单例惰性装饰 owner 被取消时，等待者必须有界退出，独立重试不能被遗留 attempt 阻塞。
#[tokio::test]
async fn cancelled_lazy_extension_wakes_waiters_and_allows_clean_retry() -> Result<()> {
    let entered = Arc::new(Notify::new());
    let calls = Arc::new(AtomicUsize::new(0));
    let mut builder = ContainerBuilder::new();
    builder.singleton::<usize, _>(|_| Box::pin(async { Ok(Arc::new(1)) }));
    builder.extend::<usize, _>({
        let entered = Arc::clone(&entered);
        let calls = Arc::clone(&calls);
        move |old, _| {
            let entered = Arc::clone(&entered);
            let calls = Arc::clone(&calls);
            Box::pin(async move {
                if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                    entered.notify_one();
                    std::future::pending::<()>().await;
                }
                Ok(Arc::new(*old + 1))
            })
        }
    })?;
    let container = builder.build();
    let owner = tokio::spawn({
        let container = container.clone();
        async move { container.resolve::<usize>().await }
    });
    bounded(entered.notified()).await?;
    let mut waiter = Box::pin(container.resolve::<usize>());
    assert!(futures_util::poll!(waiter.as_mut()).is_pending());
    owner.abort();
    assert!(bounded(owner).await?.is_err());
    assert_eq!(
        bounded(waiter).await?.err().as_ref().map(Error::code),
        Some("service_initialization_cancelled")
    );
    assert_eq!(*bounded(container.resolve::<usize>()).await??, 2);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    Ok(())
}

// scope 上装饰 singleton 仍使用应用资源 owner，关闭发起 scope 不应回收应用共享资源。
#[tokio::test]
async fn scope_singleton_extension_uses_application_owner_and_scoped_extension_uses_scope_owner()
-> Result<()> {
    let owner = ResourceOwner::new();
    let closed = Arc::new(AtomicUsize::new(0));
    let mut builder = ContainerBuilder::new();
    builder.context(
        openrust::config::repository::ConfigRepository::new(Figment::new()),
        owner.registrar(),
    );
    builder.instance(Arc::new(1usize));
    builder.scoped::<u8, _>(|_| Box::pin(async { Ok(Arc::new(1)) }));
    let container = builder.build();
    let scope = container.scope()?;
    scope
        .extend::<usize, _>({
            let closed = Arc::clone(&closed);
            move |old, resolver| {
                let closed = Arc::clone(&closed);
                Box::pin(async move {
                    resolver.resources()?.on_shutdown(move || {
                        Box::pin(async move {
                            closed.fetch_add(10, Ordering::SeqCst);
                            Ok(())
                        })
                    })?;
                    Ok(Arc::new(*old + 1))
                })
            }
        })
        .await?;
    assert_eq!(*scope.resolve::<u8>().await?, 1);
    scope
        .extend::<u8, _>({
            let closed = Arc::clone(&closed);
            move |old, resolver| {
                let closed = Arc::clone(&closed);
                Box::pin(async move {
                    resolver.resources()?.on_shutdown(move || {
                        Box::pin(async move {
                            closed.fetch_add(1, Ordering::SeqCst);
                            Ok(())
                        })
                    })?;
                    Ok(Arc::new(*old + 1))
                })
            }
        })
        .await?;
    scope
        .shutdown(Instant::now() + Duration::from_secs(1))
        .await?;
    assert_eq!(closed.load(Ordering::SeqCst), 1);
    assert_eq!(*container.resolve::<usize>().await?, 2);
    owner
        .shutdown(Instant::now() + Duration::from_secs(1))
        .await?;
    assert_eq!(closed.load(Ordering::SeqCst), 11);
    Ok(())
}

struct OrderedA(usize);
struct OrderedB(usize);
struct OrderedInstances(Arc<Mutex<Vec<String>>>, bool);

fn record_event(events: &Mutex<Vec<String>>, event: String) {
    events
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(event);
}

fn ordered_a(
    events: Arc<Mutex<Vec<String>>>,
) -> impl Fn(Arc<OrderedA>, Resolver) -> BoxFuture<'static, Result<Arc<OrderedA>>> + Send + Sync {
    move |old, _| {
        let events = Arc::clone(&events);
        Box::pin(async move {
            record_event(&events, format!("extend-a:{}", old.0));
            Ok(Arc::new(OrderedA(old.0 + 1)))
        })
    }
}

impl ServiceProvider for OrderedInstances {
    fn register<'a>(
        &'a self,
        app: &'a mut RegisterContext<'_>,
    ) -> openrust::support::BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.instance(Arc::new(OrderedA(1))).await?;
            app.instance(Arc::new(OrderedB(10))).await?;
            let _ = app
                .rebinding::<OrderedA, _>({
                    let events = Arc::clone(&self.0);
                    let resolve_dependency = self.1;
                    move |resolver, value| {
                        let events = Arc::clone(&events);
                        Box::pin(async move {
                            assert!(Arc::ptr_eq(&value, &resolver.resolve::<OrderedA>().await?));
                            if resolve_dependency && value.0 == 2 {
                                assert_eq!(resolver.resolve::<OrderedB>().await?.0, 10);
                            }
                            record_event(&events, format!("callback-a:{}", value.0));
                            Ok(())
                        })
                    }
                })
                .await?;
            let _ = app
                .rebinding::<OrderedB, _>({
                    let events = Arc::clone(&self.0);
                    move |resolver, value| {
                        let events = Arc::clone(&events);
                        Box::pin(async move {
                            assert!(Arc::ptr_eq(&value, &resolver.resolve::<OrderedB>().await?));
                            record_event(&events, format!("callback-b:{}", value.0));
                            Ok(())
                        })
                    }
                })
                .await?;
            app.extend::<OrderedA, _>(ordered_a(Arc::clone(&self.0)))
                .await?;
            app.extend::<OrderedB, _>({
                let events = Arc::clone(&self.0);
                move |old, _| {
                    let events = Arc::clone(&events);
                    Box::pin(async move {
                        record_event(&events, format!("extend-b:{}", old.0));
                        Ok(Arc::new(OrderedB(old.0 + 1)))
                    })
                }
            })
            .await?;
            app.extend::<OrderedA, _>(ordered_a(Arc::clone(&self.0)))
                .await?;
            Ok(())
        })
    }

    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            assert_eq!(app.resolve::<OrderedA>().await?.0, 3);
            assert_eq!(app.resolve::<OrderedB>().await?.0, 11);
            Ok(())
        })
    }
}

// A→B→A 的即时操作和每次发布后的通知必须按登记顺序执行，不能受 HashMap 随机种子影响。
#[tokio::test]
async fn provider_pending_instance_operations_preserve_cross_type_registration_order() -> Result<()>
{
    for _ in 0..16 {
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut builder = ProviderBuilder::new();
        builder.provider(OrderedInstances(Arc::clone(&events), false));
        let runtime = bounded(builder.start(Figment::from(Serialized::defaults(
            FrameworkConfig::default(),
        ))))
        .await??;
        let actual = events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        assert_eq!(
            actual,
            [
                "extend-a:1",
                "callback-a:2",
                "extend-b:10",
                "callback-b:11",
                "extend-a:2",
                "callback-a:3"
            ]
        );
        runtime
            .shutdown(Instant::now() + Duration::from_secs(1))
            .await?;
    }
    Ok(())
}

// live A 扩展当场通知并解析尚未扩展的 B，后续 B、A 扩展各自只执行一次。
#[tokio::test]
async fn provider_live_dependency_resolution_observes_current_order_without_duplicate_drain()
-> Result<()> {
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut builder = ProviderBuilder::new();
    builder.provider(OrderedInstances(Arc::clone(&events), true));
    let runtime = bounded(builder.start(Figment::from(Serialized::defaults(
        FrameworkConfig::default(),
    ))))
    .await??;
    let actual = events
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    assert_eq!(
        actual,
        [
            "extend-a:1",
            "callback-a:2",
            "extend-b:10",
            "callback-b:11",
            "extend-a:2",
            "callback-a:3"
        ]
    );
    runtime
        .shutdown(Instant::now() + Duration::from_secs(1))
        .await?;
    Ok(())
}
