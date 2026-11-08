//! 原键目录、方法特定别名与用户 Closure 的原始调用；类目录规范化由装配前置提供。

use super::{
    Arc, Binding, BindingChange, BoxFuture, ClassDescriptor, ClassRegistry, Container,
    ContainerBuilder, HashMap, HashSet, Lifetime, ResolveRequest, Resolver, Result, ServiceKey,
    State, TypeId, Value, container_error,
};
#[cfg(test)]
use super::{ParameterOverrides, lock};
#[cfg(test)]
mod raw;
#[cfg(test)]
mod raw_delivery;
#[cfg(test)]
pub(super) use raw::{FactoryHandle, RawRecord};

#[derive(Default)]
pub(super) struct AliasState {
    aliases: HashMap<ServiceKey, ServiceKey>,
    reverse: HashMap<ServiceKey, Vec<ServiceKey>>,
    revisions: HashMap<ServiceKey, u64>,
}

impl AliasState {
    pub(super) fn contains(&self, key: &ServiceKey) -> bool {
        self.aliases.contains_key(key)
    }

    #[cfg(test)]
    pub(super) fn change_needed(&self, target: &ServiceKey, alias: &ServiceKey) -> Result<bool> {
        if target == alias {
            return Err(container_error(
                "service_alias_self",
                format!("服务 `{alias}` 不能别名到自身"),
            ));
        }
        Ok(self.aliases.get(alias) != Some(target))
    }

    #[cfg(test)]
    pub(super) fn insert(&mut self, target: ServiceKey, alias: ServiceKey) -> Result<()> {
        if self.change_needed(&target, &alias)? {
            // Builder尚未发布，无读票据，不消耗State运行代次。
            self.replace(target, alias);
        }
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn insert_at(&mut self, target: ServiceKey, alias: ServiceKey, revision: u64) {
        self.revisions.insert(alias.clone(), revision);
        self.replace(target, alias);
    }

    #[cfg(test)]
    fn replace(&mut self, target: ServiceKey, alias: ServiceKey) {
        self.remove(&alias);
        self.aliases.insert(alias.clone(), target.clone());
        self.reverse.entry(target).or_default().push(alias);
    }

    pub(super) fn remove_at(&mut self, alias: &ServiceKey, revision: u64) {
        if self.contains(alias) {
            self.remove(alias);
            self.revisions.insert(alias.clone(), revision);
        }
    }

    pub(super) fn remove(&mut self, alias: &ServiceKey) {
        if let Some(target) = self.aliases.remove(alias)
            && let Some(aliases) = self.reverse.get_mut(&target)
        {
            aliases.retain(|key| key != alias);
            if aliases.is_empty() {
                self.reverse.remove(&target);
            }
        }
    }

    pub(super) fn resolve(&self, key: &ServiceKey) -> Result<ServiceKey> {
        self.walk(key, |_, _| {})
    }

    fn walk(
        &self,
        key: &ServiceKey,
        mut observe: impl FnMut(&ServiceKey, &ServiceKey),
    ) -> Result<ServiceKey> {
        let mut key = key.clone();
        let mut seen = HashSet::new();
        while let Some(target) = self.aliases.get(&key) {
            if !seen.insert(key.clone()) {
                return Err(container_error(
                    "service_alias_cycle",
                    format!("服务别名 `{key}` 存在循环"),
                ));
            }
            observe(&key, target);
            key = target.clone();
        }
        Ok(key)
    }

    pub(super) fn witness(&self, key: &ServiceKey) -> Result<AliasWitness> {
        let mut edges = Vec::new();
        let terminal = self.walk(key, |alias, target| {
            edges.push((alias.clone(), target.clone(), self.revision(alias)));
        })?;
        let terminal_revision = self.revision(&terminal);
        Ok(AliasWitness {
            edges,
            terminal,
            terminal_revision,
        })
    }

    fn revision(&self, key: &ServiceKey) -> u64 {
        self.revisions.get(key).copied().unwrap_or(0)
    }

    pub(super) fn accepts(&self, witness: &AliasWitness) -> bool {
        !self.contains(&witness.terminal)
            && self.revision(&witness.terminal) == witness.terminal_revision
            && witness.edges.iter().all(|(alias, target, revision)| {
                self.aliases.get(alias) == Some(target) && self.revision(alias) == *revision
            })
    }
}

pub(super) struct AliasWitness {
    edges: Vec<(ServiceKey, ServiceKey, u64)>,
    pub(super) terminal: ServiceKey,
    terminal_revision: u64,
}

/// 条件登记的原子结果；no-op 不执行工厂、通知或清理已有状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RegistrationOutcome {
    /// 本次请求已提交新的绑定。
    Registered,
    /// 原键已有 binding、instance 或 alias，本次请求保持原状态。
    AlreadyBound,
}

