# cli

> 受众：AI Agent
> 更新：2026-10-29
> 摘要：源码内 `openrust` 命令提供中文帮助、项目生成、开发启动与运行期命令代理；生成项目遵循根 workspace 和简体中文说明规范。

## 当前实现

- 应用端口使用 `APP_PORT`（自定义命名空间对应 `<前缀>PORT`），生成 `.env.example` 在 `APP_URL="http://localhost:${APP_PORT}"` 前声明 `APP_PORT=3000`；内部配置字段仍为 `server.port`。

- `openrust new <目标目录>` 从源码 workspace 创建独立应用，支持相对/绝对目标，不修改源 workspace；应用拥有自己的 Cargo.lock，本地 framework/tool 路径明确解析。
- C04-C普通与认证模板均接入CookieServiceProvider和`tests/support/request_cookie.rs`，由正式Provider/Router测试读取、队列、直接响应及删除，使用opaque wire回送；加密缺失仅显式plain选择，不暗降级。普通项目fmt/check/3项测试、认证PG18/MySQL8.4项目各2项及完整CLI20项已实际通过，证据见[testing](../memory/testing.md)。
- `dev`/`serve` 通过 metadata 找到含 bootstrap/app.rs 的当前应用，再从其目录运行普通 cargo run；dev 设置 APP_ENV=development，复用/准备缓存服务，不提供 watch。

- package `cli` 位于仓库 `tools/cli/`，二进制命令 `openrust` 保留；源码交付与应用隔离按 [ADR-039](../decisions/adr-039-source-workspace-distribution.md) 已完成。
- `openrust --help` 及子命令帮助使用简体中文标题、选项说明和帮助/版本说明；命令、参数、配置键和协议值保留技术原名。
- 新应用的 `AGENTS.md`、`.env.example` 注释、模块入口 Rustdoc，以及 Model/Migration/Mailer/Policy 生成 Rustdoc 使用简体中文；模板说明不改变应用邮件等运行时语言策略。

- `openrust generate model` 生成 SeaORM entity 并注册 module；`openrust generate migration` 生成建表迁移并注册到 `Migrator`，支持基础标量、UTC 时间、JSON、nullable 与 references 字段 DSL。两者可一致启用 `--uuid-id`（UUIDv7）和 `--soft-delete`（显式 `Entity::not_deleted()` scope，非全局过滤）。`generate mailer` 生成实现 `Email` 的纯文本类型并注册邮件模块；`generate policy` 生成基于既有 `auth::Policy` 的资源 CRUD 授权包装，要求项目启用 `auth`。
- 其他命令转发为当前项目的 `cargo run --quiet -- <args>`，并保留子进程退出码。
- 默认模板快照覆盖 manifest、主入口、应用、路由、基础配置、测试与 `AGENTS.md`；双库模板单测覆盖 backend feature、migration crate、插件和 seed 文件。数据生成器测试覆盖 entity fields、UTC/JSON columns、references 和多迁移 registry 更新。
- `tests/application.rs` 通过读取生成的 `Cargo.toml` 并与 `CARGO_PKG_NAME` 比较验证包名，避免不同项目名长度改变 rustfmt 排版；PG18 与 MySQL 8.4 的新生成认证项目格式、check 与 HTTP 集成测试均通过。
- 数据库模板以稳定 `openrust_migration` dependency alias 引用独立 migration package，生成应用名长短不会改变 `app.rs` 格式。
- `openrust new` 支持 `--http-client`、`--sms-twilio`、`--encryption` 可选项，可与 `--db` / `--auth` 组合；`openrust key generate` 生成 32-byte OS 随机密钥并输出 Base64，不写配置文件。
- `openrust new --grpc` 生成可选 gRPC 应用：转发 `grpc` feature，创建应用自有 `proto/echo.proto`、vendored-protoc `build.rs`、typed proto module 和 `[grpc] listen_addr` 配置，并安装 Echo、`GrpcServiceProvider` 与 `HttpGrpcServiceProvider`。默认入口仍是 HTTP `serve`；配置 TLS 证书/私钥后可显式运行 `serve all`，以同一应用在两个端口提供 HTTP 与 typed Echo。默认模板不增加 gRPC 依赖或代码生成。
- `--auth-mfa` 生成应用在认证 HTTP 测试内实际启用因子，验证登录仅返回挑战、恢复码一次性使用、轮换与 refresh 撤销。`--grpc` 生成测试启动生成的二进制，验证 TLS 信任/拒绝、HTTP/Echo 与信号停机；普通、双库认证/MFA、纯 gRPC 及双库认证/MFA+gRPC 十变体的格式、check 和适用真实测试见 [R1-E 验收记录](../memory/testing.md#r1-e-业务示例定向验收证据整簇进行中)。
- 生成源码采用内置模板文件与 Rust 字符串拼装，当前未使用 MiniJinja；生成器包含 Model、Migration、Mailer、Policy 和 Job。UUID 主键默认关闭，启用时应用和 migration package 均添加可选 `db-uuid` feature。

- C04-B2在生成的`tests/support/request_input.rs`共用公开Request/Fluent测试入口；普通`test_app`与认证`auth_flows`均调用，认证路径复用真实隔离DB配置和原清理流程。模板根路由从最终应用配置读取`app.name`，不以包名替代；`app.timezone`默认UTC。本簇最终普通与双库生成认证项目均实际通过check/tests，共用helper获独立复审；证据见testing，未以历史快照代替。

## 后续边界

- controller/scaffold/command 等生成器、只读 `doctor`、诊断和开发编排属于 M6 待补项。具体顺序以 [v1 计划](../plans/gen1-plan.md) 与活跃待办为准。
- 出站 HTTP 客户端与生成 feature 的已有验收见 [测试基线](../memory/testing.md)；外部邮件/SMS 投递属于各通信 driver 的单独验收边界。

目录/分发以 [ADR-073](../decisions/adr-073-root-application-and-example-module.md) 和 [architecture](../architecture.md) 为准；独立生成应用不修改源 workspace。根 package 为 openrust library；同一根 package 提供唯一默认 server bin。

后继 R13 补全命令/生成器、doctor、watch 和编排；当前 generate API 暂保留，未宣称 Artisan 全命令完成。
