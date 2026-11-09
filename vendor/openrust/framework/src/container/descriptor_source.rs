//! 同一次 State 读取选择实际 Binding 或描述符，并保存有限的来源见证。

use super::{
    Binding, ClassRegistry, Factory, Lifetime, ResolveRequest, Resolver, ResourceRegistrar,
    ScopeStatus, ServiceKey, State,
    class_descriptor::{ClassKind, PreparedConstructor},
    container_error,
    contextual::ContextualImplementation,
    directory::AliasWitness,
};
use crate::Result;
use std::{any::TypeId, sync::Arc};

pub(super) enum ConstructionSource {
    Binding(BindingSource),
    Descriptor(DescriptorSource),
    Missing(ServiceKey),
}

pub(super) struct BindingSource {
    pub(super) key: ServiceKey,
    pub(super) binding: Binding,
    pub(super) aliases: Option<AliasWitness>,
    pub(super) contextual: Option<ContextualSource>,
    pub(super) implicit: bool,
    pub(super) shared_condition_miss: Option<Box<super::class_binding::DiscoveryFlight>>,
}

impl BindingSource {
    /// 已认领的操作和特定代次重绑定只使用规范键。
    pub(super) fn pinned(key: ServiceKey, binding: Binding) -> Self {
        Self {
            key,
            binding,
            aliases: None,
            contextual: None,
            implicit: false,
            shared_condition_miss: None,
        }
    }

    pub(super) fn implicit(
        key: ServiceKey,
        binding: Binding,
        aliases: Option<AliasWitness>,
    ) -> Self {
        Self {
            key,
            binding,
            aliases,
            contextual: None,
            implicit: true,
            shared_condition_miss: None,
        }
    }

    pub(super) fn is_current(&self, state: &State) -> bool {
        self.aliases
            .as_ref()
            .is_none_or(|aliases| state.aliases.accepts(aliases))
            && self.contextual.as_ref().map_or_else(
                || {
                    if self.implicit {
                        return !state.bindings.contains_key(&self.key)
                            && state.class_revision(&self.key) == self.binding.generation;
                    }
                    state
                        .bindings
                        .get(&self.key)
                        .map(|current| current.generation)
                        == Some(self.binding.generation)
                },
                |source| source.is_current(state, &self.key),
            )
    }
}

pub(super) struct ContextualSource {
    consumer: ServiceKey,
    active: Arc<super::AtomicBool>,
    implementation: Arc<ContextualImplementation>,
    flush_revision: u64,
}

impl ContextualSource {
    fn is_current(&self, state: &State, key: &ServiceKey) -> bool {
        self.flush_revision == state.flush_revision
            && self.active.load(super::Ordering::Acquire)
            && state
                .contextual_implementation(&self.consumer, key)
                .is_some_and(|current| Arc::ptr_eq(current, &self.implementation))
    }
}

enum DescriptorEntry {
    Resolution {
        aliases: AliasWitness,
        with_parameters: bool,
        contextual: Option<ContextualSource>,
        after_condition_write: bool,
    },
    Build(ServiceKey),
}

pub(super) struct DescriptorSource {
    entry: DescriptorEntry,
    registry: Arc<ClassRegistry>,
    pub(super) revision: u64,
    scope: u64,
    closed: Arc<ScopeStatus>,
    pub(super) owner: ResourceRegistrar,
}

impl State {
    pub(super) fn class_revision(&self, key: &ServiceKey) -> u64 {
        self.class_source_revisions
            .get(key)
            .copied()
            .unwrap_or(self.flush_revision)
    }

    fn class_lifetime(&self, key: &ServiceKey) -> Lifetime {
        self.classes
            .class(key)
            .filter(|class| class.kind() != ClassKind::Interface)
            .map_or(Lifetime::Bind, |class| class.lifetime)
    }

    pub(super) fn implicit_class_binding(&self, key: &ServiceKey) -> Option<Binding> {
        let mut binding = self.classes.class(key)?.implicit_binding.clone()?;
        binding.generation = self.class_revision(key);
        Some(binding)
    }

