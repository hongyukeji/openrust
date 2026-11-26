//! 可选的应用级用户快照绑定；原生未装配时保持原有Token哈希契约。
use crate::{
    auth::{AuthError, AuthResult},
    contracts::auth::{Authenticatable, CanResetPassword},
};
use std::{
    collections::BTreeMap,
    sync::{Arc, RwLock},
};

type Binding = dyn Fn(&dyn CanResetPassword) -> AuthResult<Option<String>> + Send + Sync;
type RuleKey = (String, String);
struct Rule {
    user_type: std::any::TypeId,
    callback: Arc<Binding>,
}
#[derive(Default)]
pub(super) struct UserBindings(RwLock<BTreeMap<RuleKey, Rule>>);
impl UserBindings {
    pub fn register<M: Authenticatable>(
        &self,
        key: &str,
        callback: impl Fn(&M) -> AuthResult<String> + Send + Sync + 'static,
    ) -> AuthResult<()> {
        if key.trim().is_empty() {
            return Err(AuthError::Configuration("密码重置绑定名称不能为空"));
        }
        let callback = Arc::new(move |user: &dyn CanResetPassword| {
            let Some(user) = (user as &dyn std::any::Any).downcast_ref::<M>() else {
                return Ok(None);
            };
            let binding = callback(user)?;
            if binding.is_empty() {
                return Err(AuthError::Configuration("密码重置绑定值不能为空"));
            }
            Ok(Some(binding))
        }) as Arc<Binding>;
        self.0.write().expect("password user bindings lock").insert(
            (std::any::type_name::<M>().into(), key.into()),
            Rule {
                user_type: std::any::TypeId::of::<M>(),
                callback,
            },
        );
        Ok(())
    }
    pub fn ensure_capability(&self, user: &dyn Authenticatable) -> AuthResult<()> {
        let source = (user as &dyn std::any::Any).type_id();
        let capability = user
            .as_password_reset()
            .map(|value| (value as &dyn std::any::Any).type_id());
        let rules = self.0.read().expect("password user bindings lock");
        if rules.values().any(|rule| rule.user_type == source)
            && capability.is_some_and(|kind| !rules.values().any(|rule| rule.user_type == kind))
        {
            return Err(AuthError::Configuration(
                "用户的密码重置包装类型尚未注册身份绑定",
            ));
        }
        Ok(())
    }
    pub fn values(&self, user: &dyn CanResetPassword) -> AuthResult<Vec<(RuleKey, String)>> {
        // 回调可访问应用服务；不能在执行回调时持有注册锁。
        let rules = self
            .0
            .read()
            .expect("password user bindings lock")
            .iter()
            .map(|(key, callback)| (key.clone(), callback.callback.clone()))
            .collect::<Vec<_>>();
        let mut values = Vec::new();
        for (key, callback) in rules {
            if let Some(value) = callback(user)? {
                values.push((key, value));
            }
        }
        Ok(values)
    }
}
