//! 原键目录、方法特定别名与用户 Closure 的原始调用；类目录规范化由装配前置提供。

use super::ParameterOverrides;
use super::{
    Arc, Binding, BindingChange, BoxFuture, ClassDescriptor, ClassRegistry, Container,
    ContainerBuilder, HashMap, HashSet, Lifetime, Resolver, Result, ServiceKey, State, TypeId,
    class_binding::ClassFactory, container_error, lock,
};
mod raw;
mod raw_delivery;
pub(super) use raw::RawRecord;
pub use raw::{BindingSnapshot, FactoryHandle};

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

    pub(super) fn change_needed(&self, target: &ServiceKey, alias: &ServiceKey) -> Result<bool> {
        if target == alias {
            return Err(container_error(
                "service_alias_self",
                format!("服务 `{alias}` 不能别名到自身"),
            ));
        }
        Ok(self.aliases.get(alias) != Some(target))
    }

    pub(super) fn insert(&mut self, target: ServiceKey, alias: ServiceKey) -> Result<()> {
        if self.change_needed(&target, &alias)? {
            // Builder尚未发布，无读票据，不消耗State运行代次。
            self.replace(target, alias);
        }
        Ok(())
    }

    pub(super) fn insert_at(&mut self, target: ServiceKey, alias: ServiceKey, revision: u64) {
        self.revisions.insert(alias.clone(), revision);
        self.replace(target, alias);
    }

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
            pinned: false,
        })
    }

    pub(super) fn lookup_witness(&self, key: &ServiceKey, pinned: bool) -> Result<AliasWitness> {
        if pinned {
            return Ok(AliasWitness {
                edges: Vec::new(),
                terminal: key.clone(),
                terminal_revision: 0,
                pinned: true,
            });
        }
        self.witness(key)
    }

    pub(super) fn direct_aliases(&self, target: &ServiceKey) -> &[ServiceKey] {
        self.reverse.get(target).map_or(&[], Vec::as_slice)
    }

    fn revision(&self, key: &ServiceKey) -> u64 {
        self.revisions.get(key).copied().unwrap_or(0)
    }

    pub(super) fn accepts(&self, witness: &AliasWitness) -> bool {
        witness.pinned
            || (!self.contains(&witness.terminal)
                && self.revision(&witness.terminal) == witness.terminal_revision
                && witness.edges.iter().all(|(alias, target, revision)| {
                    self.aliases.get(alias) == Some(target) && self.revision(alias) == *revision
                }))
    }
}

pub(super) struct AliasWitness {
    edges: Vec<(ServiceKey, ServiceKey, u64)>,
    pub(super) terminal: ServiceKey,
    terminal_revision: u64,
    pinned: bool,
}

/// 条件登记的原子结果；no-op 不执行工厂、通知或清理已有状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RegistrationOutcome {
    /// 本次请求已提交新的绑定。
    Registered,
    /// 原键已有 binding、instance 或 alias，本次请求保持原状态。
    AlreadyBound,
}