    pub(super) fn service_lifetime(&self, key: &ServiceKey, binding: Option<&Binding>) -> Lifetime {
        match binding.map_or(Lifetime::Bind, Binding::effective_lifetime) {
            Lifetime::Bind => self.class_lifetime(key),
            lifetime => lifetime,
        }
    }

    pub(super) fn resolution_binding(
        &self,
        key: &ServiceKey,
        with_parameters: bool,
    ) -> Option<Binding> {
        let mut binding = self.bindings.get(key)?.for_resolution(with_parameters)?;
        if !with_parameters {
            binding.lifetime = self.service_lifetime(key, Some(&binding));
        }
        Some(binding)
    }

    pub(super) fn contextual_implementation(
        &self,
        consumer: &ServiceKey,
        key: &ServiceKey,
    ) -> Option<&Arc<ContextualImplementation>> {
        self.contextual.get_service(consumer, key).or_else(|| {
            self.aliases
                .direct_aliases(key)
                .iter()
                .find_map(|alias| self.contextual.get_service(consumer, alias))
        })
    }

    pub(super) fn contextual_source(
        &self,
        requested: &ServiceKey,
        resolver: &Resolver,
        request: &ResolveRequest,
        pinned: bool,
    ) -> Result<Option<ConstructionSource>> {
        resolver.check_open(self)?;
        let Some((consumer, active)) = resolver.active_consumer_frame() else {
            return Ok(None);
        };
        let aliases = self
            .aliases
            .lookup_witness(&self.classes.canonical_key(requested), pinned)?;
        let key = &aliases.terminal;
        let Some(implementation) = self.contextual_implementation(consumer, key) else {
            return Ok(None);
        };
        let contextual = ContextualSource {
            consumer: consumer.clone(),
            active: Arc::clone(active),
            implementation: Arc::clone(implementation),
            flush_revision: self.flush_revision,
        };
        let (factory, factory_output) = match implementation.as_ref() {
            ContextualImplementation::Factory { factory, output } => {
                self.contextual_factory(factory, *output, request)?
            }
            ContextualImplementation::Service(implementation)
                if self.classes.canonical_key(implementation) == *key =>
            {
                if self.classes.class(key).is_none() {
                    return Ok(Some(ConstructionSource::Missing(key.clone())));
                }
                let descriptor =
                    self.descriptor_source(aliases, resolver, Some(contextual), false)?;
                return Ok(Some(ConstructionSource::Descriptor(descriptor)));
            }
            ContextualImplementation::Service(implementation) => {
                let implementation = implementation.clone();
                let target = request.target().cloned().zip(request.expected_output());
                let frame = resolver.build_consumer.clone();
                let factory: Factory = Arc::new(move |mut child| {
                    let implementation = implementation.clone();
                    let target = target.clone();
                    // 类字符串经 make 解析，继续使用仍活跃的消费类，Closure 工厂才屏蔽它。
                    child.build_consumer.clone_from(&frame);
                    let child = child.for_new_lookup();
                    Box::pin(async move {
                        let request = match target {
                            Some((target, erased_output)) => ResolveRequest::Typed {
                                lookup: Some(implementation),
                                target,
                                erased_output,
                            },
                            None => ResolveRequest::Erased(implementation),
                        };
                        child.resolve_projected(request, None).await
                    })
                });
                (factory, request.expected_output())
            }
            ContextualImplementation::Services(_)
            | ContextualImplementation::Tagged(_)
            | ContextualImplementation::FactoryMany { .. } => {
                return Err(container_error(
                    "container_contextual_variadic_required",
                    "上下文服务集合需要声明为构造变参",
                ));
            }
            ContextualImplementation::Parameter(_)
            | ContextualImplementation::ParameterFactory(_) => {
                return Err(container_error(
                    "container_contextual_need_invalid",
                    "类依赖不能使用普通值来源",
                ));
            }
        };
        Ok(Some(ConstructionSource::Binding(BindingSource {
            key: key.clone(),
            binding: Binding {
                generation: 0,
                pending: Vec::new(),
                lifetime: Lifetime::Bind,
                factory: Some(factory),
                factory_output,
                class_factory: None,
                instance: None,
            },
            aliases: Some(aliases),
            contextual: Some(contextual),
            implicit: false,
            shared_condition_miss: None,
        })))
    }

