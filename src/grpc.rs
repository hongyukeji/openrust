//! gRPC server 与客户端入口，基于 tonic / Protobuf。
//!
//! 应用通过 [`GrpcPlugin`] 显式启用 gRPC；注册服务不会启动监听器，需运行 `grpc serve`。

use std::{
    collections::{BTreeMap, HashSet},
    convert::Infallible,
    future::Future,
    net::SocketAddr,
    sync::Arc,
};

use crate::{
    App, AppBuilder, AppState, BoxFuture, Command, Error, Result,
    config::GrpcConfig,
    core::{Plugin, shutdown_signal},
};
use http::Request;
use tokio::net::TcpListener;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::{
    body::Body,
    server::NamedService,
    service::{Routes, RoutesBuilder},
    transport::{Channel, Endpoint, server::Router},
};
use tower::Service;

/// tonic / prost crate re-export；生成代码可通过 facade 访问运行时类型。
pub use prost;
/// tonic runtime re-export。
pub use tonic;
/// tonic-prost codec re-export，供生成代码的模块引用。
pub use tonic_prost;

type ServiceRegistration = Box<dyn Fn(&AppState, &mut RoutesBuilder) + Send + Sync>;

/// 由 [`GrpcPlugin::register_service`] 收集的应用服务工厂。
#[derive(Default)]
pub struct GrpcServiceRegistry {
    names: HashSet<&'static str>,
    factories: Vec<ServiceRegistration>,
}

impl GrpcServiceRegistry {
    fn register<S, F>(&mut self, factory: F) -> Result<()>
    where
        S: Service<Request<Body>, Error = Infallible>
            + NamedService
            + Clone
            + Send
            + Sync
            + 'static,
        S::Response: axum::response::IntoResponse,
        S::Future: Send + 'static,
        F: Fn(AppState) -> S + Send + Sync + 'static,
    {
        if !self.names.insert(S::NAME) {
            return Err(Error::custom(
                "grpc_service_duplicate",
                "同一 gRPC 服务不能重复注册",
            ));
        }
        self.factories.push(Box::new(move |state, routes| {
            routes.add_service(factory(state.clone()));
        }));
        Ok(())
    }

    fn routes(&self, state: &AppState) -> Result<Routes> {
        if self.factories.is_empty() {
            return Err(Error::custom(
                "grpc_service_missing",
                "没有注册 gRPC 服务；请在应用构建时调用 GrpcPlugin::register_service",
            ));
        }
        let mut routes = RoutesBuilder::default();
        for factory in &self.factories {
            factory(state, &mut routes);
        }
        Ok(routes.routes())
    }
}

/// gRPC typed client 的共享 channel 注册表。
#[derive(Clone, Default)]
pub struct GrpcClientRegistry {
    default_client: Option<String>,
    channels: Arc<BTreeMap<String, Channel>>,
}

impl GrpcClientRegistry {
    fn from_config(config: &GrpcConfig) -> Result<Self> {
        let mut channels = BTreeMap::new();
        for (name, uri) in &config.clients {
            let endpoint = Endpoint::from_shared(uri.clone()).map_err(|_| {
                Error::custom(
                    "grpc_endpoint_invalid",
                    format!("grpc.clients.{name} 不是有效的 endpoint URI"),
                )
            })?;
            if endpoint.uri().scheme_str() != Some("http") {
                return Err(Error::custom(
                    "grpc_endpoint_scheme_unsupported",
                    format!("grpc.clients.{name} 在 G1 仅支持 http:// endpoint；TLS 支持见 G2"),
                ));
            }
            channels.insert(name.clone(), endpoint.connect_lazy());
        }
        Ok(Self {
            default_client: config.default_client.clone(),
            channels: Arc::new(channels),
        })
    }

    /// 选择默认或具名客户端，并返回可直接传给 tonic 生成 client 的共享 Channel。
    pub fn channel(&self, name: Option<&str>) -> Result<Channel> {
        let name = name.or(self.default_client.as_deref()).ok_or_else(|| {
            Error::custom(
                "grpc_default_client_missing",
                "未配置默认 gRPC 客户端；请提供具名客户端名称",
            )
        })?;
        self.channels.get(name).cloned().ok_or_else(|| {
            Error::custom(
                "grpc_client_not_found",
                format!("未配置 gRPC 客户端 `{name}`"),
            )
        })
    }

    /// 将测试或嵌入式应用提供的 Channel 替换/注册到指定名称。
    pub fn with_channel(mut self, name: impl Into<String>, channel: Channel) -> Result<Self> {
        let name = name.into();
        if name.is_empty() {
            return Err(Error::custom(
                "grpc_client_name_invalid",
                "gRPC 客户端名称不能为空",
            ));
        }
        let mut channels = Arc::unwrap_or_clone(self.channels);
        channels.insert(name, channel);
        self.channels = Arc::new(channels);
        Ok(self)
    }
}

/// 绑定到已组装应用的 gRPC server 路由。
pub struct GrpcServer {
    listen_addr: SocketAddr,
    routes: Routes,
}

