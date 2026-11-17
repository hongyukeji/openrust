# gRPC

## 安装

在应用源码目录运行安装命令：

```sh
./artisan install:grpc
```

安装器创建 `routes/grpc.rs`，并通过 `bootstrap/app.rs` 中的 `withGrpc` 接入注册函数。重复执行会保留现有路由文件。注册服务后重新构建应用。

## 注册服务

在 `routes/grpc.rs` 中，将生成的 Tonic 服务加入传入的 `RoutesBuilder`。注册函数接收应用实例，可以从中解析服务。OpenRust 在 Provider 和 HTTP 路由装配后执行一次注册函数。

```rust
use openrust::{foundation::Application, grpc::RoutesBuilder, support::ProviderResult};

pub(crate) fn register(_app: &Application, services: &mut RoutesBuilder) -> ProviderResult {
    let (_, health) = tonic_health::server::health_reporter();
    services.add_service(health);
    Ok(())
}
```

此示例需要在应用中添加 `tonic-health` 依赖。

## 运行监听器

启动应用前配置 `GRPC_HOST` 和 `GRPC_PORT`。主机为空时禁用监听。HTTP 和 gRPC 端口绑定后才报告就绪，绑定失败会终止启动，两者共用优雅关停流程。Artisan 引导应用时无需绑定 gRPC 端口。

监听器默认使用明文 HTTP/2。可以配置服务端凭据直接提供 TLS，也可以由反向代理终止 TLS。

## 命名客户端 {#named-clients}

在 `config/grpc.rs` 返回的 `clients` 对象中声明出站连接。每个客户端名称对应当前应用共享的惰性 Tonic 连接，首次 RPC 时建立连接。

```rust
"clients": {
    "health": {
        "host": "127.0.0.1",
        "port": 50051,
        "connect_timeout_ms": 1000,
        "timeout_ms": 2000,
    },
    "billing": {"endpoint": "http://billing.example.test:50051"},
},
```

在 HTTP 处理器和 Artisan 命令中使用 `Grpc::connect("health")`，也可以从应用容器解析 `GrpcClients`。把返回的服务传给生成的 Tonic 客户端，例如 `HealthClient::new(Grpc::connect("health")?)`。缺失客户端和错误地址会在解析时返回错误，网络故障由 RPC 返回。

## 凭据与拦截器 {#credentials-and-interceptors}

在应用启动期间注册 Tonic 凭据和拦截器组，确保在首次解析客户端或启动监听器之前完成。在客户端配置中填写组名。

```rust
use openrust::grpc::{Grpc, tonic::transport::ClientTlsConfig};

Grpc::clients()?.register_credentials(
    "internal-tls",
    ClientTlsConfig::new().ca_certificate(internal_ca).domain_name("health.example.test"),
);
Grpc::clients()?.register_interceptor("request-id", |mut request| {
    request.metadata_mut().insert("x-request-id", "grpc-probe".parse().unwrap());
    Ok(request)
});
```

```json
{"host":"health.example.test","port":50051,"credentials":"internal-tls","interceptors":["request-id"]}
```

配置 TLS 凭据组后，使用主机和端口的客户端会采用 HTTPS。显式 HTTPS 端点要求已注册的凭据组。凭据组未知或端点协议与凭据不符时返回配置错误。

客户端按配置顺序执行拦截器组，组内按注册顺序执行。未注册的拦截器组会跳过。使用 `Grpc::server()?.add_interceptor(...)` 为所有已注册服务添加服务端拦截器。拦截器返回 Tonic 状态时停止执行，并将状态返回给调用方。

通过 `Grpc::server()?.set_credentials(ServerTlsConfig::new().identity(server_identity))` 启用服务端 TLS。若要求双向 TLS，在服务端添加 `client_ca_root(client_ca)`，并在客户端凭据中添加 `identity(client_identity)`。
