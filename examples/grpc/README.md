# gRPC 示例

该独立示例使用应用自有的 `proto/echo.proto` 和 `build.rs` 生成 typed client/server，并通过真实 TCP loopback 完成 unary、服务端流、客户端流和双向流调用。客户端流通过结束输入完成半关闭；所有连接都在本进程中运行，不需要数据库或全局 `protoc`。

```bash
cargo run --locked
```

应用可从 `GrpcClientRegistry` 选择默认或具名共享 `Channel`，服务由 `GrpcPlugin::register_service` 注册。启用 `grpc` feature 不会自动监听端口；示例显式绑定随机 loopback 端口并通过 shutdown future 优雅停止。
