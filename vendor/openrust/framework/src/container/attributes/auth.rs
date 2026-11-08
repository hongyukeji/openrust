//! 从当前容器解析具名 Guard 和账户。

use super::{
    Arc, AttributeParameter, AttributeValue, BoxFuture, ContextualAttribute, ParameterValue,
    Resolver, Result,
};
use crate::auth::AuthManager;

/// 注入当前请求的 `Arc<dyn Guard>`。`default()` 使用默认 Guard。
#[derive(Default)]
pub struct Auth(Option<Arc<str>>);

impl Auth {
    /// 按配置名称选择 Guard。
    pub fn new(guard: impl Into<Arc<str>>) -> Self {
        Self(Some(guard.into()))
    }

    /// 返回声明的原始名称，省略时为 `None`。
    pub fn guard_name(&self) -> Option<&str> {
        self.0.as_deref()
    }
}

impl ContextualAttribute for Auth {
    fn resolve(
        self: Arc<Self>,
        _parameter: AttributeParameter,
        resolver: Resolver,
    ) -> BoxFuture<'static, Result<AttributeValue>> {
        Box::pin(async move {
            let manager = resolver.resolve::<AuthManager>().await?;
            let guard = manager.guard_for_attribute(self.guard_name()).await?;
            Ok(AttributeValue::One(ParameterValue::service(guard)))
        })
    }
}

/// 通过当前用户解析器注入账户。`default()` 使用默认 Guard，访客交给容器处理 null。
#[derive(Default)]
pub struct Authenticated(Option<Arc<str>>);

impl Authenticated {
    /// 将指定的 Guard 名称传给用户解析器。
    pub fn new(guard: impl Into<Arc<str>>) -> Self {
        Self(Some(guard.into()))
    }

    /// 返回声明的原始名称，省略时为 `None`。
    pub fn guard_name(&self) -> Option<&str> {
        self.0.as_deref()
    }
}

impl ContextualAttribute for Authenticated {
    fn resolve(
        self: Arc<Self>,
        _parameter: AttributeParameter,
        resolver: Resolver,
    ) -> BoxFuture<'static, Result<AttributeValue>> {
        Box::pin(async move { resolve_user(&resolver, self.guard_name()).await })
    }
}

/// 与 `Authenticated` 同语义的独立属性类型，可单独覆盖或注册后置回调。
#[derive(Default)]
pub struct CurrentUser(Option<Arc<str>>);

impl CurrentUser {
    /// 将指定的 Guard 名称传给用户解析器。
    pub fn new(guard: impl Into<Arc<str>>) -> Self {
        Self(Some(guard.into()))
    }

    /// 返回声明的原始名称，省略时为 `None`。
    pub fn guard_name(&self) -> Option<&str> {
        self.0.as_deref()
    }
}

impl ContextualAttribute for CurrentUser {
    fn resolve(
        self: Arc<Self>,
        _parameter: AttributeParameter,
        resolver: Resolver,
    ) -> BoxFuture<'static, Result<AttributeValue>> {
        Box::pin(async move { resolve_user(&resolver, self.guard_name()).await })
    }
}

async fn resolve_user(resolver: &Resolver, guard: Option<&str>) -> Result<AttributeValue> {
    let manager = resolver.resolve::<AuthManager>().await?;
    let user = manager.user(guard).await?;
    Ok(AttributeValue::One(user.map_or_else(
        ParameterValue::null,
        ParameterValue::service,
    )))
}
