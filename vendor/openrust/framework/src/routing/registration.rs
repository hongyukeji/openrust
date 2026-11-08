//! 路由和命名middleware的应用登记仓储。

use crate::core::Result;
use crate::foundation::BootContext;
use crate::routing::Router;
use crate::routing::router::{MiddlewareLayerBuilder, MiddlewareRegistry};
use axum::extract::Request;
use axum::response::IntoResponse;
use axum::routing::Route as AxumRoute;
use std::convert::Infallible;
use std::sync::Arc;
use tower::{Layer, Service};
pub(crate) type RouteBuilder = Box<dyn Fn(Router) -> Router + Send + Sync>;

/// 注册阶段收集的路由构建器；构建 router 时按顺序应用。
#[derive(Default)]
pub struct RouteRegistry {
    pub(crate) builders: Vec<RouteBuilder>,
    pub(crate) middleware: MiddlewareRegistry,
    pub(crate) root_controller_namespace: Option<String>,
    pub(crate) signed_except: Vec<String>,
    #[cfg(feature = "session")]
    pub(crate) forgery: crate::http::PreventRequestForgery,
}

impl RouteRegistry {
    /// 已注册的路由构建器数量。
    pub fn len(&self) -> usize {
        self.builders.len()
    }

    /// 没有注册任何路由时返回 `true`。
    pub fn is_empty(&self) -> bool {
        self.builders.is_empty()
    }
}