    fn contextual_factory(
        &self,
        factory: &Factory,
        output: TypeId,
        request: &ResolveRequest,
    ) -> Result<(Factory, Option<TypeId>)> {
        let Some((target, expected)) = request
            .target()
            .zip(request.expected_output())
            .filter(|(_, expected)| *expected != output)
        else {
            return Ok((Arc::clone(factory), Some(output)));
        };
        let projection = self
            .classes
            .output_projection(output, target)
            .cloned()
            .ok_or_else(|| {
                container_error("service_type_mismatch", "上下文工厂声明输出与请求类型不同")
            })?;
        let factory = Arc::clone(factory);
        let projected: Factory = Arc::new(move |child| {
            let projection = projection.clone();
            let future = factory(child.clone());
            Box::pin(async move {
                let value = future.await?;
                match child.container.upgrade() {
                    Some(inner) => child.deliver_output(&inner, value, Some(&projection)),
                    None => child.deliver_after_parameters(value),
                }
            })
        });
        Ok((projected, Some(expected)))
    }

    /// 调用者持有本次实际 State 锁。别名路径只走一次，显式绑定先于类记录。
    pub(super) fn construction_source(
        &self,
        requested: &ServiceKey,
        resolver: &Resolver,
        pinned: bool,
        checked: Option<&mut super::class_binding::CheckedBinding>,
    ) -> Result<ConstructionSource> {
        resolver.check_open(self)?;
        let aliases = self
            .aliases
            .lookup_witness(&self.classes.canonical_key(requested), pinned)?;
        let key = &aliases.terminal;
        let with_parameters = resolver.parameters.is_some();
        if let Some(checked) = checked.filter(|checked| {
            &checked.key == key && checked.mutation == Some(self.class_revision(key))
        }) {
            // 条件未命中时，本次仍构造原类。它登记的工厂供后续解析使用。
            if let Some(class) = self.classes.class(key)
                && let Some(published) = self.bindings.get(key)
                && let Some(flight) = checked.shared_flight.take()
            {
                let lifetime = self.service_lifetime(key, Some(published));
                let mut binding =
                    super::class_binding::ClassFactory::direct_binding(key, lifetime, class.output);
                binding.generation = published.generation;
                return Ok(ConstructionSource::Binding(BindingSource {
                    key: key.clone(),
                    binding,
                    aliases: Some(aliases),
                    contextual: None,
                    implicit: false,
                    shared_condition_miss: Some(flight),
                }));
            }
            return self
                .descriptor_source(aliases, resolver, None, true)
                .map(ConstructionSource::Descriptor);
        }
        if let Some(binding) = self.resolution_binding(key, with_parameters) {
            return Ok(ConstructionSource::Binding(BindingSource {
                key: key.clone(),
                binding,
                aliases: Some(aliases),
                contextual: None,
                implicit: false,
                shared_condition_miss: None,
            }));
        }
        if !with_parameters && let Some(binding) = self.implicit_class_binding(key) {
            return Ok(ConstructionSource::Binding(BindingSource::implicit(
                key.clone(),
                binding,
                Some(aliases),
            )));
        }
        if self.classes.class(key).is_none() {
            return Ok(ConstructionSource::Missing(key.clone()));
        }
        self.descriptor_source(aliases, resolver, None, false)
            .map(ConstructionSource::Descriptor)
    }

