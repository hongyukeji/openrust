//! 显式类名绑定的原工厂，沿同一 Resolver 执行直接构造或具体服务解析。

use super::attributes::BindingDeclaration;
use super::directory::AliasWitness;
use super::{
    Arc, Attempt, Binding, BindingChange, ClassRegistry, Container, Factory, Inner, Lifetime,
    ParameterFrame, ResolveRequest, Resolver, Result, ScopeStatus, ServiceKey, State, TypeId,
    Value, WaitEdge, container_error, descriptor_source::ConstructionSource, erase, lock,
};
use crate::config::Environment;
use crate::foundation::lifecycle::callback_future;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

tokio::task_local! {
    static BINDING_CONDITIONS: Vec<Arc<InFlightBinding>>;
}

pub(super) enum BindingDiscovery {
    Owner(DeclarationCheck),
    Wait {
        flight: Arc<InFlightBinding>,
        aliases: AliasWitness,
        edge: Option<WaitEdge>,
    },
}

pub(super) struct InFlightBinding {
    pub(super) attempt: Attempt,
    aliases: AliasWitness,
    environment: Option<Environment>,
    closed: Arc<ScopeStatus>,
    consumer: Option<(ServiceKey, Arc<AtomicBool>)>,
    with_parameters: bool,
    scope: u64,
    initial_revision: u64,
    condition_write: AtomicU64,
}

impl InFlightBinding {
    fn mutation_revision(&self) -> Option<u64> {
        let revision = self.condition_write.load(Ordering::Relaxed);
        (revision != 0).then_some(revision)
    }

    fn is_current(&self, state: &State) -> bool {
        let key = &self.attempt.service;
        let mutation = self.mutation_revision();
        state
            .binding_discoveries
            .get(key)
            .is_some_and(|current| std::ptr::eq(current.as_ref(), self))
            && !self.closed.closed.load(Ordering::Acquire)
            && state.binding_environment == self.environment
            && state.aliases.accepts(&self.aliases)
            && state.class_revision(key) == mutation.unwrap_or(self.initial_revision)
            && (mutation.is_some()
                || state.can_discover_binding(key, self.with_parameters, self.scope))
            && self.consumer.as_ref().is_none_or(|(consumer, active)| {
                !active.load(Ordering::Acquire)
                    || state.contextual_implementation(consumer, key).is_none()
            })
    }

    fn admits_waiter(&self, state: &State, resolver: &Resolver) -> bool {
        self.mutation_revision().is_some()
            || state.can_discover_binding(
                &self.attempt.service,
                resolver.parameters.is_some(),
                resolver.scope,
            )
    }
}

pub(super) struct CheckedBinding {
    pub(super) key: ServiceKey,
    pub(super) mutation: Option<u64>,
    pub(super) shared_flight: Option<Box<DiscoveryFlight>>,
}

pub(super) struct DeclarationCheck {
    flight: DiscoveryFlight,
    classes: Arc<ClassRegistry>,
    termination: Arc<ScopeStatus>,
    conditional: bool,
}

pub(super) struct DiscoveryFlight {
    inner: Arc<Inner>,
    record: Arc<InFlightBinding>,
    active: Arc<AtomicBool>,
    _edge: Option<WaitEdge>,
}

impl DiscoveryFlight {
    pub(super) fn wait_for(
        &self,
        resolver: &Resolver,
        state: &mut State,
        attempt: &Attempt,
    ) -> Result<Option<WaitEdge>> {
        let mut child = resolver.clone();
        child.constructing = Some((self.record.attempt.id, Arc::clone(&self.active)));
        child.wait_edge(&self.inner, state, attempt)
    }

    async fn invalidated(&self) {
        loop {
            let changed = {
                let state = lock(&self.inner.state);
                if !self.record.is_current(&state) {
                    return;
                }
                state.listen_for_binding_change()
            };
            changed.await;
        }
    }
}

