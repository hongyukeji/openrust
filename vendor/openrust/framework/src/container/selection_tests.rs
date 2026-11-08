//! 私有选择票据读验直接使用生产State。这里没有第二个Resolver或缓存。
use super::class_descriptor::{ClassDescriptor, ClassKind};
use super::directory::{AliasWitness, RegistrationOutcome};
use super::*;
use std::sync::atomic::AtomicUsize;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SourceKind {
    Binding(u64),
    Instance(u64),
    Descriptor(ClassKind),
}

struct SelectedSource {
    cache: CacheKey,
    requested_scope: u64,
    kind: SourceKind,
    source_revision: u64,
    attempt: Option<u64>,
    aliases: AliasWitness,
}

impl State {
    fn selection_kind(&self, key: &ServiceKey) -> Option<(SourceKind, Lifetime)> {
        if let Some(binding) = self.bindings.get(key) {
            if binding.instance.is_some() {
                return Some((
                    SourceKind::Instance(binding.generation),
                    Lifetime::Singleton,
                ));
            }
            if binding.factory.is_some() {
                return Some((SourceKind::Binding(binding.generation), binding.lifetime));
            }
        }
        self.classes
            .class(key)
            .map(|class| (SourceKind::Descriptor(class.kind()), Lifetime::Bind))
    }

    fn selection_revision(&self, key: &ServiceKey, kind: SourceKind) -> u64 {
        self.class_source_revisions
            .get(key)
            .copied()
            .unwrap_or(match kind {
                SourceKind::Binding(generation) | SourceKind::Instance(generation) => generation,
                SourceKind::Descriptor(_) => 0,
            })
    }

    fn selected_source(&self, key: &ServiceKey, scope: u64) -> Result<Option<SelectedSource>> {
        let aliases = self.aliases.witness(&self.classes.canonical_key(key))?;
        let Some((kind, lifetime)) = self.selection_kind(&aliases.terminal) else {
            return Ok(None);
        };
        let cache = CacheKey {
            service: aliases.terminal.clone(),
            scope: if lifetime == Lifetime::Singleton {
                u64::MAX
            } else {
                scope
            },
        };
        let source_revision = self.selection_revision(&cache.service, kind);
        let attempt = self.slots.get(&cache).map(|attempt| attempt.id);
        Ok(Some(SelectedSource {
            cache,
            requested_scope: scope,
            kind,
            source_revision,
            attempt,
            aliases,
        }))
    }

    fn selection_current(&self, ticket: &SelectedSource, scope: u64) -> bool {
        !self.terminated
            && ticket.requested_scope == scope
            && self.aliases.accepts(&ticket.aliases)
            && self
                .selection_kind(&ticket.cache.service)
                .is_some_and(|(kind, lifetime)| {
                    kind == ticket.kind
                        && self.selection_revision(&ticket.cache.service, kind)
                            == ticket.source_revision
                        && ticket.cache.scope
                            == if lifetime == Lifetime::Singleton {
                                u64::MAX
                            } else {
                                scope
                            }
                })
            && ticket.attempt.is_none_or(|id| {
                self.slots
                    .get(&ticket.cache)
                    .is_some_and(|attempt| attempt.id == id)
            })
    }
}

fn symbol() -> ServiceKey {
    ServiceKey::named("App/Counter")
}
fn typed() -> ServiceKey {
    ServiceKey::typed::<usize>()
}
fn missing_ticket() -> Error {
    container_error("test_selection_missing", "缺少真实State选择票据")
}
fn builder(count: &Arc<AtomicUsize>) -> Result<ContainerBuilder> {
    let calls = Arc::clone(count);
    ContainerBuilder::with_class_descriptors(vec![
        ClassDescriptor::concrete::<usize>("App/Counter", vec![], move |_| {
            calls.fetch_add(1, Ordering::AcqRel);
            Ok(Arc::new(100))
        }),
        ClassDescriptor::concrete::<String>("App/Other", vec![], |_| Ok(Arc::new(String::new()))),
    ])
}
fn ticket(container: &Container, key: &ServiceKey) -> Result<SelectedSource> {
    let state = lock(&container.inner.state);
    state
        .selected_source(key, state.root_scope)?
        .ok_or_else(missing_ticket)
}
fn current(container: &Container, ticket: &SelectedSource) -> bool {
    let state = lock(&container.inner.state);
    state.selection_current(ticket, state.root_scope)
}

