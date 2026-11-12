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

监听器接收通过 HTTP/2 传输的明文 gRPC。面向不可信网络时，由 TLS 代理终止加密连接。