fn binding<T, F>(lifetime: Lifetime, factory: F) -> Binding
where
    T: ?Sized + Send + Sync + 'static,
    F: Fn(Resolver) -> BoxFuture<'static, Result<Arc<T>>> + Send + Sync + 'static,
{
    let factory = Arc::new(factory);
    Binding {
        generation: 0,
        pending: Vec::new(),
        lifetime,
        factory_output: Some(TypeId::of::<Arc<T>>()),
        factory: Some(Arc::new(move |resolver| {
            let guarded = crate::foundation::lifecycle::callback_future(
                resolver.callback_disposal(),
                Arc::clone(&factory),
                resolver,
                Ok(()),
                None,
                |factory, resolver, ()| factory(resolver),
            );
            Box::pin(async move { Ok(Arc::new(guarded.await?) as Value) })
        })),
        instance: None,
    }
}

impl ContainerBuilder {
    // Registry装入前尚无绑定或Provider；build只移动同一Arc进入State。
    pub(super) fn with_class_registry(classes: ClassRegistry) -> Self {
        Self {
            classes: Arc::new(classes),
            ..Self::default()
        }
    }

    /// 使用编译类清单创建空绑定目录。拒绝重复类型、符号及无效参数声明。
    pub fn with_class_descriptors(descriptors: Vec<ClassDescriptor>) -> Result<Self> {
        ClassRegistry::from_descriptors(descriptors).map(Self::with_class_registry)
    }

    #[cfg(test)]
    pub(super) fn with_class_identities(classes: super::class_identity::ClassIdentities) -> Self {
        Self::with_class_registry(ClassRegistry::identity_only(classes))
    }

    pub(super) fn bound_key(&self, key: &ServiceKey) -> bool {
        let key = self.classes.canonical_key(key);
        self.bindings.contains_key(&key) || self.aliases.contains(&key)
    }

    #[cfg(test)]
    pub(super) fn alias_key(&mut self, target: ServiceKey, alias: ServiceKey) -> Result<()> {
        self.aliases.insert(
            self.classes.canonical_key_owned(target),
            self.classes.canonical_key_owned(alias),
        )
    }

    pub(super) fn register_key<T, F>(
        &mut self,
        key: ServiceKey,
        lifetime: Lifetime,
        factory: F,
        only_if_unbound: bool,
    ) -> RegistrationOutcome
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(Resolver) -> BoxFuture<'static, Result<Arc<T>>> + Send + Sync + 'static,
    {
        let key = self.classes.canonical_key_owned(key);
        if only_if_unbound && self.bound_key(&key) {
            return RegistrationOutcome::AlreadyBound;
        }
        self.aliases.remove(&key);
        self.extensions.discard_pending(&key);
        if self
            .bindings
            .get(&key)
            .is_none_or(|binding| binding.factory.is_none())
        {
            self.binding_order.push(key.clone());
        }
        self.bindings
            .insert(key, binding::<T, F>(lifetime, factory));
        RegistrationOutcome::Registered
    }

    pub(super) fn instance_key<T: ?Sized + Send + Sync + 'static>(
        &mut self,
        key: ServiceKey,
        value: Arc<T>,
    ) {
        let key = self.classes.canonical_key_owned(key);
        self.aliases.remove(&key);
        self.extensions.discard_pending(&key);
        let value: Value = Arc::new(value);
        if let Some(binding) = self.bindings.get_mut(&key) {
            binding.pending.clear();
            binding.instance = Some(value);
        } else {
            self.bindings.insert(key, Binding::instance_only(value));
        }
    }
}