impl RegistrationOutcome {
    fn was_registered(self) -> bool {
        self == Self::Registered
    }
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
        class_factory: None,
        factory: Some(Arc::new(move |resolver| {
            let guarded = crate::foundation::lifecycle::callback_future(
                resolver.callback_disposal(),
                Arc::clone(&factory),
                resolver,
                Ok(()),
                None,
                |factory, resolver, ()| factory(resolver),
            );
            Box::pin(async move { Ok(super::erase(guarded.await?)) })
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

    /// 检查原键是否有绑定、实例或别名。类声明本身不算绑定。
    pub fn contains_key(&self, key: &ServiceKey) -> bool {
        self.bound_key(key)
    }

    /// 使 `alias` 指向 `target`。相同规范键不能互为别名，链式循环在查找时返回错误。
    pub fn alias(&mut self, target: ServiceKey, alias: ServiceKey) -> Result<()> {
        self.aliases.insert(
            self.classes.canonical_key_owned(target),
            self.classes.canonical_key_owned(alias),
        )
    }

    #[cfg(test)]
    pub(super) fn alias_key(&mut self, target: ServiceKey, alias: ServiceKey) -> Result<()> {
        self.alias(target, alias)
    }

    /// 在原键登记每次解析均调用的工厂，替换该键已有的绑定或别名。
    pub fn bind_key<T, F>(&mut self, key: ServiceKey, factory: F) -> &mut Self
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(Resolver) -> BoxFuture<'static, Result<Arc<T>>> + Send + Sync + 'static,
    {
        self.register_key::<T, F>(key, Lifetime::Bind, factory, false);
        self
    }

    /// 在原键登记应用内共享的工厂，替换该键已有的绑定或别名。
    pub fn singleton_key<T, F>(&mut self, key: ServiceKey, factory: F) -> &mut Self
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(Resolver) -> BoxFuture<'static, Result<Arc<T>>> + Send + Sync + 'static,
    {
        self.register_key::<T, F>(key, Lifetime::Singleton, factory, false);
        self
    }

    /// 在原键登记每个作用域独立共享的工厂，替换该键已有的绑定或别名。
    pub fn scoped_key<T, F>(&mut self, key: ServiceKey, factory: F) -> &mut Self
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(Resolver) -> BoxFuture<'static, Result<Arc<T>>> + Send + Sync + 'static,
    {
        self.register_key::<T, F>(key, Lifetime::Scoped, factory, false);
        self
    }

    /// 在原键登记临时类工厂，替换该键已有的绑定或别名。
    ///
    /// 类的编译符号与类型键等价。异键实现沿其自身的绑定和生命周期解析。
    pub fn bind_class<T: ?Sized + Send + Sync + 'static>(
        &mut self,
        key: ServiceKey,
        concrete: ServiceKey,
    ) -> &mut Self {
        self.register_class::<T>(key, concrete, Lifetime::Bind, false);
        self
    }

    /// 仅在原键没有绑定、实例或别名时登记临时类工厂，返回是否完成登记。
    pub fn bind_class_if<T: ?Sized + Send + Sync + 'static>(
        &mut self,
        key: ServiceKey,
        concrete: ServiceKey,
    ) -> bool {
        self.register_class::<T>(key, concrete, Lifetime::Bind, true)
            .was_registered()
    }

    /// 在原键登记应用共享类工厂，替换该键已有的绑定或别名。
    pub fn singleton_class<T: ?Sized + Send + Sync + 'static>(
        &mut self,
        key: ServiceKey,
        concrete: ServiceKey,
    ) -> &mut Self {
        self.register_class::<T>(key, concrete, Lifetime::Singleton, false);
        self
    }