    fn descriptor_source(
        &self,
        aliases: AliasWitness,
        resolver: &Resolver,
        contextual: Option<ContextualSource>,
        after_condition_write: bool,
    ) -> Result<DescriptorSource> {
        let revision = self.class_revision(&aliases.terminal);
        self.make_descriptor_source(
            DescriptorEntry::Resolution {
                aliases,
                with_parameters: resolver.parameters.is_some(),
                contextual,
                after_condition_write,
            },
            revision,
            resolver,
        )
    }

    pub(super) fn direct_class_source(
        &self,
        key: ServiceKey,
        resolver: &Resolver,
    ) -> Result<DescriptorSource> {
        resolver.check_open(self)?;
        let key = self.classes.canonical_key_owned(key);
        self.make_descriptor_source(DescriptorEntry::Build(key), 0, resolver)
    }

    fn make_descriptor_source(
        &self,
        entry: DescriptorEntry,
        revision: u64,
        resolver: &Resolver,
    ) -> Result<DescriptorSource> {
        Ok(DescriptorSource {
            entry,
            registry: Arc::clone(&self.classes),
            revision,
            scope: resolver.scope,
            closed: Arc::clone(&resolver.closed),
            owner: resolver.resources()?,
        })
    }
}

impl DescriptorSource {
    pub(super) fn key(&self) -> &ServiceKey {
        match &self.entry {
            DescriptorEntry::Resolution { aliases, .. } => &aliases.terminal,
            DescriptorEntry::Build(key) => key,
        }
    }

    pub(super) fn decorates(&self) -> bool {
        matches!(self.entry, DescriptorEntry::Resolution { .. })
    }

    pub(super) fn records_history(&self) -> bool {
        matches!(
            self.entry,
            DescriptorEntry::Resolution {
                with_parameters: false,
                contextual: None,
                ..
            }
        )
    }

    /// 锁外纯预检，借用同一次选择保留的已冻结目录。
    pub(super) fn prepare(&self, target: &ServiceKey) -> Result<PreparedConstructor<'_>> {
        self.registry.prepare(self.key(), target)
    }

    pub(super) fn same_scope(&self, state: &State, resolver: &Resolver) -> Result<bool> {
        resolver.check_open(state)?;
        Ok(self.scope == resolver.scope
            && Arc::ptr_eq(&self.closed, &resolver.closed)
            && resolver.resources()?.same_owner(&self.owner))
    }

    pub(super) fn is_current(&self, state: &State) -> bool {
        if !Arc::ptr_eq(&self.registry, &state.classes) {
            return false;
        }
        let DescriptorEntry::Resolution {
            aliases,
            with_parameters,
            contextual,
            after_condition_write,
        } = &self.entry
        else {
            return true;
        };
        state.aliases.accepts(aliases)
            && contextual.as_ref().map_or_else(
                || {
                    *after_condition_write
                        || state
                            .bindings
                            .get(self.key())
                            .is_none_or(|binding| *with_parameters && binding.factory.is_none())
                },
                |source| source.is_current(state, self.key()),
            )
            && state.class_revision(self.key()) == self.revision
    }

    /// State→owner 下仅核验来源并认领框架阶段。禁止在 commit 中调用用户代码。
    pub(super) fn claim<T>(
        &self,
        state: &mut State,
        resolver: &Resolver,
        commit: impl FnOnce(&mut State) -> T,
    ) -> Result<Option<T>> {
        if !self.same_scope(state, resolver)? {
            return Ok(None);
        }
        self.owner
            .with_open_owner(|| self.is_current(state).then(|| commit(state)))
    }
}

#[cfg(test)]
mod tests {
    use super::{ConstructionSource, DescriptorSource};
    use crate::Result;
    use crate::container::{
        Container, ContainerBuilder, Resolver, Scope, ServiceKey,
        class_descriptor::ClassDescriptor, container_error, lock,
    };
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    fn build(calls: &Arc<AtomicUsize>) -> Result<Container> {
        let calls = Arc::clone(calls);
        let builder =
            ContainerBuilder::with_class_descriptors(vec![ClassDescriptor::concrete::<usize>(
                "App/Counter",
                vec![],
                move |_| {
                    calls.fetch_add(1, Ordering::AcqRel);
                    Ok(Arc::new(7))
                },
            )])?;
        Ok(builder.build())
    }