impl Drop for DiscoveryFlight {
    fn drop(&mut self) {
        self.active.store(false, Ordering::Release);
        let attempt = &self.record.attempt;
        let mut state = lock(&self.inner.state);
        if state
            .binding_discoveries
            .get(&attempt.service)
            .is_some_and(|current| Arc::ptr_eq(current, &self.record))
        {
            state.binding_discoveries.remove(&attempt.service);
        }
        state.edges.retain(|(from, _), _| *from != attempt.id);
        *lock(&attempt.outcome) = Some(Ok(erase(Arc::new(()))));
        drop(state);
        attempt.changed.notify_waiters();
    }
}

impl BindingDiscovery {
    pub(super) async fn resolve(
        self,
        inner: &Arc<Inner>,
        resolver: &Resolver,
    ) -> Result<Option<CheckedBinding>> {
        match self {
            Self::Owner(check) => check.resolve(resolver).await,
            Self::Wait {
                flight,
                aliases,
                edge: _edge,
            } => {
                let (termination, changed) = {
                    let state = lock(&inner.state);
                    resolver.check_open(&state)?;
                    if !flight.is_current(&state)
                        || !state.aliases.accepts(&aliases)
                        || !flight.admits_waiter(&state, resolver)
                        || resolver
                            .active_consumer_frame()
                            .is_some_and(|(consumer, _)| {
                                state
                                    .contextual_implementation(consumer, &aliases.terminal)
                                    .is_some()
                            })
                    {
                        return Ok(None);
                    }
                    (
                        Arc::clone(&state.termination),
                        state.listen_for_binding_change(),
                    )
                };
                tokio::select! {
                    _ = flight.attempt.wait() => Ok(None),
                    () = changed => Ok(None),
                    () = flight.closed.wait_closed() => Ok(None),
                    () = termination.wait_closed() => Err(container_error("container_closed", "容器已关闭")),
                    () = resolver.closed.wait_closed() => Err(lock(&inner.state).resolution_closure_error()),
                }
            }
        }
    }
}

impl DeclarationCheck {
    async fn resolve(self, resolver: &Resolver) -> Result<Option<CheckedBinding>> {
        let key = &self.flight.record.attempt.service;
        let class = self
            .classes
            .class(key)
            .ok_or_else(|| resolver.missing_service(key))?;
        let selection = tokio::select! {
            biased;
            () = self.flight.invalidated() => return Ok(None),
            selection = self.select(resolver, &class.binds) => selection,
        };
        let concrete = match selection {
            Ok(Some(concrete)) => concrete,
            selection => {
                let mut state = lock(&self.flight.inner.state);
                resolver.check_open(&state)?;
                if !self.flight.record.is_current(&state) {
                    return Ok(None);
                }
                if selection.is_err() || !self.conditional {
                    state.checked_binding_declarations.insert(key.clone());
                }
                selection?;
                let mutation = self.flight.record.mutation_revision();
                let shares_original = mutation.is_some()
                    && !self.flight.record.with_parameters
                    && class.kind() != super::class_descriptor::ClassKind::Interface
                    && state.bindings.get(key).is_some_and(|binding| {
                        state.service_lifetime(key, Some(binding)) != Lifetime::Bind
                    });
                let key = key.clone();
                drop(state);
                return Ok(Some(CheckedBinding {
                    key,
                    mutation,
                    shared_flight: shares_original.then(|| Box::new(self.flight)),
                }));
            }
        };
        let container = Container {
            inner: Arc::clone(&self.flight.inner),
        };
        let binding = ClassFactory::binding_erased(
            &self.classes,
            key,
            concrete,
            class.lifetime,
            key.clone(),
            class.output,
        );
        let (_, rebound) = container.commit_binding_change_when(
            key.clone(),
            BindingChange::Factory(binding),
            |state, _| {
                resolver.check_open(state)?;
                Ok(self.flight.record.is_current(state))
            },
        )?;
        // 工厂已经发布，重绑回调可直接解析同一服务，不必等待声明求值。
        drop(self);
        if let Some(rebound) = rebound {
            rebound.dispatch().await?;
        }
        Ok(None)
    }