    /// 仅在原键没有绑定、实例或别名时登记应用共享类工厂，返回是否完成登记。
    pub fn singleton_class_if<T: ?Sized + Send + Sync + 'static>(
        &mut self,
        key: ServiceKey,
        concrete: ServiceKey,
    ) -> bool {
        self.register_class::<T>(key, concrete, Lifetime::Singleton, true)
            .was_registered()
    }

    /// 在原键登记作用域共享类工厂，替换该键已有的绑定或别名。
    pub fn scoped_class<T: ?Sized + Send + Sync + 'static>(
        &mut self,
        key: ServiceKey,
        concrete: ServiceKey,
    ) -> &mut Self {
        self.register_class::<T>(key, concrete, Lifetime::Scoped, false);
        self
    }

    /// 仅在原键没有绑定、实例或别名时登记作用域共享类工厂，返回是否完成登记。
    pub fn scoped_class_if<T: ?Sized + Send + Sync + 'static>(
        &mut self,
        key: ServiceKey,
        concrete: ServiceKey,
    ) -> bool {
        self.register_class::<T>(key, concrete, Lifetime::Scoped, true)
            .was_registered()
    }

    /// 仅在类型键没有绑定、实例或别名时登记临时工厂，返回是否完成登记。
    pub fn bind_if<T, F>(&mut self, factory: F) -> bool
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(Resolver) -> BoxFuture<'static, Result<Arc<T>>> + Send + Sync + 'static,
    {
        self.bind_key_if(ServiceKey::typed::<T>(), factory)
    }

    /// 仅在原键没有绑定、实例或别名时登记临时工厂，返回是否完成登记。
    ///
    /// 类声明本身不占用原键。已有绑定时跳过登记，保留当前状态。
    pub fn bind_key_if<T, F>(&mut self, key: ServiceKey, factory: F) -> bool
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(Resolver) -> BoxFuture<'static, Result<Arc<T>>> + Send + Sync + 'static,
    {
        self.register_key::<T, F>(key, Lifetime::Bind, factory, true)
            .was_registered()
    }

    /// 仅在类型键没有绑定、实例或别名时登记应用内共享工厂，返回是否完成登记。
    pub fn singleton_if<T, F>(&mut self, factory: F) -> bool
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(Resolver) -> BoxFuture<'static, Result<Arc<T>>> + Send + Sync + 'static,
    {
        self.singleton_key_if(ServiceKey::typed::<T>(), factory)
    }

    /// 仅在原键没有绑定、实例或别名时登记应用内共享工厂，返回是否完成登记。
    ///
    /// 类声明本身不占用原键。已有绑定时跳过登记，保留当前状态。
    pub fn singleton_key_if<T, F>(&mut self, key: ServiceKey, factory: F) -> bool
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(Resolver) -> BoxFuture<'static, Result<Arc<T>>> + Send + Sync + 'static,
    {
        self.register_key::<T, F>(key, Lifetime::Singleton, factory, true)
            .was_registered()
    }

    /// 仅在类型键没有绑定、实例或别名时登记作用域内共享工厂，返回是否完成登记。
    pub fn scoped_if<T, F>(&mut self, factory: F) -> bool
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(Resolver) -> BoxFuture<'static, Result<Arc<T>>> + Send + Sync + 'static,
    {
        self.scoped_key_if(ServiceKey::typed::<T>(), factory)
    }

    /// 仅在原键没有绑定、实例或别名时登记作用域内共享工厂，返回是否完成登记。
    ///
    /// 类声明本身不占用原键。已有绑定时跳过登记，保留当前状态。
    pub fn scoped_key_if<T, F>(&mut self, key: ServiceKey, factory: F) -> bool
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(Resolver) -> BoxFuture<'static, Result<Arc<T>>> + Send + Sync + 'static,
    {
        self.register_key::<T, F>(key, Lifetime::Scoped, factory, true)
            .was_registered()
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
        self.register_binding(key, only_if_unbound, |_, _| {
            binding::<T, F>(lifetime, factory)
        })
    }

    fn register_class<T: ?Sized + Send + Sync + 'static>(
        &mut self,
        key: ServiceKey,
        concrete: ServiceKey,
        lifetime: Lifetime,
        only_if_unbound: bool,
    ) -> RegistrationOutcome {
        self.register_binding(key, only_if_unbound, |classes, key| {
            ClassFactory::binding::<T>(classes, key, concrete, lifetime)
        })
    }

    fn register_binding(
        &mut self,
        key: ServiceKey,
        only_if_unbound: bool,
        make_binding: impl FnOnce(&ClassRegistry, &ServiceKey) -> Binding,
    ) -> RegistrationOutcome {
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
        let binding = make_binding(&self.classes, &key);
        self.bindings.insert(key, binding);
        RegistrationOutcome::Registered
    }

    /// 在原键登记共享实例，移除该键的别名并保留其原有工厂。
    pub fn instance_key<T: ?Sized + Send + Sync + 'static>(
        &mut self,
        key: ServiceKey,
        value: Arc<T>,
    ) -> &mut Self {
        let key = self.classes.canonical_key_owned(key);
        self.aliases.remove(&key);
        self.extensions.discard_pending(&key);
        let value = super::erase(value);
        if let Some(binding) = self.bindings.get_mut(&key) {
            binding.pending.clear();
            binding.instance = Some(value);
        } else {
            self.bindings.insert(key, Binding::instance_only(value));
        }
        self
    }
}

impl State {
    pub(super) fn alias_key(&self, key: &ServiceKey) -> Result<ServiceKey> {
        self.aliases.resolve(&self.classes.canonical_key(key))
    }

