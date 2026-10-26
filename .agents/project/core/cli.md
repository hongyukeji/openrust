# cli

> 受众：AI Agent
> 更新：2026-10-26
> 摘要：源码内 `openrust` 命令提供中文帮助、项目生成、开发启动与运行期命令代理；生成项目遵循根 workspace 和简体中文说明规范。

## 当前实现

- package `cli` 位于仓库 `cli/`，二进制命令 `openrust` 保留；源码交付与应用隔离按 [ADR-039](../decisions/adr-039-source-workspace-distribution.md) 已完成。
- `openrust --help` 及子命令帮助使用简体中文标题、选项说明和帮助/版本说明；命令、参数、配置键和协议值保留技术原名。
- 新应用的 `AGENTS.md`、`.env.example` 注释、模块入口 Rustdoc，以及 Model/Migration/Mailer/Policy 生成 Rustdoc 使用简体中文；模板说明不改变应用邮件等运行时语言策略。

- `openrust new <name>` 或 `openrust new apps/<name>` 在已识别的源码 workspace 中生成应用，并把应用及可选 migration 加为 workspace member；绝对/外部路径、同名 package、已有目录和非法多级目标会在写入前拒绝。`--db postgres|mysql` 增加 backend feature、数据库插件、migration crate、seed 入口与 `.env.example`。`--auth` 增加 JWT/Auth/AuthFlows plugins、账户/refresh/verification/Policy/限流 migrations、身份示例及注册、验证、登录、refresh、logout、密码重置 routes。
- `openrust dev` 以 `APP_ENV=development` 启动当前应用的 `serve` 命令；`openrust serve` 不改环境变量。命令通过 Cargo metadata 定位 `apps/<name>`，显式运行该 package 的同名 binary，并从该应用目录加载配置。两者当前只代理一次 `cargo run`，没有文件监听重启、基础服务编排、就绪等待或自动迁移。
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
- ADR-039 的包名、workspace/path 依赖、`publish = false`、双应用注册与启动/配置隔离已实现；在干净源码副本中默认及 PG/MySQL auth+encryption 应用 check/test、双库 HTTP auth 流程通过。PostgreSQL/MySQL workspace CI 集成目标均以真实服务执行通过。历史 registry patch 记录只表示旧验收方式，不代表新 contract。
- 出站 HTTP 客户端与生成 feature 的已有验收见 [测试基线](../memory/testing.md)；外部邮件/SMS 投递属于各通信 driver 的单独验收边界。
