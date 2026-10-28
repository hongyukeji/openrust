# 应用开发指南

本项目基于 OpenRust，普通 `cargo run` 启动应用，`cargo test` 执行测试。

`src/main.rs` 只负责入口；`bootstrap/app.rs` 装配应用，`routes/web.rs` 注册 HTTP 路由。`app/` 保留通用应用装配，业务放在 `modules/<业务>/`，配置在 `config/`，迁移与种子在 `database/`，资源在 `resources/`，公开文件在 `public/`，运行数据在 `storage/`。

本地依赖通过应用自身 Cargo.toml 的 workspace 配置解析；保持框架源码目录可达。`--grpc` 生成 proto/build.rs、Echo 服务与 `serve all` 联合运行命令；普通 `cargo run` 仍只启动 HTTP。配置 `config/grpc.toml` 的 TLS 证书和私钥后用 `cargo run -- serve all` 启动 HTTP 与 gRPC 两个端口；不配置 TLS 时 gRPC 启动会拒绝，只有本机开发或可信 TLS 终止拓扑才能显式开启明文。

`modules/mod.rs` 静态声明业务代码，并通过 `manifest(root)` 返回采用 `FileModuleActivator` 的公开 `ModuleManifest`；默认从运行工作目录读取 `modules_statuses.json`，与默认 `config/` 加载约定一致；部署时从应用根启动，不能把编译机路径当部署根。`bootstrap/app.rs` 的 `Application::modules` 使用此清单；模块的 `ModuleDescriptor` 引入普通 ServiceProvider，与框架共用唯一容器和 register/boot 生命周期。模块根使用部署时的应用目录；生成的 `Configuration.modules_root` 可显式指定运行根，`Application::modules` 把相同根传给状态文件和各模块 descriptor，不扫描 Rust 源码或在运行时自动加载 crate。启用状态文件默认是空对象 `{}`；只有对应名称的 JSON `true` 启用模块，缺少文件/键、`false` 或其他非 `true` 值均关闭，损坏 JSON 启动失败且不被自动覆写。状态变更只影响下次应用启动，不热卸载当前应用；管理写命令尚未实现。

模块专属配置、迁移、资源与测试留在该模块目录；现有根级生成器产物属于应用级职责，模块生成与资源发布等管理命令仍待后续实现。第三方依赖在本应用 Cargo.toml 管理，不在普通模块下放无效 Cargo.toml。