    pub(super) fn bound_key(&self, key: &ServiceKey) -> bool {
        self.bound_key_at(key, self.root_scope)
    }

    pub(super) fn bound_key_at(&self, key: &ServiceKey, scope: u64) -> bool {
        let key = self.classes.canonical_key(key);
        self.bindings.contains_key(&key)
            || self.aliases.contains(&key)
            || self.shared_attempt_at(&key, scope).is_some_and(|attempt| {
                lock(&attempt.outcome)
                    .as_ref()
                    .is_some_and(std::result::Result::is_ok)
            })
    }

    pub(super) fn shared_attempt_at(
        &self,
        key: &ServiceKey,
        scope: u64,
    ) -> Option<&Arc<super::Attempt>> {
        let mut cache = super::CacheKey {
            service: key.clone(),
            scope: u64::MAX,
        };
        self.slots.get(&cache).or_else(|| {
            cache.scope = scope;
            self.slots.get(&cache)
        })
    }

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

    pub(super) fn shared_key(&self, key: &ServiceKey) -> bool {
        let key = self.classes.canonical_key(key);
        self.service_lifetime(&key, self.bindings.get(&key)) != Lifetime::Bind
    }
}

impl Container {
    /// 使 `alias` 指向 `target`，后续查找使用新目标。
    ///
    /// 拒绝指向同一个规范键。已登记的别名链出现循环时，跟随别名的操作返回错误。
    pub fn alias(&self, target: ServiceKey, alias: ServiceKey) -> Result<()> {
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
        let changes = Arc::clone(&state.binding_changes);
        drop(state);
        changes.notify_waiters();
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn alias_key(&self, target: ServiceKey, alias: ServiceKey) -> Result<()> {
        self.alias(target, alias)
    }

    /// 检查原键是否有绑定、实例或别名。类声明本身不算绑定。
    pub fn contains_key(&self, key: &ServiceKey) -> bool {
        lock(&self.inner.state).bound_key(key)
    }

    /// 检查原键是否已登记为别名，无须查找目标。
    pub fn is_alias(&self, key: &ServiceKey) -> bool {
        let state = lock(&self.inner.state);
        state.aliases.contains(&state.classes.canonical_key(key))
    }

    /// 返回别名链末端的规范键。未登记别名时返回该键的规范形式。
    pub fn get_alias(&self, key: &ServiceKey) -> Result<ServiceKey> {
        lock(&self.inner.state).alias_key(key)
    }

    /// 跟随别名，检查目标是否有现存实例或成功解析记录。
    pub fn resolved_key(&self, key: &ServiceKey) -> Result<bool> {
        lock(&self.inner.state).resolved_key(key)
    }

    /// 检查原键是否共享实例、工厂或具体类声明，不跟随别名。
    pub fn is_shared_key(&self, key: &ServiceKey) -> bool {
        lock(&self.inner.state).shared_key(key)
    }

    /// 在原键动态登记每次解析均调用的工厂，替换该键已有的绑定或别名。
    pub async fn bind_key<T, F>(&self, key: ServiceKey, factory: F) -> Result<()>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(Resolver) -> BoxFuture<'static, Result<Arc<T>>> + Send + Sync + 'static,
    {
        self.register_key::<T, F>(key, Lifetime::Bind, factory, false)
            .await
            .map(|_| ())
    }

    /// 在原键动态登记应用内共享的工厂，替换该键已有的绑定或别名。
    pub async fn singleton_key<T, F>(&self, key: ServiceKey, factory: F) -> Result<()>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(Resolver) -> BoxFuture<'static, Result<Arc<T>>> + Send + Sync + 'static,
    {
        self.register_key::<T, F>(key, Lifetime::Singleton, factory, false)
            .await
            .map(|_| ())
    }

    /// 在原键动态登记每个作用域独立共享的工厂，替换该键已有的绑定或别名。
    pub async fn scoped_key<T, F>(&self, key: ServiceKey, factory: F) -> Result<()>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(Resolver) -> BoxFuture<'static, Result<Arc<T>>> + Send + Sync + 'static,
    {
        self.register_key::<T, F>(key, Lifetime::Scoped, factory, false)
            .await
            .map(|_| ())
    }

    /// 在原键登记临时类工厂，替换该键已有的绑定或别名。
    ///
    /// 类的编译符号与类型键等价。异键实现沿其自身的绑定和生命周期解析。
    pub async fn bind_class<T: ?Sized + Send + Sync + 'static>(
        &self,
        key: ServiceKey,
        concrete: ServiceKey,
    ) -> Result<()> {
        self.register_class::<T>(key, concrete, Lifetime::Bind, false)
            .await
            .map(|_| ())
    }