impl GrpcServer {
    /// 从服务注册表和配置构建 server；不绑定端口或启动后台任务。
    pub fn from_app(app: &App) -> Result<Self> {
        let registry = app.extension::<GrpcServiceRegistry>().ok_or_else(|| {
            Error::custom(
                "grpc_service_registry_missing",
                "GrpcPlugin 未注册到当前应用",
            )
        })?;
        Ok(Self {
            listen_addr: app.config().grpc.listen_addr,
            routes: registry.routes(app.state())?,
        })
    }

    /// 返回配置的 listener 地址。
    pub fn listen_addr(&self) -> SocketAddr {
        self.listen_addr
    }

    /// 在调用方绑定的 TCP listener 上服务，直到 shutdown future 完成。
    pub async fn serve<F>(self, listener: TcpListener, shutdown: F) -> Result<()>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let mut server = tonic::transport::Server::builder();
        let router: Router = server.add_routes(self.routes);
        router
            .serve_with_incoming_shutdown(TcpListenerStream::new(listener), shutdown)
            .await
            .map_err(Error::internal)
    }
}

/// 显式注册 gRPC 命令、客户端 channel 和应用服务。
#[derive(Default)]
pub struct GrpcPlugin {
    clients: Option<GrpcClientRegistry>,
}

impl GrpcPlugin {
    /// 创建配置驱动的 gRPC 插件。
    pub fn new() -> Self {
        Self::default()
    }

    /// 使用外部构造的 channel registry，便于测试替换或应用托管连接。
    #[must_use]
    pub fn with_clients(mut self, clients: GrpcClientRegistry) -> Self {
        self.clients = Some(clients);
        self
    }

    /// 在应用构建时注册一个 tonic 生成的 server 工厂。
    ///
    /// 工厂在 `grpc serve` 构造路由时接收共享的 [`AppState`]；注册本身不会启动 listener。
    pub fn register_service<S, F>(builder: &mut AppBuilder, factory: F) -> Result<()>
    where
        S: Service<Request<Body>, Error = Infallible>
            + NamedService
            + Clone
            + Send
            + Sync
            + 'static,
        S::Response: axum::response::IntoResponse,
        S::Future: Send + 'static,
        F: Fn(AppState) -> S + Send + Sync + 'static,
    {
        builder
            .extension_mut::<GrpcServiceRegistry>()
            .register(factory)
    }
}

impl Plugin for GrpcPlugin {
    fn build(&self, builder: &mut AppBuilder) -> Result<()> {
        let config = &builder.config().grpc;
        config.validate()?;
        let clients = match &self.clients {
            Some(clients) => clients.clone(),
            None => GrpcClientRegistry::from_config(config)?,
        };
        builder.extension_mut::<GrpcServiceRegistry>();
        builder.try_insert(clients)?;
        builder.try_command(GrpcServeCommand)?;
        Ok(())
    }
}

struct GrpcServeCommand;

impl Command for GrpcServeCommand {
    fn name(&self) -> &'static str {
        "grpc serve"
    }

    fn about(&self) -> &'static str {
        "启动 gRPC 服务器"
    }

    fn run<'a>(&'a self, app: &'a App, _args: Vec<String>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let server = GrpcServer::from_app(app)?;
            let listener = TcpListener::bind(server.listen_addr()).await?;
            let address = listener.local_addr()?;
            tracing::info!(%address, "gRPC listener ready");
            server.serve(listener, shutdown_signal()).await
        })
    }
}

/// 将生成模块与 tonic runtime 版本保持一致。
pub mod codegen {
    pub use tonic::codegen::*;
}

#[cfg(test)]
mod tests {
    use super::{GrpcClientRegistry, GrpcConfig};
    use std::collections::BTreeMap;

    #[tokio::test]
    async fn configured_clients_share_lazy_channels_and_validate_endpoint_scheme()
    -> crate::Result<()> {
        let config = GrpcConfig {
            clients: BTreeMap::from([
                ("default".to_owned(), "http://127.0.0.1:50051".to_owned()),
                ("billing".to_owned(), "http://127.0.0.1:50052".to_owned()),
            ]),
            default_client: Some("default".to_owned()),
            ..GrpcConfig::default()
        };
        let clients = GrpcClientRegistry::from_config(&config)?;
        assert!(clients.channel(None).is_ok());
        assert!(clients.channel(Some("billing")).is_ok());
        assert_eq!(
            clients
                .channel(Some("missing"))
                .err()
                .map(|error| error.code().to_owned()),
            Some("grpc_client_not_found".to_owned())
        );
        let tls = GrpcConfig {
            clients: BTreeMap::from([("secure".to_owned(), "https://example.test".to_owned())]),
            ..GrpcConfig::default()
        };
        assert_eq!(
            GrpcClientRegistry::from_config(&tls)
                .err()
                .map(|error| error.code().to_owned()),
            Some("grpc_endpoint_scheme_unsupported".to_owned())
        );
        Ok(())
    }
}
