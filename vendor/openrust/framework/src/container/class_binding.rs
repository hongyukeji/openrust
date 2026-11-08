//! 显式类名绑定的原工厂，沿同一 Resolver 执行直接构造或具体服务解析。

use super::directory::AliasWitness;
use super::{
    Arc, Binding, BindingChange, ClassRegistry, Container, Factory, Lifetime, ParameterFrame,
    ResolveRequest, Resolver, Result, ServiceKey, State, TypeId, Value, container_error,
    descriptor_source::ConstructionSource, lock,
};
use crate::config::Environment;

pub(super) struct DeclaredBinding {
    aliases: AliasWitness,
    environment: Environment,
    binding: Binding,
}

impl DeclaredBinding {
    pub(super) async fn install(self, container: &Container, resolver: &Resolver) -> Result<()> {
        let Self {
            aliases,
            environment,
            binding,
        } = self;
        let (_, rebound) = container.commit_binding_change_when(
            aliases.terminal.clone(),
            BindingChange::Factory(binding),
            |state, key| {
                resolver.check_open(state)?;
                Ok(state.binding_environment.as_ref() == Some(&environment)
                    && state.aliases.accepts(&aliases)
                    && state.can_discover_environment_binding(key, resolver.parameters.is_some())
                    && resolver
                        .active_consumer_frame()
                        .is_none_or(|(consumer, _)| {
                            state.contextual_implementation(consumer, key).is_none()
                        }))
            },
        )?;
        if let Some(rebound) = rebound {
            rebound.dispatch().await?;
        }
        Ok(())
    }
}

impl State {
    fn can_discover_environment_binding(&self, key: &ServiceKey, with_parameters: bool) -> bool {
        !self.aliases.contains(key)
            && !self.checked_environment_bindings.contains(key)
            && self
                .bindings
                .get(key)
                .is_none_or(|binding| with_parameters && binding.factory.is_none())
    }

    pub(super) fn discover_environment_binding(
        &mut self,
        requested: &ServiceKey,
        resolver: &Resolver,
        pinned: bool,
    ) -> Result<Option<DeclaredBinding>> {
        resolver.check_open(self)?;
        let Some(environment) = &self.binding_environment else {
            return Ok(None);
        };
        let aliases = self
            .aliases
            .lookup_witness(&self.classes.canonical_key(requested), pinned)?;
        let key = &aliases.terminal;
        if !self.can_discover_environment_binding(key, resolver.parameters.is_some()) {
            return Ok(None);
        }
        let Some(class) = self.classes.class(key) else {
            return Ok(None);
        };
        if class.binds.is_empty() {
            return Ok(None);
        }
        let mut fallback = None;
        let mut concrete = None;
        for bind in &class.binds {
            if bind.environments().is_empty() {
                self.checked_environment_bindings.insert(key.clone());
                return Err(container_error(
                    "container_bind_environment_empty",
                    "环境绑定至少需要一个有效环境",
                ));
            }
            if bind.is_fallback() {
                fallback = fallback.or(Some(bind.concrete()));
            } else if bind.matches(environment) {
                concrete = Some(bind.concrete());
                break;
            }
        }
        let Some(concrete) = concrete.or(fallback) else {
            self.checked_environment_bindings.insert(key.clone());
            return Ok(None);
        };
        let binding = ClassFactory::binding_erased(
            &self.classes,
            key,
            concrete.clone(),
            Lifetime::Bind,
            key.clone(),
            class.output,
        );
        Ok(Some(DeclaredBinding {
            aliases,
            environment: environment.clone(),
            binding,
        }))
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
        let class_factory = Arc::new(Self {
            direct: abstract_key == classes.canonical_key(&concrete),
            concrete,
        });
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