    /// 仅在原键没有绑定、实例或别名时登记临时类工厂，返回是否完成登记。
    pub async fn bind_class_if<T: ?Sized + Send + Sync + 'static>(
        &self,
        key: ServiceKey,
        concrete: ServiceKey,
    ) -> Result<bool> {
        self.register_class::<T>(key, concrete, Lifetime::Bind, true)
            .await
            .map(RegistrationOutcome::was_registered)
    }

    /// 在原键登记应用共享类工厂，替换该键已有的绑定或别名。
    pub async fn singleton_class<T: ?Sized + Send + Sync + 'static>(
        &self,
        key: ServiceKey,
        concrete: ServiceKey,
    ) -> Result<()> {
        self.register_class::<T>(key, concrete, Lifetime::Singleton, false)
            .await
            .map(|_| ())
    }

    /// 仅在原键没有绑定、实例或别名时登记应用共享类工厂，返回是否完成登记。
    pub async fn singleton_class_if<T: ?Sized + Send + Sync + 'static>(
        &self,
        key: ServiceKey,
        concrete: ServiceKey,
    ) -> Result<bool> {
        self.register_class::<T>(key, concrete, Lifetime::Singleton, true)
            .await
            .map(RegistrationOutcome::was_registered)
    }

    /// 在原键登记作用域共享类工厂，替换该键已有的绑定或别名。
    pub async fn scoped_class<T: ?Sized + Send + Sync + 'static>(
        &self,
        key: ServiceKey,
        concrete: ServiceKey,
    ) -> Result<()> {
        self.register_class::<T>(key, concrete, Lifetime::Scoped, false)
            .await
            .map(|_| ())
    }

    /// 仅在原键没有绑定、实例或别名时登记作用域共享类工厂，返回是否完成登记。
    pub async fn scoped_class_if<T: ?Sized + Send + Sync + 'static>(
        &self,
        key: ServiceKey,
        concrete: ServiceKey,
    ) -> Result<bool> {
        self.register_class::<T>(key, concrete, Lifetime::Scoped, true)
            .await
            .map(RegistrationOutcome::was_registered)
    }

    /// 仅在类型键没有绑定、实例或别名时登记临时工厂，返回是否完成登记。
    pub async fn bind_if<T, F>(&self, factory: F) -> Result<bool>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(Resolver) -> BoxFuture<'static, Result<Arc<T>>> + Send + Sync + 'static,
    {
        self.bind_key_if(ServiceKey::typed::<T>(), factory).await
    }

    /// 仅在原键没有绑定、实例或别名时登记临时工厂，返回是否完成登记。
    ///
    /// 类声明本身不占用原键。已有绑定时跳过登记，保留当前状态。
    pub async fn bind_key_if<T, F>(&self, key: ServiceKey, factory: F) -> Result<bool>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(Resolver) -> BoxFuture<'static, Result<Arc<T>>> + Send + Sync + 'static,
    {
        self.register_key::<T, F>(key, Lifetime::Bind, factory, true)
            .await
            .map(RegistrationOutcome::was_registered)
    }

    /// 仅在类型键没有绑定、实例或别名时登记应用内共享工厂，返回是否完成登记。
    pub async fn singleton_if<T, F>(&self, factory: F) -> Result<bool>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(Resolver) -> BoxFuture<'static, Result<Arc<T>>> + Send + Sync + 'static,
    {
        self.singleton_key_if(ServiceKey::typed::<T>(), factory)
            .await
    }

    /// 仅在原键没有绑定、实例或别名时登记应用内共享工厂，返回是否完成登记。
    ///
    /// 类声明本身不占用原键。已有绑定时跳过登记，保留当前状态。
    pub async fn singleton_key_if<T, F>(&self, key: ServiceKey, factory: F) -> Result<bool>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(Resolver) -> BoxFuture<'static, Result<Arc<T>>> + Send + Sync + 'static,
    {
        self.register_key::<T, F>(key, Lifetime::Singleton, factory, true)
            .await
            .map(RegistrationOutcome::was_registered)
    }

    /// 仅在类型键没有绑定、实例或别名时登记作用域内共享工厂，返回是否完成登记。
    pub async fn scoped_if<T, F>(&self, factory: F) -> Result<bool>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(Resolver) -> BoxFuture<'static, Result<Arc<T>>> + Send + Sync + 'static,
    {
        self.scoped_key_if(ServiceKey::typed::<T>(), factory).await
    }

    /// 仅在原键没有绑定、实例或别名时登记作用域内共享工厂，返回是否完成登记。
    ///
    /// 类声明本身不占用原键。已有绑定时跳过登记，保留当前状态。
    pub async fn scoped_key_if<T, F>(&self, key: ServiceKey, factory: F) -> Result<bool>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(Resolver) -> BoxFuture<'static, Result<Arc<T>>> + Send + Sync + 'static,
    {
        self.register_key::<T, F>(key, Lifetime::Scoped, factory, true)
            .await
            .map(RegistrationOutcome::was_registered)
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
        self.register_binding(key, binding::<T, F>(lifetime, factory), only_if_unbound)
            .await
    }

    async fn register_class<T: ?Sized + Send + Sync + 'static>(
        &self,
        key: ServiceKey,
        concrete: ServiceKey,
        lifetime: Lifetime,
        only_if_unbound: bool,
    ) -> Result<RegistrationOutcome> {
        let classes = Arc::clone(&lock(&self.inner.state).classes);
        let binding = ClassFactory::binding::<T>(&classes, &key, concrete, lifetime);
        self.register_binding(key, binding, only_if_unbound).await
    }

    async fn register_binding(
        &self,
        key: ServiceKey,
        binding: Binding,
        only_if_unbound: bool,
    ) -> Result<RegistrationOutcome> {
        let (outcome, event) =
            self.commit_binding_change_if(key, BindingChange::Factory(binding), only_if_unbound)?;
        if let Some(event) = event {
            event.dispatch().await?;
        }
        Ok(outcome)
    }

    /// 在原键动态登记共享实例，移除该键的别名并保留其原有工厂。
    pub async fn instance_key<T: ?Sized + Send + Sync + 'static>(
        &self,
        key: ServiceKey,
        value: Arc<T>,
    ) -> Result<()> {
        self.apply_binding_change(key, BindingChange::Instance(super::erase(value)))
            .await
    }

    /// 忘记原键的实例与缓存，保留原工厂及解析记录，不跟随别名。
    pub async fn forget_instance_key(&self, key: ServiceKey) -> Result<()> {
        self.apply_binding_change(key, BindingChange::ForgetInstance)
            .await
    }
}

impl Resolver {
    /// 在当前构造链和作用域内跟随别名，解析指定键的 `T` 输出。
    pub fn resolve_key<T: ?Sized + Send + Sync + 'static>(
        &self,
        key: ServiceKey,
    ) -> BoxFuture<'_, Result<Arc<T>>> {
        self.resolve_lookup(Some(key))
    }

    pub(super) fn resolve_lookup<T: ?Sized + Send + Sync + 'static>(
        &self,
        lookup: Option<ServiceKey>,
    ) -> BoxFuture<'_, Result<Arc<T>>> {
        let resolver = self.clone();
        Self::resolve_from(
            move || (Ok(resolver), ()),
            lookup,
            ParameterOverrides::new(),
        )
    }

    /// 本次原工厂显式实参的只读视图；不表示 ambient constructor 覆盖帧。
    #[cfg(test)]
    pub(super) fn factory_parameters(&self) -> &ParameterOverrides {
        &self.factory_parameters.overrides
    }
}
