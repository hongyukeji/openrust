//! 当前请求实际命中的路由快照与解析器。

use std::{
    collections::BTreeMap,
    fmt,
    sync::{Arc, Mutex, Weak},
};

use serde_json::Value;

use crate::container::ParameterValue;
use crate::core::{Error, Result};
use crate::foundation::lifecycle::disposal;
use crate::support::data::arr_get;

/// 受信应用可替换的同步路由解析器。
pub type RouteResolver = Arc<dyn Fn() -> Result<Option<Route>> + Send + Sync>;

/// 当前请求实际命中的只读路由快照。
#[derive(Clone)]
pub struct Route {
    name: Option<String>,
    action: Option<String>,
    uri: String,
    domain: Option<String>,
    methods: Vec<String>,
    parameters: Vec<(String, Value)>,
    bound_parameters: BTreeMap<String, ParameterValue>,
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
            bound_parameters: BTreeMap::new(),
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

    /// 按完整参数名读取原始 JSON 捕获值。
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

    /// 为可信路由快照设置类型化绑定。原始 JSON 捕获值保持不变。
    #[must_use]
    pub fn with_bound_parameter(mut self, name: impl Into<String>, value: ParameterValue) -> Self {
        disposal::dispose(self.bound_parameters.insert(name.into(), value), None);
        self
    }

    /// 读取指定参数的绑定对象或从原始 JSON 捕获值取得类型化值。
    ///
    /// 优先匹配完整的绑定键和捕获键，包括显式 `null`。无精确匹配时，按点路径
    /// 查找原始 JSON 捕获值。此入口保留 [`Self::parameter`] 的精确查找。
    pub fn bound_parameter(&self, name: &str) -> Option<ParameterValue> {
        self.bound_parameters
            .get(name)
            .cloned()
            .or_else(|| self.captured_parameter(name).map(capture_parameter))
    }

    fn captured_parameter(&self, name: &str) -> Option<&Value> {
        self.parameter(name).or_else(|| {
            let (first, rest) = name.split_once('.')?;
            // Arr::get 只在整条路径的入口尝试完整键，随后逐段查找。
            rest.split('.').try_fold(self.parameter(first)?, arr_get)
        })
    }

    /// 按 Laravel 通配符名称模式匹配任一候选。
    pub fn named(&self, patterns: &[&str]) -> bool {
        self.name
            .as_deref()
            .is_some_and(|name| patterns.iter().any(|pattern| wildcard_match(pattern, name)))
    }
}

impl fmt::Debug for Route {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Route")
            .field("name", &self.name)
            .field("action", &self.action)
            .field("uri", &self.uri)
            .field("domain", &self.domain)
            .field("methods", &self.methods)
            .field("parameter_count", &self.parameters.len())
            .field("bound_parameter_count", &self.bound_parameters.len())
            .finish()
    }
}

impl PartialEq for Route {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && self.action == other.action
            && self.uri == other.uri
            && self.domain == other.domain
            && self.methods == other.methods
            && self.parameters == other.parameters
            && self.bound_parameters.len() == other.bound_parameters.len()
            && self
                .bound_parameters
                .iter()
                .zip(&other.bound_parameters)
                .all(|((name, value), (other_name, other_value))| {
                    name == other_name && value.shares_payload(other_value)
                })
    }
}

impl Drop for Route {
    fn drop(&mut self) {
        for (_, value) in std::mem::take(&mut self.bound_parameters) {
            disposal::dispose(value, None);
        }
    }
}

