use std::{
    any::{TypeId, type_name},
    collections::HashMap,
    sync::Arc,
};

use super::{
    Argument, AuthError, AuthResult, Container, GateRegistry, Policy, PolicyTarget,
    RegisteredPolicy, TypedPolicy,
};
use crate::contracts::container::Container as _;

pub(super) type PolicyFactory =
    dyn Fn(&Container) -> AuthResult<Arc<dyn RegisteredPolicy>> + Send + Sync;
pub(super) type GuessPolicyNames = dyn Fn(&str) -> Vec<String> + Send + Sync;

/// 已解析的真实策略实例；按具体策略类型取回句柄，不复制策略状态。
#[derive(Clone)]
pub struct PolicyHandle(pub(super) Arc<dyn RegisteredPolicy>);

impl PolicyHandle {
    pub fn name(&self) -> &'static str {
        self.0.name()
    }

    pub fn downcast<P: Send + Sync + 'static>(&self) -> Option<Arc<P>> {
        self.0.instance().downcast().ok()
    }
}

impl GateRegistry {
    /// 将已编译策略登记到发现目录；不提前构造，也不添加显式模型映射。
    pub fn discover_policy<T, P>(&self) -> &Self
    where
        T: Send + Sync + 'static,
        P: Policy<T> + 'static,
    {
        let mut definitions = self.definitions.write().expect("gate registry lock");
        definitions
            .model_names
            .insert(TypeId::of::<T>(), type_name::<T>());
        definitions.policy_types.insert(
            (type_name::<P>().into(), TypeId::of::<T>()),
            Arc::new(|container| {
                let policy = container.make::<P>()?;
                Ok(Arc::new(TypedPolicy::<T, P>(
                    policy,
                    std::marker::PhantomData,
                )))
            }),
        );
        drop(definitions);
        self
    }

    /// 显式策略映射的独立快照；发现目录不会成为显式映射。
    pub fn policies(&self) -> HashMap<TypeId, &'static str> {
        self.definitions
            .read()
            .expect("gate registry lock")
            .policies
            .iter()
            .map(|(target, policy)| (*target, policy.name()))
            .collect()
    }

    pub fn guess_policy_names_using(
        &self,
        callback: impl Fn(&str) -> Vec<String> + Send + Sync + 'static,
    ) -> &Self {
        self.definitions
            .write()
            .expect("gate registry lock")
            .guess_policy_names = Some(Arc::new(callback));
        self
    }

    /// 通过现有容器构造策略，保留绑定的瞬态、共享和错误语义。
    pub fn resolve_policy<P: Send + Sync + 'static>(&self) -> AuthResult<Arc<P>> {
        let owner = self
            .owner
            .upgrade()
            .ok_or_else(|| AuthError::backend(std::io::Error::other("策略解析缺少应用容器")))?;
        owner.make::<P>().map_err(AuthError::backend)
    }

    pub fn get_policy_for(&self, target: &Argument) -> AuthResult<Option<PolicyHandle>> {
        let descriptor = target.downcast_ref::<PolicyTarget>();
        let target_id = descriptor.map_or_else(|| target.type_id(), |target| target.0);
        let (explicit, name, guess) = {
            let definitions = self.definitions.read().expect("gate registry lock");
            (
                definitions.policies.get(&target_id).cloned(),
                descriptor
                    .map(|target| target.1)
                    .or_else(|| definitions.model_names.get(&target_id).copied()),
                definitions.guess_policy_names.clone(),
            )
        };
        if let Some(explicit) = explicit {
            return Ok(Some(PolicyHandle(explicit)));
        }
        let Some(name) = name else {
            return Ok(None);
        };
        // 回调和容器工厂都在释放注册锁后执行，允许依赖解析及回调中的重入。
        let candidates = guess.map_or_else(|| policy_names(name), |guess| guess(name));
        for candidate in candidates {
            let (factory, exists) = {
                let definitions = self.definitions.read().expect("gate registry lock");
                (
                    definitions
                        .policy_types
                        .get(&(candidate.clone(), target_id))
                        .cloned(),
                    definitions
                        .policy_types
                        .keys()
                        .any(|(name, _)| name == &candidate),
                )
            };
            let Some(factory) = factory else {
                if exists {
                    return Err(AuthError::backend(std::io::Error::other(
                        "策略资源类型与目标不一致",
                    )));
                }
                continue;
            };
            let owner = self
                .owner
                .upgrade()
                .ok_or_else(|| AuthError::backend(std::io::Error::other("策略解析缺少应用容器")))?;
            let policy = PolicyHandle(factory(&owner)?);
            return Ok(Some(policy));
        }
        Ok(None)
    }
}

fn policy_names(name: &str) -> Vec<String> {
    let Some((directory, model)) = name.rsplit_once("::") else {
        return Vec::new();
    };
    let segments: Vec<_> = directory.split("::").collect();
    let mut names: Vec<_> = (1..=segments.len())
        .map(|length| format!("{}::policies::{model}Policy", segments[..length].join("::")))
        .collect();
    if directory.contains("::models::") {
        names.push(format!(
            "{}::{model}Policy",
            directory.replace("::models::", "::policies::")
        ));
        names.push(format!(
            "{}::{model}Policy",
            directory.replace("::models::", "::models::policies::")
        ));
    }
    names.reverse();
    names
}
