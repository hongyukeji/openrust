# openrust-cli

> 受众：AI Agent
> 更新：2026-10-26
> 摘要：全局 `openrust` 命令提供项目生成、开发启动与运行期命令代理。

## 当前实现

- 独立 package `openrust-cli` 位于仓库 `cli/`，模板位于 `cli/templates/`，保留 package 名、`openrust` 二进制名和 CLI 分发边界。

- `openrust new <name-or-path>` 生成无数据库应用骨架；`--db postgres|mysql` 增加 backend feature、数据库插件、migration crate、seed 入口与 `.env.example`。`--auth` 增加 JWT/Auth/AuthFlows plugins、账户/refresh/verification/Policy/限流 migrations、身份示例及注册、验证、登录、refresh、logout、密码重置 routes。
- `openrust dev` 以 `APP_ENV=development` 启动当前项目的 `serve` 命令；`openrust serve` 不改环境变量。两者当前只代理一次 `cargo run`，没有文件监听重启、基础服务编排、就绪等待或自动迁移。
- `openrust generate model` 生成 SeaORM entity 并注册 module；`openrust generate migration` 生成建表迁移并注册到 `Migrator`，支持基础标量、UTC 时间、JSON、nullable 与 references 字段 DSL。两者可一致启用 `--uuid-id`（UUIDv7）和 `--soft-delete`（显式 `Entity::not_deleted()` scope，非全局过滤）。`generate mailer` 生成实现 `Email` 的纯文本类型并注册邮件模块；`generate policy` 生成基于既有 `auth::Policy` 的资源 CRUD 授权包装，要求项目启用 `auth`。
- 其他命令转发为当前项目的 `cargo run --quiet -- <args>`，并保留子进程退出码。
- 默认模板快照覆盖 manifest、主入口、应用、路由、基础配置、测试与 `AGENTS.md`；双库模板单测覆盖 backend feature、migration crate、插件和 seed 文件。数据生成器测试覆盖 entity fields、UTC/JSON columns、references 和多迁移 registry 更新。
- `tests/application.rs` 通过读取生成的 `Cargo.toml` 并与 `CARGO_PKG_NAME` 比较验证包名，避免不同项目名长度改变 rustfmt 排版；PG18 与 MySQL 8.4 的新生成认证项目格式、check 与 HTTP 集成测试均通过。
- 数据库模板以稳定 `openrust_migration` dependency alias 引用独立 migration package，生成应用名长短不会改变 `app.rs` 格式。
- `openrust new` 支持 `--http-client`、`--sms-twilio`、`--encryption` 可选项，可与 `--db` / `--auth` 组合；`openrust key generate` 生成 32-byte OS 随机密钥并输出 Base64，不写配置文件。
- 生成源码采用内置模板文件与 Rust 字符串拼装，当前未使用 MiniJinja；生成器包含 Model、Migration、Mailer 和 Policy。UUID 主键默认关闭，启用时应用和 migration package 均添加可选 `db-uuid` feature。

## 后续边界

- `generate job` 属于 M4，尚未实现；controller/scaffold/command 等生成器、只读 `doctor`、诊断和开发编排属于 M6 待补项。具体顺序以 [v1 计划](../plans/gen1-plan.md) 与活跃待办为准。
- M6 补齐 controller/scaffold/command 等剩余生成器、只读 `doctor`、诊断和开发编排；已有 runtime 命令转发不等于 CLI 已提供这些入口。具体顺序以 [v1 计划](../plans/gen1-plan.md) 与活跃待办为准。
- 模板声明 workspace 对应版本；当前包版本为 `0.0.0`，不能从版本字符串推断已经发布。仓库内验收未发布源码时使用临时 Cargo patch；v1 正式安装验收须走不依赖本地 patch 的路径。
- 出站 HTTP 客户端与生成 feature 的已有验收见 [测试基线](../memory/testing.md)；外部邮件/SMS 投递属于各通信 driver 的单独验收边界。