#[tokio::test]
async fn installed_registry_is_shared_and_descriptor_does_not_forge_binding_or_execute()
-> Result<()> {
    let count = Arc::new(AtomicUsize::new(0));
    let duplicate = ContainerBuilder::with_class_descriptors(vec![
        ClassDescriptor::abstract_class::<usize>("Duplicate"),
        ClassDescriptor::interface::<String>("Duplicate"),
    ]);
    assert!(
        matches!(duplicate, Err(error) if error.code() == "container_class_identity_duplicate")
    );
    let builder = builder(&count)?;
    let registry = Arc::clone(&builder.classes);
    assert!(!builder.bound_key(&symbol()));
    assert!(!builder.contains::<usize>());
    let container = builder.build();
    {
        let state = lock(&container.inner.state);
        assert!(Arc::ptr_eq(&registry, &state.classes));
        assert_eq!(state.classes.canonical_key(&symbol()), typed());
        assert_eq!(
            state
                .classes
                .class(&symbol())
                .map(class_descriptor::ClassRecord::kind),
            Some(ClassKind::Concrete)
        );
        assert!(
            state
                .classes
                .prepare(&symbol(), &typed())?
                .parameters()
                .is_empty()
        );
        assert!(!state.aliases.contains(&typed()));
        assert!(!state.resolved_key(&symbol())?);
        assert!(state.slots.is_empty());
    }
    assert!(container.bindings_snapshot()?.is_empty());
    let scope = container.scope()?;
    let resolver = container.resolver(
        scope.id,
        Arc::clone(&scope.closed),
        scope.resources.registrar(),
    );
    assert!(
        matches!(resolver.resolve_value(typed()).await, Err(error) if error.code() == "service_not_bound")
    );
    assert_eq!(count.load(Ordering::Acquire), 0);
    let descriptor = ticket(&container, &symbol())?;
    assert_eq!(descriptor.kind, SourceKind::Descriptor(ClassKind::Concrete));
    assert_eq!(descriptor.cache.service, typed());
    assert_eq!(descriptor.cache.scope, descriptor.requested_scope);
    assert!(current(&container, &descriptor));
    assert!(
        !lock(&container.inner.state)
            .selection_current(&descriptor, descriptor.requested_scope + 1)
    );
    let result = container
        .register_key::<usize, _>(
            symbol(),
            Lifetime::Singleton,
            |_| Box::pin(async { Ok(Arc::new(7)) }),
            true,
        )
        .await?;
    assert_eq!(result, RegistrationOutcome::Registered);
    let explicit = ticket(&container, &symbol())?;
    assert!(matches!(explicit.kind, SourceKind::Binding(_)));
    assert_eq!(explicit.cache.scope, u64::MAX);
    assert!(!current(&container, &descriptor));
    assert_eq!(*container.resolve::<usize>().await?, 7);
    assert_eq!(count.load(Ordering::Acquire), 0);
    assert!(lock(&container.inner.state).resolved_key(&symbol())?);
    Ok(())
}

#[tokio::test]
async fn instance_only_aba_keeps_revision_and_invalidates_existing_attempt_ticket() -> Result<()> {
    let count = Arc::new(AtomicUsize::new(0));
    let container = builder(&count)?.build();
    let baseline = ticket(&container, &symbol())?;
    // 用同一State slots及原Attempt编号，验证metadata失效；本阶段不启动descriptor任务。
    let attempt_id = {
        let mut state = lock(&container.inner.state);
        let id = state.id()?;
        state.slots.insert(
            baseline.cache.clone(),
            Arc::new(Attempt {
                id,
                service: typed(),
                outcome: Mutex::new(None),
                changed: Notify::new(),
            }),
        );
        id
    };
    let pending = ticket(&container, &symbol())?;
    assert_eq!(pending.attempt, Some(attempt_id));
    assert!(current(&container, &pending));
    container.instance_key(symbol(), Arc::new(9_usize)).await?;
    let instance = ticket(&container, &symbol())?;
    assert!(matches!(instance.kind, SourceKind::Instance(_)));
    assert!(instance.source_revision > baseline.source_revision);
    assert!(!current(&container, &pending));
    assert!(
        !lock(&container.inner.state)
            .slots
            .contains_key(&baseline.cache)
    );
    container.forget_instance_key(symbol()).await?;
    let returned = ticket(&container, &symbol())?;
    assert_eq!(returned.kind, baseline.kind);
    assert_eq!(returned.cache.service, baseline.cache.service);
    assert!(returned.source_revision > instance.source_revision);
    assert!(!current(&container, &baseline));
    assert!(!current(&container, &instance));
    assert!(current(&container, &returned));
    assert!(!container.contains::<usize>());
    assert!(container.bindings_snapshot()?.is_empty());
    let revision = lock(&container.inner.state).next_generation;
    container.forget_instance_key(symbol()).await?;
    assert_eq!(lock(&container.inner.state).next_generation, revision);
    assert!(current(&container, &returned));
    assert_eq!(count.load(Ordering::Acquire), 0);
    Ok(())
}