    async fn select(
        &self,
        resolver: &Resolver,
        declarations: &[BindingDeclaration],
    ) -> Result<Option<ServiceKey>> {
        let mut fallback = None;
        for declaration in declarations {
            match declaration {
                BindingDeclaration::Environment(bind) => {
                    let Some(environment) = &self.flight.record.environment else {
                        continue;
                    };
                    if bind.environments().is_empty() {
                        return Err(container_error(
                            "container_bind_environment_empty",
                            "环境绑定至少需要一个有效环境",
                        ));
                    }
                    if bind.is_fallback() {
                        fallback.get_or_insert_with(|| bind.concrete().clone());
                    } else if bind.matches(environment) {
                        return Ok(Some(bind.concrete().clone()));
                    }
                }
                BindingDeclaration::Conditional(bind) => {
                    let mut child = resolver.clone();
                    let active = &self.flight.active;
                    let attempt = &self.flight.record.attempt;
                    child
                        .binding_checks
                        .push((attempt.service.clone(), Arc::clone(active)));
                    child.constructing = Some((attempt.id, Arc::clone(active)));
                    let callback = callback_future(
                        resolver.callback_disposal(),
                        bind.condition(),
                        child,
                        Ok(()),
                        None,
                        |condition, resolver, ()| (condition.0)(resolver),
                    );
                    let mut conditions = BINDING_CONDITIONS
                        .try_with(Clone::clone)
                        .unwrap_or_default();
                    conditions.push(Arc::clone(&self.flight.record));
                    let matched = tokio::select! {
                        biased;
                        () = self.termination.wait_closed() => Err(container_error("container_closed", "容器已关闭")),
                        () = resolver.closed.wait_closed() => Err(lock(&self.flight.inner.state).resolution_closure_error()),
                        result = BINDING_CONDITIONS.scope(conditions, callback) => result,
                    }?;
                    if matched {
                        return Ok(Some(bind.concrete().clone()));
                    }
                }
            }
        }
        Ok(fallback)
    }
}

impl State {
    pub(super) fn record_condition_write(&self, key: &ServiceKey, revision: u64) {
        let Some(flight) = self.binding_discoveries.get(key) else {
            return;
        };
        if Self::condition_is_active(flight) && flight.is_current(self) {
            flight.condition_write.store(revision, Ordering::Relaxed);
        }
    }

    fn condition_is_active(flight: &Arc<InFlightBinding>) -> bool {
        BINDING_CONDITIONS
            .try_with(|conditions| conditions.iter().any(|active| Arc::ptr_eq(active, flight)))
            .unwrap_or(false)
    }

    fn listen_for_binding_change(&self) -> std::pin::Pin<Box<tokio::sync::futures::OwnedNotified>> {
        let mut changed = Box::pin(Arc::clone(&self.binding_changes).notified_owned());
        changed.as_mut().enable();
        changed
    }

    fn can_discover_binding(&self, key: &ServiceKey, with_parameters: bool, scope: u64) -> bool {
        !self.aliases.contains(key)
            && !self.checked_binding_declarations.contains(key)
            && self
                .bindings
                .get(key)
                .is_none_or(|binding| with_parameters && binding.factory.is_none())
            && (with_parameters || self.shared_attempt_at(key, scope).is_none())
    }
}

