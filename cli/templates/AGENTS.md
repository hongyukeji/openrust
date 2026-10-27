# 应用开发指南

本项目基于 OpenRust。运行 `openrust dev` 启动开发服务器，运行 `cargo test` 执行测试。

应用代码位于 `src/`；HTTP 处理器放在 `src/controllers/`，路由在 `src/routes.rs` 中注册。

若通过 `openrust new --grpc` 生成了 `proto/` 与 `src/grpc.rs`，可在 `proto/` 维护 Protobuf，并由 `build.rs` 生成 typed Rust 代码。启用 gRPC feature 不会自动监听端口；需在应用中注册服务并安装 `GrpcPlugin`。