impl State {
    pub(super) fn alias_key(&self, key: &ServiceKey) -> Result<ServiceKey> {
        self.aliases.resolve(&self.classes.canonical_key(key))
    }

    pub(super) fn bound_key(&self, key: &ServiceKey) -> bool {
        let key = self.classes.canonical_key(key);
        self.bindings.contains_key(&key) || self.aliases.contains(&key)
    }

    #[cfg(test)]
    pub(super) fn resolved_key(&self, key: &ServiceKey) -> Result<bool> {
        let key = self.alias_key(key)?;
        Ok(self.resolved_history.contains(&key)
            || self
                .bindings
                .get(&key)
                .is_some_and(|binding| binding.instance.is_some())
            || self.slots.iter().any(|(cache, attempt)| {
                cache.service == key
                    && cache.scope == u64::MAX
                    && lock(&attempt.outcome)
                        .as_ref()
                        .is_some_and(std::result::Result::is_ok)
            }))
    }

    #[cfg(test)]
    pub(super) fn shared_key(&self, key: &ServiceKey) -> bool {
        let key = self.classes.canonical_key(key);
        self.bindings
            .get(&key)
            .is_some_and(|binding| binding.effective_lifetime() != Lifetime::Bind)
    }
}

impl Container {
    #[cfg(test)]
    pub(super) fn alias_key(&self, target: ServiceKey, alias: ServiceKey) -> Result<()> {
        let mut state = lock(&self.inner.state);
        if state.terminated {
            return Err(container_error("container_closed", "容器已关闭"));
        }
        let target = state.classes.canonical_key_owned(target);
        let alias = state.classes.canonical_key_owned(alias);
        if !state.aliases.change_needed(&target, &alias)? {
            return Ok(());
        }
        let revision = state.next_source_revision()?;
        state.aliases.insert_at(target, alias, revision);
        state.next_generation = revision;
        Ok(())
    }

    pub(super) async fn register_key<T, F>(
        &self,
        key: ServiceKey,
        lifetime: Lifetime,
        factory: F,
        only_if_unbound: bool,
    ) -> Result<RegistrationOutcome>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(Resolver) -> BoxFuture<'static, Result<Arc<T>>> + Send + Sync + 'static,
    {
        let (outcome, event) = self.commit_binding_change_if(
            key,
            BindingChange::Factory(binding::<T, F>(lifetime, factory)),
            only_if_unbound,
        )?;
        if let Some(event) = event {
            event.dispatch().await?;
        }
        Ok(outcome)
    }

    pub(super) async fn instance_key<T: ?Sized + Send + Sync + 'static>(
        &self,
        key: ServiceKey,
        value: Arc<T>,
    ) -> Result<()> {
        self.apply_binding_change(key, BindingChange::Instance(Arc::new(value)))
            .await
    }

    pub(super) async fn forget_instance_key(&self, key: ServiceKey) -> Result<()> {
        self.apply_binding_change(key, BindingChange::ForgetInstance)
            .await
    }
}

impl Resolver {
    pub(super) fn resolve_key<T: ?Sized + Send + Sync + 'static>(
        &self,
        lookup: impl Into<Option<ServiceKey>>,
    ) -> BoxFuture<'_, Result<Arc<T>>> {
        let request = ResolveRequest::Typed {
            lookup: lookup.into(),
            target: ServiceKey::typed::<T>(),
            erased_output: TypeId::of::<Arc<T>>(),
        };
        let mut child = self.clone();
        #[cfg(test)]
        {
            child.factory_parameters = Arc::new(super::FactoryParameters::empty());
        }
        child.factory_completion = None;
        Box::pin(async move {
            let value = child.resolve_projected(request, None).await?;
            value.downcast_ref::<Arc<T>>().cloned().ok_or_else(|| {
                container_error("service_type_mismatch", "服务的实际输出不符合声明类型")
            })
        })
    }

    /// 本次原工厂显式实参的只读视图；不表示 ambient constructor 覆盖帧。
    #[cfg(test)]
    pub(super) fn factory_parameters(&self) -> &ParameterOverrides {
        &self.factory_parameters.overrides
    }
}