impl Resolver {
    pub(super) fn begin_binding_discovery(
        &self,
        inner: &Arc<Inner>,
        state: &mut State,
        requested: &ServiceKey,
        pinned: bool,
        checked: Option<&ServiceKey>,
    ) -> Result<Option<BindingDiscovery>> {
        self.check_open(state)?;
        let aliases = state
            .aliases
            .lookup_witness(&state.classes.canonical_key(requested), pinned)?;
        let key = &aliases.terminal;
        if checked == Some(key)
            || self
                .binding_checks
                .iter()
                .any(|(service, active)| service == key && active.load(Ordering::Acquire))
        {
            return Ok(None);
        }
        if let Some(flight) = state.binding_discoveries.get(key)
            && flight.is_current(state)
        {
            if State::condition_is_active(flight) || !flight.admits_waiter(state, self) {
                return Ok(None);
            }
            let flight = Arc::clone(flight);
            let edge = self.wait_edge(inner, state, &flight.attempt)?;
            return Ok(Some(BindingDiscovery::Wait {
                flight,
                aliases,
                edge,
            }));
        }
        if !state.can_discover_binding(key, self.parameters.is_some(), self.scope) {
            return Ok(None);
        }
        let Some(class) = state.classes.class(key) else {
            return Ok(None);
        };
        let conditional = class
            .binds
            .iter()
            .any(|bind| matches!(bind, BindingDeclaration::Conditional(_)));
        if class.binds.is_empty() || (!conditional && state.binding_environment.is_none()) {
            return Ok(None);
        }
        let initial_revision = state.class_revision(key);
        let record = Arc::new(InFlightBinding {
            attempt: Attempt {
                id: state.id()?,
                service: key.clone(),
                outcome: std::sync::Mutex::new(None),
                changed: tokio::sync::Notify::new(),
            },
            aliases,
            environment: state.binding_environment.clone(),
            closed: Arc::clone(&self.closed),
            consumer: self.active_consumer_frame().cloned(),
            with_parameters: self.parameters.is_some(),
            scope: self.scope,
            initial_revision,
            condition_write: AtomicU64::new(0),
        });
        let edge = self.wait_edge(inner, state, &record.attempt)?;
        state
            .binding_discoveries
            .insert(record.attempt.service.clone(), Arc::clone(&record));
        let flight = DiscoveryFlight {
            inner: Arc::clone(inner),
            record,
            active: Arc::new(AtomicBool::new(true)),
            _edge: edge,
        };
        Ok(Some(BindingDiscovery::Owner(DeclarationCheck {
            flight,
            classes: Arc::clone(&state.classes),
            termination: Arc::clone(&state.termination),
            conditional,
        })))
    }
}

pub(super) struct ClassFactory {
    concrete: ServiceKey,
    direct: bool,
}

impl ClassFactory {
    pub(super) fn builds_directly(&self) -> bool {
        self.direct
    }

