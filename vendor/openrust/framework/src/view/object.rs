//! 可信模板的对象调用边界；服务生命周期和解析仍由现有应用容器负责。
use std::{fmt, sync::Arc};

use super::{ViewError, ViewFuture, ViewResult, ViewValue, rendering::render_error};
use crate::{
    container::{
        EnvironmentQuery, EnvironmentResult, ParameterKey, ParameterOverrides, ParameterValue,
    },
    contracts::{config::Repository, container::Container as _, foundation::Application as _},
    foundation::Application,
    support::facades::{App, Facade, WeakContextSnapshot, context_snapshot},
};
type ObjectResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// Rust 服务显式公开给模板的调用接口，不依赖运行时反射。
/// 使用现有 ClassProjection 或 dyn ViewObject 容器绑定暴露实际服务。
#[allow(non_snake_case, reason = "与现有异步容器命名一致")]
pub trait ViewObject: std::any::Any + Send + Sync + 'static {
    /// 对象严格比较使用服务实例身份，而非临时适配器的地址。
    fn identity(&self) -> usize {
        std::ptr::from_ref(self).cast::<()>().addr()
    }
    fn call(&self, method: &str, arguments: &[ViewValue]) -> ViewResult<ViewValue>;

    /// HTTP 渲染由已有请求桥接执行异步调用；同步实现无需重复适配。
    fn callAsync<'a>(
        &'a self,
        method: &'a str,
        arguments: Vec<ViewValue>,
    ) -> ViewFuture<'a, ViewResult<ViewValue>> {
        Box::pin(async move { self.call(method, &arguments) })
    }
}

/// 只在创建时的应用／请求作用域中可用，不保存到模板缓存或 JSON。
#[derive(Clone)]
pub struct ObjectValue {
    value: Arc<dyn ViewObject>,
    context: WeakContextSnapshot,
}
impl ObjectValue {
    pub(super) fn new(value: Arc<dyn ViewObject>) -> Self {
        Self {
            value,
            context: context_snapshot().downgrade(),
        }
    }
    fn check(&self) -> ViewResult<()> {
        if !self.context.is_current() {
            Err(render_error("视图服务对象已离开所属应用或请求作用域。"))
        } else {
            Ok(())
        }
    }
    pub(super) fn call(&self, method: &str, arguments: &[ViewValue]) -> ViewResult<ViewValue> {
        self.check()?;
        self.value.call(method, arguments)
    }
    pub(super) async fn call_async(
        &self,
        method: &str,
        arguments: Vec<ViewValue>,
    ) -> ViewResult<ViewValue> {
        self.check()?;
        self.value.callAsync(method, arguments).await
    }
}
impl fmt::Debug for ObjectValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ViewObject(<service>)")
    }
}
impl PartialEq for ObjectValue {
    fn eq(&self, other: &Self) -> bool {
        self.value.as_ref().type_id() == other.value.as_ref().type_id()
            && self.value.identity() == other.value.identity()
    }
}

pub(super) fn application() -> ViewResult<ViewValue> {
    let app = App::getFacadeRoot().map_err(|source| ViewError::RenderFailed(Box::new(source)))?;
    Ok(ViewValue::from_object(Arc::new(ApplicationObject(
        Arc::downgrade(&app),
    ))))
}
struct ApplicationObject(std::sync::Weak<Application>);
struct ConfigurationObject(Arc<dyn Repository>);

