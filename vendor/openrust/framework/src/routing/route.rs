//! 当前请求实际命中的路由快照与解析器。

use std::sync::{Arc, Mutex, Weak};

use serde_json::Value;

use crate::core::{Error, Result};

/// 受信应用可替换的同步路由解析器。
pub type RouteResolver = Arc<dyn Fn() -> Result<Option<Route>> + Send + Sync>;

/// 当前请求实际命中的只读路由快照。
#[derive(Clone, Debug, PartialEq)]
pub struct Route {
    name: Option<String>,
    action: Option<String>,
    uri: String,
    domain: Option<String>,
    methods: Vec<String>,
    parameters: Vec<(String, Value)>,
}

impl Route {
    /// 构造可用于可信应用 resolver 的路由快照。
    pub fn new(
        name: Option<String>,
        uri: impl Into<String>,
        domain: Option<String>,
        methods: Vec<String>,
        parameters: Vec<(String, Value)>,
    ) -> Self {
        Self {
            name,
            action: None,
            uri: uri.into(),
            domain,
            methods,
            parameters,
        }
    }

    /// 路由名称；匿名路由返回 `None`。
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// 实际匹配路由的原始动作声明；普通 handler 路由返回 `None`。
    pub fn action(&self) -> Option<&str> {
        self.action.as_deref()
    }

    /// 为可信 resolver 的快照设置动作元数据；不会登记 URL 或执行控制器。
    #[must_use]
    pub fn with_action(mut self, action: impl Into<String>) -> Self {
        self.action = Some(action.into());
        self
    }

    /// 不含首斜线的声明 URI；根路由返回 `/`。
    pub fn uri(&self) -> &str {
        &self.uri
    }

    /// 域约束声明。
    pub fn domain(&self) -> Option<&str> {
        self.domain.as_deref()
    }

    /// 声明支持的方法，保持声明顺序。
    pub fn methods(&self) -> &[String] {
        &self.methods
    }

    /// 保持捕获顺序的已解码参数。
    pub fn parameters(&self) -> &[(String, Value)] {
        &self.parameters
    }

    /// 按参数名获取已绑定值。
    pub fn parameter(&self, name: &str) -> Option<&Value> {
        self.parameters
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value)
    }

    /// 缺少参数时返回默认值。
    pub fn parameter_or(&self, name: &str, default: Value) -> Value {
        self.parameter(name).cloned().unwrap_or(default)
    }

    /// 按 Laravel 通配符名称模式匹配任一候选。
    pub fn named(&self, patterns: &[&str]) -> bool {
        self.name
            .as_deref()
            .is_some_and(|name| patterns.iter().any(|pattern| wildcard_match(pattern, name)))
    }
}

fn wildcard_match(pattern: &str, value: &str) -> bool {
    if !pattern.contains('*') {
        return pattern == value;
    }
    let chunks: Vec<_> = pattern.split('*').collect();
    let first = chunks[0];
    let last = chunks.last().copied().unwrap_or_default();
    if !value.starts_with(first) || !value.ends_with(last) {
        return false;
    }
    let mut remaining = &value[first.len()..];
    for chunk in chunks.iter().skip(1).take(chunks.len().saturating_sub(2)) {
        let Some(index) = remaining.find(chunk) else {
            return false;
        };
        remaining = &remaining[index + chunk.len()..];
    }
    remaining.ends_with(last)
}

#[derive(Default)]
struct RouteState {
    matched: Option<Route>,
    resolver: Option<RouteResolver>,
}

/// 单请求共享的私有路由状态；候选 miss 会清除实际匹配事实。
#[derive(Clone, Default)]
pub(crate) struct RouteSlot(Arc<Mutex<RouteState>>);

impl RouteSlot {
    pub(crate) fn publish(&self, route: Route) -> Result<()> {
        self.0.lock().map_err(|_| poisoned())?.matched = Some(route);
        Ok(())
    }

    pub(crate) fn clear(&self) -> Result<()> {
        self.0.lock().map_err(|_| poisoned())?.matched = None;
        Ok(())
    }

    pub(crate) fn route(&self) -> Result<Option<Route>> {
        let resolver = self.0.lock().map_err(|_| poisoned())?.resolver.clone();
        if let Some(resolver) = resolver {
            resolver()
        } else {
            Ok(self.0.lock().map_err(|_| poisoned())?.matched.clone())
        }
    }

    pub(crate) fn resolver(&self) -> Result<RouteResolver> {
        let mut state = self.0.lock().map_err(|_| poisoned())?;
        if let Some(resolver) = &state.resolver {
            return Ok(Arc::clone(resolver));
        }
        let weak: Weak<Mutex<RouteState>> = Arc::downgrade(&self.0);
        let resolver: RouteResolver = Arc::new(move || {
            let Some(state) = weak.upgrade() else {
                return Ok(None);
            };
            Ok(state.lock().map_err(|_| poisoned())?.matched.clone())
        });
        state.resolver = Some(Arc::clone(&resolver));
        Ok(resolver)
    }

    pub(crate) fn set_resolver(&self, resolver: RouteResolver) -> Result<()> {
        self.0.lock().map_err(|_| poisoned())?.resolver = Some(resolver);
        Ok(())
    }
}

fn poisoned() -> Error {
    Error::custom("http_route_context_poisoned", "请求路由上下文不可用")
}