impl BootContext<'_> {
    /// 设置本应用默认 web 组的请求伪造策略；仅影响该应用最终 Router。
    #[cfg(feature = "session")]
    pub fn prevent_request_forgery(
        &mut self,
        policy: crate::http::PreventRequestForgery,
    ) -> Result<&mut Self> {
        self.extension_mut::<RouteRegistry>()?.forgery = policy;
        Ok(self)
    }

    /// 在 Provider boot 阶段贡献路由。
    pub fn routes<F>(&mut self, builder: F) -> Result<&mut Self>
    where
        F: Fn(Router) -> Router + Send + Sync + 'static,
    {
        self.extension_mut::<RouteRegistry>()?
            .builders
            .push(Box::new(builder));
        Ok(self)
    }

    /// 设置最终动作 URL 生成器的查询根命名空间；不会修改路由组的登记空间。
    ///
    /// 空字符串清空默认空间，控制字符以 `http_route_action_invalid` 返回错误。
    pub fn set_root_controller_namespace(
        &mut self,
        namespace: impl Into<String>,
    ) -> Result<&mut Self> {
        let namespace = namespace.into();
        crate::routing::action::validate_namespace(&namespace)?;
        self.extension_mut::<RouteRegistry>()?
            .root_controller_namespace = (!namespace.is_empty()).then_some(namespace);
        Ok(self)
    }

    /// 为本应用的全部默认 `signed` middleware 设置全局忽略字段。
    ///
    /// 默认与参数化别名在 router 构建时读取同一列表；另一 App 不受影响。
    pub fn signed_url_except<I, S>(&mut self, keys: I) -> Result<&mut Self>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let except = &mut self.extension_mut::<RouteRegistry>()?.signed_except;
        for key in keys.into_iter().map(Into::into) {
            if !except.contains(&key) {
                except.push(key);
            }
        }
        Ok(self)
    }

    /// 在 Provider boot 阶段登记 middleware alias，供应用路由按名称引用。
    pub fn middleware_alias<L>(&mut self, name: impl Into<String>, layer: L) -> Result<&mut Self>
    where
        L: Layer<AxumRoute> + Clone + Send + Sync + 'static,
        L::Service: Service<Request> + Clone + Send + Sync + 'static,
        <L::Service as Service<Request>>::Response: IntoResponse + 'static,
        <L::Service as Service<Request>>::Error: Into<Infallible> + 'static,
        <L::Service as Service<Request>>::Future: Send + 'static,
    {
        let builder: MiddlewareLayerBuilder = Arc::new(move |routes| routes.layer(layer.clone()));
        self.extension_mut::<RouteRegistry>()?
            .middleware
            .alias(name.into(), builder)?;
        Ok(self)
    }

    /// 登记同一middleware registry的参数化alias；参数按逗号拆分且保留空项。
    ///
    /// 无冒号引用传入空Vec，`name:`传入一个空字符串；参数数量与语义由工厂校验。
    /// 工厂在路由装配时执行，返回错误会阻止应用启动；不能从工厂创建另一请求scope。
    pub fn middleware_alias_with_parameters<L, F>(
        &mut self,
        name: impl Into<String>,
        factory: F,
    ) -> Result<&mut Self>
    where
        F: Fn(Vec<String>) -> Result<L> + Send + Sync + 'static,
        L: Layer<AxumRoute> + Clone + Send + Sync + 'static,
        L::Service: Service<Request> + Clone + Send + Sync + 'static,
        <L::Service as Service<Request>>::Response: IntoResponse + 'static,
        <L::Service as Service<Request>>::Error: Into<Infallible> + 'static,
        <L::Service as Service<Request>>::Future: Send + 'static,
    {
        self.extension_mut::<RouteRegistry>()?
            .middleware
            .parameter_alias(
                name.into(),
                Arc::new(move |parameters| {
                    let layer = factory(parameters)?;
                    let builder: MiddlewareLayerBuilder =
                        Arc::new(move |routes| routes.layer(layer.clone()));
                    Ok(builder)
                }),
            )?;
        Ok(self)
    }

    /// 登记由容器解析的终止中间件别名，可在路由组中引用并附带参数。
    /// 在 `register` 阶段绑定 `M`，`handle` 与 `terminate` 分别解析同一绑定。
    pub fn middleware_alias_terminable<M: crate::http::middleware::TerminableMiddleware>(
        &mut self,
        name: impl Into<String>,
    ) -> Result<&mut Self> {
        let registry = &mut self.extension_mut::<RouteRegistry>()?.middleware;
        registry.parameter_alias(
            name.into(),
            Arc::new(|arguments| {
                let arguments: Arc<[String]> = arguments.into();
                Ok(Arc::new(move |routes| {
                    routes.terminable_layer::<M>(Arc::clone(&arguments))
                }))
            }),
        )?;
        registry.has_terminable = true;
        Ok(self)
    }

    /// 在 Provider boot 阶段登记由 alias 或其他 group 组成的 middleware group。
    pub fn middleware_group<I, S>(
        &mut self,
        name: impl Into<String>,
        members: I,
    ) -> Result<&mut Self>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let members = members.into_iter().map(Into::into).collect();
        self.extension_mut::<RouteRegistry>()?
            .middleware
            .group(name.into(), members)?;
        Ok(self)
    }

    /// 设置路由命名 middleware 的优先级；不启用或自动挂载其中的 alias。
    ///
    /// 可先于 alias 登记；组展开后沿统一路由链排序，未列出的项保持相对顺序。
    /// 全局 `http_layer` 与原生 Axum 逃生入口不属于此路由优先级表。
    pub fn middleware_priority<I, S>(&mut self, names: I) -> Result<&mut Self>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.extension_mut::<RouteRegistry>()?
            .middleware
            .priority(names.into_iter().map(Into::into).collect())?;
        Ok(self)
    }

    /// 将一个 alias 放到路由优先级表首部；不改变全局 HTTP layer。
    pub fn prepend_middleware_priority(&mut self, name: impl Into<String>) -> Result<&mut Self> {
        self.extension_mut::<RouteRegistry>()?
            .middleware
            .prepend_priority(name.into())?;
        Ok(self)
    }

    /// 将一个 alias 放到路由优先级表尾部；不改变全局 HTTP layer。
    pub fn append_middleware_priority(&mut self, name: impl Into<String>) -> Result<&mut Self> {
        self.extension_mut::<RouteRegistry>()?
            .middleware
            .append_priority(name.into())?;
        Ok(self)
    }
}
