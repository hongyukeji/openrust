# 应用开发指南

本项目基于 OpenRust，普通 `cargo run` 启动应用，`cargo test` 执行测试。

`src/main.rs` 只负责入口；`bootstrap/app.rs` 装配应用，`routes/web.rs` 注册 HTTP 路由。业务放在 `app/`，配置在 `config/`，迁移与种子在 `database/`，资源在 `resources/`，公开文件在 `public/`，运行数据在 `storage/`。

本地依赖通过应用自身 Cargo.toml 的 workspace 配置解析；保持框架源码目录可达。gRPC 的 proto/build.rs 是独立编译期流程，启用 feature 不自动监听端口。