fn capture_parameter(value: &Value) -> ParameterValue {
    match value {
        Value::Null => ParameterValue::null(),
        Value::Bool(value) => ParameterValue::new(*value),
        Value::Number(number) => number
            .as_i64()
            .map(ParameterValue::new)
            .or_else(|| number.as_u64().map(ParameterValue::new))
            .or_else(|| number.as_f64().map(ParameterValue::new))
            .unwrap_or_else(|| ParameterValue::new(value.clone())),
        Value::String(value) => ParameterValue::new(value.clone()),
        Value::Array(_) | Value::Object(_) => ParameterValue::new(value.clone()),
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
    terminating: Arc<[crate::http::middleware::TerminateMiddleware]>,
    resolver: Option<RouteResolver>,
    http_entry_claimed: bool,
}

/// 单请求共享的私有路由状态；候选 miss 会清除实际匹配事实。
#[derive(Clone, Default)]
pub(crate) struct RouteSlot(Arc<Mutex<RouteState>>);

impl RouteSlot {
    pub(crate) fn claim_http_entry(&self) -> bool {
        self.0
            .lock()
            .is_ok_and(|mut state| !std::mem::replace(&mut state.http_entry_claimed, true))
    }

    pub(crate) fn publish(&self, route: Route) -> Result<()> {
        self.publish_terminable(route, Arc::default())
    }

    pub(crate) fn publish_terminable(
        &self,
        route: Route,
        terminating: Arc<[crate::http::middleware::TerminateMiddleware]>,
    ) -> Result<()> {
        let previous = {
            let mut state = self.0.lock().map_err(|_| poisoned())?;
            state.terminating = terminating;
            state.matched.replace(route)
        };
        drop(previous);
        Ok(())
    }

    pub(crate) fn clear(&self) -> Result<()> {
        let previous = {
            let mut state = self.0.lock().map_err(|_| poisoned())?;
            state.terminating = Arc::default();
            state.matched.take()
        };
        drop(previous);
        Ok(())
    }

    pub(crate) fn terminating(
        &self,
    ) -> Result<Arc<[crate::http::middleware::TerminateMiddleware]>> {
        Ok(Arc::clone(
            &self.0.lock().map_err(|_| poisoned())?.terminating,
        ))
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
        let previous = self
            .0
            .lock()
            .map_err(|_| poisoned())?
            .resolver
            .replace(resolver);
        disposal::dispose(previous, None);
        Ok(())
    }
}

fn poisoned() -> Error {
    Error::custom("http_route_context_poisoned", "请求路由上下文不可用")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicBool, Ordering};

    fn route(parameters: &[(&str, Value)]) -> Route {
        Route::new(
            None,
            "articles/{article}",
            None,
            vec!["GET".to_owned()],
            parameters
                .iter()
                .map(|(key, value)| ((*key).to_owned(), value.clone()))
                .collect(),
        )
    }

    fn bound(route: &Route, name: &str) -> Result<ParameterValue> {
        route
            .bound_parameter(name)
            .ok_or_else(|| Error::custom("test_parameter_missing", "测试路由缺少预期参数"))
    }

    #[test]
    fn route_projects_original_types_without_changing_json_captures() -> Result<()> {
        let route = route(&[
            ("article", json!("private-raw-capture")),
            ("flag", json!(false)),
            ("signed", json!(-7)),
            ("unsigned", json!(u64::MAX)),
            ("decimal", json!(1.25)),
            ("null", Value::Null),
            ("array", json!([1, "two"])),
            ("object", json!({"id": 7})),
        ]);
        assert_eq!(
            &*bound(&route, "article")?.downcast::<String>()?,
            "private-raw-capture"
        );
        assert!(!*bound(&route, "flag")?.downcast::<bool>()?);
        assert_eq!(*bound(&route, "signed")?.downcast::<i64>()?, -7);
        assert_eq!(*bound(&route, "unsigned")?.downcast::<u64>()?, u64::MAX);
        assert_eq!(
            bound(&route, "decimal")?.downcast::<f64>()?.to_bits(),
            1.25_f64.to_bits()
        );
        assert_eq!(
            route.parameter("article"),
            Some(&json!("private-raw-capture"))
        );
        assert!(bound(&route, "null")?.is_null());
        assert_eq!(
            &*bound(&route, "array")?.downcast::<Value>()?,
            &json!([1, "two"])
        );
        assert_eq!(
            &*bound(&route, "object")?.downcast::<Value>()?,
            &json!({"id": 7})
        );
        assert!(route.bound_parameter("missing").is_none());
        assert_eq!(route.parameters().len(), 8);
        assert!(!format!("{route:?}").contains("private-raw-capture"));
        Ok(())
    }

    #[test]
    fn route_uses_typed_exact_then_json_exact_then_nested_json() -> Result<()> {
        let original = route(&[
            (
                "filter",
                json!({"roles.0.name": "intermediate literal", "roles": [{"name": "nested"}]}),
            ),
            ("filter.roles.0", json!({"name": "root literal"})),
            ("filter.roles.0.name", json!("exact")),
        ]);
        assert_eq!(
            &*bound(&original, "filter.roles.0.name")?.downcast::<String>()?,
            "exact"
        );
        assert_eq!(
            &*bound(&original, "filter.roles.0")?.downcast::<Value>()?,
            &json!({"name": "root literal"})
        );
        let model = Arc::new(String::from("private-bound-model"));
        let typed = original.clone().with_bound_parameter(
            "filter.roles.0.name",
            ParameterValue::from_arc(Arc::clone(&model)),
        );
        assert!(Arc::ptr_eq(
            &bound(&typed, "filter.roles.0.name")?.downcast()?,
            &model
        ));
        assert_eq!(
            typed.parameter("filter.roles.0.name"),
            Some(&json!("exact"))
        );
        assert!(!format!("{typed:?}").contains("private-bound-model"));
        assert_eq!(typed, typed.clone());
        assert_ne!(typed, original);
        let separate = original.clone().with_bound_parameter(
            "filter.roles.0.name",
            ParameterValue::new(String::from("private-bound-model")),
        );
        assert_ne!(typed, separate);
        let null = typed.with_bound_parameter("filter.roles.0.name", ParameterValue::null());
        assert!(bound(&null, "filter.roles.0.name")?.is_null());
        assert_eq!(
            null,
            original.with_bound_parameter("filter.roles.0.name", ParameterValue::null())
        );

        let nested = route(&[(
            "filter",
            json!({"roles.0.name": "literal", "roles": [{"name": "nested"}]}),
        )]);
        assert!(nested.parameter("filter.roles.0.name").is_none());
        assert_eq!(
            &*bound(&nested, "filter.roles.0.name")?.downcast::<String>()?,
            "nested"
        );
        assert!(nested.bound_parameter("filter.roles.00.name").is_none());
        assert!(nested.bound_parameter("filter.roles.*.name").is_none());
        Ok(())
    }

    #[test]
    fn route_slot_has_one_physical_owner_and_retires_bindings_outside_lock() -> Result<()> {
        struct ObserveDrop {
            slot: Weak<Mutex<RouteState>>,
            unlocked: Arc<AtomicBool>,
        }

        impl Drop for ObserveDrop {
            fn drop(&mut self) {
                if let Some(slot) = self.slot.upgrade() {
                    self.unlocked
                        .store(slot.try_lock().is_ok(), Ordering::Relaxed);
                }
            }
        }

        let slot = RouteSlot::default();
        assert!(slot.claim_http_entry());
        assert!(!slot.clone().claim_http_entry());
        assert!(RouteSlot::default().claim_http_entry());
        for replace in [false, true] {
            let unlocked = Arc::new(AtomicBool::new(false));
            let value = ParameterValue::new(ObserveDrop {
                slot: Arc::downgrade(&slot.0),
                unlocked: Arc::clone(&unlocked),
            });
            slot.publish(route(&[]).with_bound_parameter("article", value))?;
            if replace {
                slot.publish(route(&[]))?;
            } else {
                slot.clear()?;
            }
            assert!(unlocked.load(Ordering::Relaxed));
        }
        Ok(())
    }
}
