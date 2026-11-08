//! 同一次 State 读取选择实际 Binding 或描述符，并保存有限的来源见证。

use super::{
    Binding, ClassRegistry, Resolver, ResourceRegistrar, ScopeStatus, ServiceKey, State,
    class_descriptor::PreparedConstructor, directory::AliasWitness,
};
use crate::Result;
use std::sync::Arc;

pub(super) enum ConstructionSource {
    Binding { key: ServiceKey, binding: Binding },
    Descriptor(DescriptorSource),
    Missing(ServiceKey),
}

pub(super) struct DescriptorSource {
    aliases: AliasWitness,
    registry: Arc<ClassRegistry>,
    pub(super) revision: u64,
    scope: u64,
    closed: Arc<ScopeStatus>,
    pub(super) owner: ResourceRegistrar,
}

impl State {
    /// 调用者持有本次实际 State 锁。别名路径只走一次，显式绑定先于类记录。
    pub(super) fn construction_source(
        &self,
        requested: &ServiceKey,
        resolver: &Resolver,
    ) -> Result<ConstructionSource> {
        resolver.check_open(self)?;
        let aliases = self
            .aliases
            .witness(&self.classes.canonical_key(requested))?;
        let key = &aliases.terminal;
        if let Some(binding) = self.bindings.get(key) {
            return Ok(ConstructionSource::Binding {
                key: key.clone(),
                binding: binding.clone(),
            });
        }
        if self.classes.class(key).is_none() {
            return Ok(ConstructionSource::Missing(key.clone()));
        }
        let revision = self.class_source_revisions.get(key).copied().unwrap_or(0);
        Ok(ConstructionSource::Descriptor(DescriptorSource {
            aliases,
            registry: Arc::clone(&self.classes),
            revision,
            scope: resolver.scope,
            closed: Arc::clone(&resolver.closed),
            owner: resolver.resources()?,
        }))
    }
}

impl DescriptorSource {
    pub(super) fn key(&self) -> &ServiceKey {
        &self.aliases.terminal
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
        Arc::ptr_eq(&self.registry, &state.classes)
            && state.aliases.accepts(&self.aliases)
            && !state.bindings.contains_key(self.key())
            && state
                .class_source_revisions
                .get(self.key())
                .copied()
                .unwrap_or(0)
                == self.revision
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
        let choice = lock(&container.inner.state).construction_source(key, resolver)?;
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
        let actual = lock(&container.inner.state).construction_source(&alias, &resolver)?;
        assert!(
            matches!(actual, ConstructionSource::Binding { key: selected, binding }
            if selected == key && binding.instance.is_some())
        );
        container.forget_instance::<usize>().await?;
        assert!(
            ticket
                .claim(&mut lock(&container.inner.state), &resolver, |_| 4)?
                .is_none()
        );
        let ticket = choose(&container, &alias, &resolver)?;
        container.alias_key(ServiceKey::typed::<String>(), alias.clone())?;
        let missing = lock(&container.inner.state).construction_source(&alias, &resolver)?;
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