    pub(super) fn binding<T: ?Sized + Send + Sync + 'static>(
        classes: &ClassRegistry,
        key: &ServiceKey,
        concrete: ServiceKey,
        lifetime: Lifetime,
    ) -> Binding {
        Self::binding_erased(
            classes,
            key,
            concrete,
            lifetime,
            ServiceKey::typed::<T>(),
            TypeId::of::<Arc<T>>(),
        )
    }

    pub(super) fn binding_erased(
        classes: &ClassRegistry,
        key: &ServiceKey,
        concrete: ServiceKey,
        lifetime: Lifetime,
        target: ServiceKey,
        output: TypeId,
    ) -> Binding {
        let abstract_key = classes.canonical_key(key);
        Self {
            direct: abstract_key == classes.canonical_key(&concrete),
            concrete,
        }
        .into_binding(lifetime, target, output)
    }

    pub(super) fn direct_binding(key: &ServiceKey, lifetime: Lifetime, output: TypeId) -> Binding {
        Self {
            direct: true,
            concrete: key.clone(),
        }
        .into_binding(lifetime, key.clone(), output)
    }

    fn into_binding(self, lifetime: Lifetime, target: ServiceKey, output: TypeId) -> Binding {
        let class_factory = Arc::new(self);
        Binding {
            generation: 0,
            pending: Vec::new(),
            lifetime,
            factory: Some(class_factory.factory(target, output)),
            factory_output: Some(output),
            class_factory: Some(class_factory),
            instance: None,
        }
    }

    pub(super) fn factory(self: &Arc<Self>, target: ServiceKey, output: TypeId) -> Factory {
        let binding = Arc::clone(self);
        Arc::new(move |resolver| {
            let binding = Arc::clone(&binding);
            let target = target.clone();
            Box::pin(async move { binding.resolve(resolver, target, output).await })
        })
    }

    pub(super) fn supports_view(
        &self,
        state: &State,
        resolver: &Resolver,
        target: ServiceKey,
        output: TypeId,
        with_parameters: bool,
    ) -> Result<bool> {
        let concrete = state.classes.canonical_key(&self.concrete);
        if self.direct {
            return Ok(state.classes.prepare(&concrete, &target).is_ok());
        }
        let request = ResolveRequest::Wrapped {
            concrete: self.concrete.clone(),
            target,
            erased_output: output,
        };
        let target = request
            .target()
            .ok_or_else(|| container_error("service_type_mismatch", "类名包装工厂缺少输出类型"))?;
        if let Some(source) = state.contextual_source(&concrete, resolver, &request, false)? {
            return Ok(match source {
                ConstructionSource::Binding(source) => source
                    .binding
                    .output_projection(&state.classes, &request)
                    .is_ok(),
                ConstructionSource::Descriptor(source) => source.prepare(target).is_ok(),
                ConstructionSource::Missing(_) => false,
            });
        }
        let concrete = state.aliases.resolve(&concrete)?;
        if let Some(binding) = state
            .bindings
            .get(&concrete)
            .and_then(|binding| binding.for_resolution(with_parameters))
        {
            return Ok(binding.output_projection(&state.classes, &request).is_ok());
        }
        Ok(state.classes.prepare(&concrete, target).is_ok())
    }

    async fn resolve(
        &self,
        resolver: Resolver,
        target: ServiceKey,
        output: TypeId,
    ) -> Result<Value> {
        if !self.direct {
            let args = resolver.parameters();
            let request = ResolveRequest::Wrapped {
                concrete: self.concrete.clone(),
                target,
                erased_output: output,
            };
            let child = resolver.for_new_lookup();
            let frame =
                (!args.is_empty()).then(|| ParameterFrame::new(args, child.callback_disposal()));
            return child.resolve_with_frame(request, frame).await;
        }

        let inner = resolver.container()?.inner;
        let source = lock(&inner.state).direct_class_source(self.concrete.clone(), &resolver)?;
        resolver
            .resolve_descriptor(&inner, &source, &target, output, false)
            .await?
            .ok_or_else(|| container_error("binding_replaced", "类构造来源已更换"))
    }
}

#[cfg(test)]
mod tests {
    use super::super::ContainerBuilder;
    use super::*;
    use std::{
        sync::atomic::{AtomicUsize, Ordering},
        time::Duration,
    };

    struct Dependency(u32);
    struct Consumer(Arc<Dependency>);

    #[tokio::test]
    async fn standalone_class_singleton_uses_application_owner_across_scoped_reset() -> Result<()> {
        let cleaned = Arc::new(AtomicUsize::new(0));
        let cleanup = Arc::clone(&cleaned);
        let mut builder = ContainerBuilder::with_class_descriptors(vec![
            super::super::ClassDescriptor::concrete::<Consumer>(
                "app::Consumer",
                vec![super::super::ConstructorParam::class::<Dependency>(
                    "dependency",
                )],
                |args| Ok(Arc::new(Consumer(args.service::<Dependency>(0)?))),
            ),
        ])?;
        builder.bind::<Dependency, _>(move |resolver| {
            let cleanup = Arc::clone(&cleanup);
            Box::pin(async move {
                resolver.resources()?.on_shutdown(move || {
                    Box::pin(async move {
                        cleanup.fetch_add(1, Ordering::SeqCst);
                        Ok(())
                    })
                })?;
                Ok(Arc::new(Dependency(7)))
            })
        });
        let key = ServiceKey::typed::<Consumer>();
        builder.singleton_class::<Consumer>(key.clone(), key);
        let container = builder.build();
        let value = container.resolve::<Consumer>().await?;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        container.forget_scoped_instances(deadline).await?;
        assert!(Arc::ptr_eq(&value, &container.resolve::<Consumer>().await?));
        assert_eq!(value.0.0, 7);
        assert_eq!(cleaned.load(Ordering::SeqCst), 0);
        container.shutdown_root(deadline).await?;
        assert_eq!(cleaned.load(Ordering::SeqCst), 1);
        Ok(())
    }
    struct Scoped;
    struct Recursive;