#[test]
fn alias_path_aba_and_terminal_absence_ignore_unrelated_edges_and_noops() -> Result<()> {
    let count = Arc::new(AtomicUsize::new(0));
    let mut builder = builder(&count)?;
    let entry = ServiceKey::named("entry");
    let front = ServiceKey::named("front");
    let other = ServiceKey::named("App/Other");
    builder.alias_key(symbol(), entry.clone())?;
    builder.alias_key(entry.clone(), front.clone())?;
    let container = builder.build();
    let initial = ticket(&container, &front)?;
    assert_eq!(initial.cache.service, typed());
    assert_eq!(lock(&container.inner.state).next_generation, 0);
    container.alias_key(other.clone(), ServiceKey::named("unrelated"))?;
    assert!(current(&container, &initial));
    let revision = lock(&container.inner.state).next_generation;
    container.alias_key(typed(), entry.clone())?;
    assert_eq!(lock(&container.inner.state).next_generation, revision);
    assert!(
        matches!(container.alias_key(typed(), symbol()), Err(error) if error.code()=="service_alias_self")
    );
    assert_eq!(lock(&container.inner.state).next_generation, revision);
    assert!(current(&container, &initial));
    container.alias_key(other, entry.clone())?;
    assert!(!current(&container, &initial));
    container.alias_key(symbol(), entry)?;
    assert_eq!(lock(&container.inner.state).alias_key(&front)?, typed());
    assert!(!current(&container, &initial));
    assert!(current(&container, &ticket(&container, &front)?));
    assert_eq!(count.load(Ordering::Acquire), 0);
    Ok(())
}

#[tokio::test]
async fn terminal_add_then_real_bind_removal_invalidates_absence_witness_alone() -> Result<()> {
    let count = Arc::new(AtomicUsize::new(0));
    let container = builder(&count)?.build();
    let front = ServiceKey::named("front");
    container.alias_key(symbol(), front.clone())?;
    let witness = lock(&container.inner.state).aliases.witness(&front)?;
    assert!(lock(&container.inner.state).aliases.accepts(&witness));
    container.alias_key(ServiceKey::named("App/Other"), symbol())?;
    assert!(!lock(&container.inner.state).aliases.accepts(&witness));
    container
        .register_key::<usize, _>(
            symbol(),
            Lifetime::Bind,
            |_| Box::pin(async { Ok(Arc::new(8)) }),
            false,
        )
        .await?;
    let state = lock(&container.inner.state);
    assert_eq!(state.alias_key(&front)?, typed());
    assert!(!state.aliases.contains(&typed()));
    // 只验alias见证，独立于本次绑定也推进了source revision。
    assert!(!state.aliases.accepts(&witness));
    assert!(state.aliases.accepts(&state.aliases.witness(&front)?));
    assert_eq!(count.load(Ordering::Acquire), 0);
    Ok(())
}

#[tokio::test]
async fn exhausted_existing_allocator_rejects_real_mutations_without_partial_publish() -> Result<()>
{
    let count = Arc::new(AtomicUsize::new(0));
    let container = builder(&count)?.build();
    let front = ServiceKey::named("front");
    container.alias_key(symbol(), front.clone())?;
    let before = ticket(&container, &front)?;
    lock(&container.inner.state).next_generation = u64::MAX;
    // 同边no-op和缺实例forget不需要分配，即使实际allocator已耗尽。
    container.alias_key(typed(), front.clone())?;
    container.forget_instance_key(symbol()).await?;
    assert!(
        matches!(container.alias_key(ServiceKey::named("App/Other"), front.clone()), Err(error) if error.code()=="container_capacity_exceeded")
    );
    assert!(
        matches!(container.instance_key(front.clone(), Arc::new(4_usize)).await, Err(error) if error.code()=="container_capacity_exceeded")
    );
    assert!(
        matches!(container.instance_key(symbol(), Arc::new(5_usize)).await, Err(error) if error.code()=="container_capacity_exceeded")
    );
    assert!(current(&container, &before));
    let state = lock(&container.inner.state);
    assert_eq!(state.next_generation, u64::MAX);
    assert_eq!(state.alias_key(&front)?, typed());
    assert!(state.bindings.is_empty());
    assert!(state.class_source_revisions.is_empty());
    assert!(state.slots.is_empty());
    Ok(())
}