    fn resolver(scope: &Scope) -> Resolver {
        scope.container.resolver(
            scope.id,
            Arc::clone(&scope.closed),
            scope.resources.registrar(),
        )
    }

    fn choose(
        container: &Container,
        key: &ServiceKey,
        resolver: &Resolver,
    ) -> Result<DescriptorSource> {
        let choice =
            lock(&container.inner.state).construction_source(key, resolver, false, None)?;
        match choice {
            ConstructionSource::Descriptor(ticket) => Ok(ticket),
            _ => Err(container_error(
                "test_descriptor_missing",
                "应选择真实描述符",
            )),
        }
    }

    #[tokio::test]
    async fn binding_precedes_prepared_descriptor_and_source_aba_rejects_old_claim() -> Result<()> {
        let calls = Arc::new(AtomicUsize::new(0));
        let container = build(&calls)?;
        let scope = container.scope()?;
        let resolver = resolver(&scope);
        let key = ServiceKey::typed::<usize>();
        let alias = ServiceKey::named("Counter");
        container.alias_key(key.clone(), alias.clone())?;
        let ticket = choose(&container, &alias, &resolver)?;
        assert_eq!(ticket.key(), &key);
        assert!(ticket.prepare(&key)?.parameters().is_empty());
        assert!(
            matches!(ticket.prepare(&ServiceKey::typed::<String>()), Err(error)
            if error.code() == "service_type_mismatch")
        );
        assert_eq!(
            ticket.claim(&mut lock(&container.inner.state), &resolver, |_| 4)?,
            Some(4)
        );

        container.instance(Arc::new(9_usize)).await?;
        let actual =
            lock(&container.inner.state).construction_source(&alias, &resolver, false, None)?;
        assert!(matches!(actual, ConstructionSource::Binding(selected)
            if selected.key == key && selected.binding.instance.is_some()));
        container.forget_instance::<usize>().await?;
        assert!(
            ticket
                .claim(&mut lock(&container.inner.state), &resolver, |_| 4)?
                .is_none()
        );
        let ticket = choose(&container, &alias, &resolver)?;
        container.alias_key(ServiceKey::typed::<String>(), alias.clone())?;
        let missing =
            lock(&container.inner.state).construction_source(&alias, &resolver, false, None)?;
        assert!(matches!(missing, ConstructionSource::Missing(target)
            if target == ServiceKey::typed::<String>()));
        container.alias_key(key, alias)?;
        assert!(
            ticket
                .claim(&mut lock(&container.inner.state), &resolver, |_| 4)?
                .is_none()
        );
        assert_eq!(calls.load(Ordering::Acquire), 0);
        Ok(())
    }

    #[tokio::test]
    async fn claim_uses_same_scope_and_actual_owner_close_gate() -> Result<()> {
        let container = build(&Arc::new(AtomicUsize::new(0)))?;
        let first = container.scope()?;
        let second = container.scope()?;
        let first_resolver = resolver(&first);
        let second_resolver = resolver(&second);
        let ticket = choose(&container, &ServiceKey::typed::<usize>(), &first_resolver)?;
        assert!(
            ticket
                .claim(&mut lock(&container.inner.state), &second_resolver, |_| ())?
                .is_none()
        );
        first.resources.seal();
        assert!(
            matches!(ticket.claim(&mut lock(&container.inner.state), &first_resolver, |_| ()),
            Err(error) if error.code() == "resources_closed")
        );
        first.close();
        assert!(
            matches!(ticket.claim(&mut lock(&container.inner.state), &first_resolver, |_| ()),
            Err(error) if error.code() == "scope_closed")
        );
        Ok(())
    }
}