fn arity(arguments: &[ViewValue], minimum: usize, maximum: usize) -> ViewResult<()> {
    if (minimum..=maximum).contains(&arguments.len()) {
        Ok(())
    } else {
        Err(render_error("模板方法参数数量不正确。"))
    }
}
fn string(value: &ViewValue) -> ViewResult<&str> {
    value
        .as_str()
        .ok_or_else(|| render_error("模板方法参数必须是字符串。"))
}
fn parameters(value: Option<&ViewValue>) -> ViewResult<ParameterOverrides> {
    let mut parameters = ParameterOverrides::new();
    let entries: Vec<_> = match value {
        None => Vec::new(),
        Some(ViewValue::Array(values)) => values
            .iter()
            .enumerate()
            .map(|(i, v)| (ParameterKey::Index(i as i64), v))
            .collect(),
        Some(ViewValue::Object(values)) => values
            .iter()
            .map(|(key, value)| (ParameterKey::name(key.as_str()), value))
            .collect(),
        _ => return Err(render_error("服务构造参数必须是数组。")),
    };
    for (key, value) in entries {
        // 模板动态参数以原生标量进入已有类型化容器；复合值保留 ViewValue。
        let value = match value {
            ViewValue::Null => ParameterValue::null(),
            ViewValue::String(value) => ParameterValue::new(value.clone()),
            ViewValue::Bool(value) => ParameterValue::new(*value),
            ViewValue::Number(value) if value.is_i64() => {
                ParameterValue::new(value.as_i64().unwrap())
            }
            ViewValue::Number(value) if value.is_u64() => {
                ParameterValue::new(value.as_u64().unwrap())
            }
            ViewValue::Number(value) => ParameterValue::new(value.as_f64().unwrap()),
            value => ParameterValue::new(value.clone()),
        };
        parameters.insert(key, value);
    }
    Ok(parameters)
}
impl ApplicationObject {
    fn application(&self) -> ObjectResult<Arc<Application>> {
        self.0
            .upgrade()
            .ok_or_else(|| render_error("模板所属应用已结束。").into())
    }
    fn resolve(&self, arguments: &[ViewValue]) -> ObjectResult<ViewValue> {
        arity(arguments, 1, 2)?;
        let key = string(&arguments[0])?;
        let parameters = parameters(arguments.get(1))?;
        let app = self.application()?;
        let container = app.container();
        let canonical = container.getAlias(key)?;
        if canonical == container.getAlias("app")? {
            let _ = container.makeWithNamed::<Application>("app", parameters)?;
            return Ok(ViewValue::from_object(Arc::new(Self(self.0.clone()))));
        }
        if canonical == container.getAlias("config")? {
            return Ok(ViewValue::from_object(Arc::new(ConfigurationObject(
                container.makeWithNamed("config", parameters)?,
            ))));
        }
        Ok(ViewValue::from_object(
            container.makeWithNamed::<dyn ViewObject>(key, parameters)?,
        ))
    }
    async fn resolve_async(&self, arguments: &[ViewValue]) -> ObjectResult<ViewValue> {
        arity(arguments, 1, 2)?;
        let key = string(&arguments[0])?;
        let parameters = parameters(arguments.get(1))?;
        let app = self.application()?;
        let container = app.container();
        let canonical = container.getAlias(key)?;
        if canonical == container.getAlias("app")? || canonical == container.getAlias("config")? {
            return self.resolve(arguments);
        }
        Ok(ViewValue::from_object(
            app.makeWithNamed::<dyn ViewObject>(key, parameters).await?,
        ))
    }
}
impl ApplicationObject {
    fn invoke(&self, method: &str, arguments: &[ViewValue]) -> ObjectResult<ViewValue> {
        let app = self.application()?;
        match method {
            "make" | "makeWith" => self.resolve(arguments),
            "currentLocale" | "getLocale" => {
                arity(arguments, 0, 0)?;
                Ok(app.currentLocale()?.into())
            }
            "isLocale" => {
                arity(arguments, 1, 1)?;
                Ok(app.isLocale(string(&arguments[0])?)?.into())
            }
            "getFallbackLocale" => {
                arity(arguments, 0, 0)?;
                Ok(app.getFallbackLocale()?.into())
            }
            "version" => {
                arity(arguments, 0, 0)?;
                Ok(app.version().into())
            }
            "bound" | "has" | "resolved" | "isShared" => {
                arity(arguments, 1, 1)?;
                let key = string(&arguments[0])?;
                let app = self.application()?;
                let container = app.container();
                Ok(match method {
                    "bound" | "has" => container.has(key),
                    "resolved" => container.resolved(key)?,
                    _ => container.isShared(key),
                }
                .into())
            }
            "environment" => {
                let values = if let [ViewValue::Array(values)] = arguments {
                    values.as_slice()
                } else {
                    arguments
                };
                let patterns = values.iter().map(string).collect::<ViewResult<Vec<_>>>()?;
                let query = if arguments.is_empty() {
                    None
                } else {
                    Some(EnvironmentQuery::Names(&patterns))
                };
                Ok(match app.environment(query)? {
                    EnvironmentResult::String(value) => value.as_ref().into(),
                    EnvironmentResult::Boolean(value) => value.into(),
                })
            }
            "basePath" | "publicPath" | "storagePath" => {
                arity(arguments, 0, 1)?;
                let path = arguments.first().map(string).transpose()?.unwrap_or("");
                let base = match method {
                    "basePath" => app.base_path().to_path_buf(),
                    "publicPath" => app.publicPath(),
                    _ => app.storagePath(""),
                };
                Ok(crate::foundation::paths::join(&base, path)
                    .to_string_lossy()
                    .into_owned()
                    .into())
            }
            "configurationIsCached" | "routesAreCached" | "runningUnitTests" => {
                arity(arguments, 0, 0)?;
                Ok(match method {
                    "configurationIsCached" => app.configurationIsCached(),
                    "routesAreCached" => app.routesAreCached(),
                    _ => app.runningUnitTests()?,
                }
                .into())
            }
            "setFallbackLocale" => {
                arity(arguments, 1, 1)?;
                app.setFallbackLocale(string(&arguments[0])?)?;
                Ok(ViewValue::Null)
            }
            "setLocale" => Err(render_error("异步语言切换需要通过 HTTP 响应渲染。").into()),
            _ => Err(render_error("应用未公开此模板调用方法。").into()),
        }
    }
}
impl ViewObject for ApplicationObject {
    fn identity(&self) -> usize {
        self.0
            .upgrade()
            .map_or(0, |app| Arc::as_ptr(&app.container_handle()).addr())
    }
    fn call(&self, method: &str, arguments: &[ViewValue]) -> ViewResult<ViewValue> {
        self.invoke(method, arguments)
            .map_err(ViewError::RenderFailed)
    }
    fn callAsync<'a>(
        &'a self,
        method: &'a str,
        arguments: Vec<ViewValue>,
    ) -> ViewFuture<'a, ViewResult<ViewValue>> {
        Box::pin(async move {
            let result: ObjectResult<ViewValue> = async {
                match method {
                    "make" | "makeWith" => self.resolve_async(&arguments).await,
                    "setLocale" => {
                        arity(&arguments, 1, 1)?;
                        self.application()?
                            .setLocale(string(&arguments[0])?)
                            .await?;
                        Ok(ViewValue::Null)
                    }
                    _ => self.invoke(method, &arguments),
                }
            }
            .await;
            result.map_err(ViewError::RenderFailed)
        })
    }
}
impl ConfigurationObject {
    fn invoke(&self, method: &str, arguments: &[ViewValue]) -> ObjectResult<ViewValue> {
        match method {
            "get" => {
                arity(arguments, 1, 2)?;
                Ok(self
                    .0
                    .get(string(&arguments[0])?)
                    .map(Into::into)
                    .unwrap_or_else(|| arguments.get(1).cloned().unwrap_or_default()))
            }
            "has" => {
                arity(arguments, 1, 1)?;
                Ok(self.0.has(string(&arguments[0])?).into())
            }
            "all" => {
                arity(arguments, 0, 0)?;
                Ok(ViewValue::Object(
                    self.0
                        .all()
                        .into_iter()
                        .map(|(k, v)| (k, v.into()))
                        .collect(),
                ))
            }
            "set" | "push" | "prepend" => {
                arity(arguments, 2, 2)?;
                let key = string(&arguments[0])?;
                let value = arguments[1]
                    .to_json()
                    .map_err(|e| super::ViewError::RenderFailed(Box::new(e)))?;
                match method {
                    "set" => self.0.set(key, value)?,
                    "push" => self.0.push(key, value)?,
                    _ => self.0.prepend(key, value)?,
                };
                Ok(ViewValue::Null)
            }
            _ => Err(render_error("配置仓储未公开此模板调用方法。").into()),
        }
    }
}

impl ViewObject for ConfigurationObject {
    fn identity(&self) -> usize {
        Arc::as_ptr(&self.0).cast::<()>().addr()
    }
    fn call(&self, method: &str, arguments: &[ViewValue]) -> ViewResult<ViewValue> {
        self.invoke(method, arguments)
            .map_err(ViewError::RenderFailed)
    }
}

pub(super) fn validate_arguments(arguments: &[ViewValue]) -> ViewResult<()> {
    if let Some(value) = arguments.get(1) {
        parameters(Some(value))?;
    }
    Ok(())
}