#[tokio::test]
async fn forgetting_unresolved_factory_preserves_inflight_and_pending_extender() -> Result<()> {
    let constructor_calls = Arc::new(AtomicUsize::new(0));
    let calls = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&calls);
    let release = Arc::new(Notify::new());
    let wait = Arc::clone(&release);
    let mut builder = builder(&constructor_calls)?;
    builder.singleton::<usize, _>(move |_| {
        count.fetch_add(1, Ordering::AcqRel);
        let wait = Arc::clone(&wait);
        Box::pin(async move {
            wait.notified().await;
            Ok(Arc::new(11))
        })
    });
    let container = builder.build();
    let mut original = Box::pin(container.resolve::<usize>());
    assert!(futures_util::poll!(original.as_mut()).is_pending());
    assert_eq!(calls.load(Ordering::Acquire), 1);
    let extenders = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&extenders);
    let mut extending = Box::pin(container.extend::<usize, _>(move |value, _| {
        count.fetch_add(1, Ordering::AcqRel);
        Box::pin(async move { Ok(Arc::new(*value + 10)) })
    }));
    assert!(futures_util::poll!(extending.as_mut()).is_pending());
    let before = ticket(&container, &symbol())?;
    assert!(before.attempt.is_some());
    assert!(
        lock(&container.inner.state)
            .extensions
            .pending_operation(&before.cache)
    );
    let revision = lock(&container.inner.state).next_generation;
    container.forget_instance::<usize>().await?;
    assert_eq!(lock(&container.inner.state).next_generation, revision);
    assert!(current(&container, &before));
    assert!(
        lock(&container.inner.state)
            .extensions
            .pending_operation(&before.cache)
    );
    assert_eq!(extenders.load(Ordering::Acquire), 0);
    release.notify_one();
    assert_eq!(*original.await?, 11);
    extending.await?;
    assert_eq!(*container.resolve::<usize>().await?, 21);
    assert_eq!(calls.load(Ordering::Acquire), 1);
    assert_eq!(extenders.load(Ordering::Acquire), 1);
    assert_eq!(constructor_calls.load(Ordering::Acquire), 0);
    Ok(())
}

#[tokio::test]
async fn real_raw_handle_retains_original_factory_and_cancellation_after_metadata_changes()
-> Result<()> {
    let constructor_calls = Arc::new(AtomicUsize::new(0));
    let calls = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&calls);
    let mut builder = builder(&constructor_calls)?;
    builder.bind::<usize, _>(move |_| {
        count.fetch_add(1, Ordering::AcqRel);
        Box::pin(async { Ok(Arc::new(7)) })
    });
    let container = builder.build();
    let factory = container
        .bindings_snapshot()?
        .into_iter()
        .next()
        .map(|binding| binding.concrete)
        .ok_or_else(|| container_error("test_factory_missing", "缺少原factory"))?;
    let old = ticket(&container, &symbol())?;
    container.instance(Arc::new(9_usize)).await?;
    assert!(!current(&container, &old));
    let resolver = container.factory_resolver_internal()?;
    assert_eq!(
        *factory
            .invoke_internal::<usize>(resolver.clone(), ParameterOverrides::new())
            .await?,
        7
    );
    assert_eq!(*container.resolve::<usize>().await?, 9);
    let cancel = crate::core::Cancellation::new();
    cancel.cancel();
    let execution =
        crate::core::ExecutionContext::new(crate::core::ExecutionMetadata::default(), None, cancel);
    let cancelled = crate::core::with_execution_context(
        execution,
        factory.invoke_internal::<usize>(resolver, ParameterOverrides::new()),
    )
    .await;
    assert!(matches!(cancelled, Err(error) if error.code()=="execution_cancelled"));
    let scope = container.scope()?;
    let scoped = scope.factory_resolver_internal()?;
    scope
        .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(3))
        .await?;
    let closed = factory
        .invoke_internal::<usize>(scoped, ParameterOverrides::new())
        .await;
    assert!(matches!(closed, Err(error) if error.code() == "scope_closed"));
    assert_eq!(calls.load(Ordering::Acquire), 1);
    assert_eq!(constructor_calls.load(Ordering::Acquire), 0);
    assert!(
        !lock(&container.inner.state)
            .resolved_history
            .contains(&typed())
    );
    assert!(lock(&container.inner.state).raw_attempts.is_empty());
    Ok(())
}