    #[tokio::test]
    async fn class_wrapper_preserves_scoped_lifetime_and_detects_self_dependency() -> Result<()> {
        use super::super::{ClassDescriptor, ConstructorParam};
        let mut builder = ContainerBuilder::with_class_descriptors(vec![
            ClassDescriptor::concrete::<Scoped>("app::Scoped", vec![], |_| Ok(Arc::new(Scoped))),
            ClassDescriptor::concrete::<Recursive>(
                "app::Recursive",
                vec![ConstructorParam::class::<Recursive>("self")],
                |_| Ok(Arc::new(Recursive)),
            ),
        ])?;
        let scoped = ServiceKey::typed::<Scoped>();
        assert!(builder.scoped_class_if::<Scoped>(scoped.clone(), scoped.clone()));
        assert!(!builder.singleton_class_if::<Scoped>(scoped.clone(), scoped.clone()));
        let recursive = ServiceKey::typed::<Recursive>();
        builder.bind_class::<Recursive>(recursive.clone(), recursive);
        let container = builder.build();
        let first_scope = container.scope()?;
        let second_scope = container.scope()?;
        let first = first_scope.resolve::<Scoped>().await?;
        assert!(Arc::ptr_eq(&first, &first_scope.resolve::<Scoped>().await?));
        assert!(!Arc::ptr_eq(
            &first,
            &second_scope.resolve::<Scoped>().await?
        ));
        assert!(
            matches!(container.resolve::<Recursive>().await, Err(error) if error.code() == "service_cycle")
        );
        Ok(())
    }
    #[tokio::test]
    async fn same_class_raw_reentry_from_extender_constructs_without_repeating_extender()
    -> Result<()> {
        use super::super::ClassDescriptor;
        let builds = Arc::new(AtomicUsize::new(0));
        let called = Arc::clone(&builds);
        let mut builder =
            ContainerBuilder::with_class_descriptors(vec![ClassDescriptor::concrete::<String>(
                "app::String",
                vec![],
                move |_| {
                    called.fetch_add(1, Ordering::SeqCst);
                    Ok(Arc::new(String::from("构造")))
                },
            )])?;
        let key = ServiceKey::typed::<String>();
        builder.bind_class::<String>(key.clone(), key);
        let container = builder.build();
        let binding = container
            .get_bindings()?
            .pop()
            .ok_or_else(|| container_error("test_binding_missing", "缺少类名包装器"))?;
        container
            .extend::<String, _>(move |original, resolver| {
                let factory = binding.concrete.clone();
                Box::pin(async move {
                    let raw = factory.invoke::<String>(resolver).await?;
                    Ok(Arc::new(format!("{original}和{raw}")))
                })
            })
            .await?;
        assert_eq!(container.resolve::<String>().await?.as_str(), "构造和构造");
        assert_eq!(builds.load(Ordering::SeqCst), 2);
        Ok(())
    }

    #[tokio::test]
    async fn different_class_raw_resolves_original_symbol_in_supplied_container() -> Result<()> {
        use super::super::ClassDescriptor;
        let symbol = ServiceKey::named("app::String");
        let mut builder =
            ContainerBuilder::with_class_descriptors(vec![ClassDescriptor::concrete::<String>(
                "app::String",
                vec![],
                |_| Ok(Arc::new(String::from("来源"))),
            )])?;
        builder.bind_class::<String>(ServiceKey::named("abstract"), symbol.clone());
        let source = builder.build();
        let binding = source
            .get_bindings()?
            .pop()
            .ok_or_else(|| container_error("test_binding_missing", "缺少类名包装器"))?;
        let mut builder = ContainerBuilder::new();
        let destination_key = ServiceKey::named("destination");
        builder.bind_key::<u32, _>(destination_key.clone(), |_| {
            Box::pin(async { Ok(Arc::new(42)) })
        });
        builder.alias(destination_key, symbol)?;
        let destination = builder.build();
        let value = binding
            .concrete
            .invoke::<u32>(destination.factory_resolver()?)
            .await?;
        assert_eq!(*value, 42);
        Ok(())
    }
}
