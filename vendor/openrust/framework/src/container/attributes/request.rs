//! 从当前 HTTP 入口读取服务端属性或匹配路由参数。

use super::{
    Arc, AttributeParameter, AttributeValue, BoxFuture, ContextualAttribute, ParameterValue,
    Resolver, Result,
};
use crate::foundation::http::{current_request_attribute, current_route_parameter};

/// 注入当前 HTTP 请求的服务端属性；缺少属性时交给容器处理 null。
pub struct RequestAttribute {
    name: Arc<str>,
}

impl RequestAttribute {
    /// 按精确名称选择请求属性。
    pub fn new(name: impl Into<Arc<str>>) -> Self {
        Self { name: name.into() }
    }

    /// 借用声明的请求属性名称。
    pub fn name(&self) -> &str {
        &self.name
    }
}

impl ContextualAttribute for RequestAttribute {
    fn resolve(
        self: Arc<Self>,
        _parameter: AttributeParameter,
        resolver: Resolver,
    ) -> BoxFuture<'static, Result<AttributeValue>> {
        Box::pin(async move {
            let value = current_request_attribute(&resolver, self.name())?;
            Ok(AttributeValue::One(
                value.unwrap_or_else(ParameterValue::null),
            ))
        })
    }
}

/// 注入当前 HTTP 路由中的参数，包括由可信 resolver 提供的类型化绑定。
#[derive(Default)]
pub struct RouteParameter {
    name: Option<Arc<str>>,
}

impl RouteParameter {
    /// 显式选择路由参数名称。`default()` 使用当前构造参数名。
    pub fn new(name: impl Into<Arc<str>>) -> Self {
        Self {
            name: Some(name.into()),
        }
    }

    /// 借用声明的名称；省略名称时返回 `None`。
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }
}

impl ContextualAttribute for RouteParameter {
    fn resolve(
        self: Arc<Self>,
        parameter: AttributeParameter,
        resolver: Resolver,
    ) -> BoxFuture<'static, Result<AttributeValue>> {
        Box::pin(async move {
            let value =
                current_route_parameter(&resolver, self.name().unwrap_or(parameter.name()))?;
            Ok(AttributeValue::One(
                value.unwrap_or_else(ParameterValue::null),
            ))
        })
    }
}

#[cfg(test)]
#[path = "request_tests.rs"]
mod tests;
