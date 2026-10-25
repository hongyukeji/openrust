# openrust-cli

> 受众：AI Agent
> 更新：2026-10-25
> 摘要：全局 `openrust` 命令提供项目生成、开发启动与运行期命令代理。

## 当前实现

- `openrust new <name-or-path>` 生成无数据库应用骨架；`--db postgres|mysql` 增加 backend feature、数据库插件、migration crate、seed 入口与 `.env.example`。`--auth` 增加 JWT/Auth/AuthFlows plugins、账户/refresh/verification/Policy/限流 migrations、身份示例及注册、验证、登录、refresh、logout、密码重置 routes。
- `openrust dev` 以 `APP_ENV=development` 启动当前项目的 `serve` 命令；`openrust serve` 不改环境变量。
- `openrust generate model` 生成 SeaORM entity 并注册 module；`openrust generate migration` 生成建表迁移并注册到 `Migrator`，支持基础标量、UTC 时间、JSON、nullable 与 references 字段 DSL。
- 其他命令转发为当前项目的 `cargo run --quiet -- <args>`，并保留子进程退出码。
- 默认模板快照覆盖 manifest、主入口、应用、路由、基础配置、测试与 `AGENTS.md`；双库模板单测覆盖 backend feature、migration crate、插件和 seed 文件。数据生成器测试覆盖 entity fields、UTC/JSON columns、references 和多迁移 registry 更新。
- 数据库模板以稳定 `openrust_migration` dependency alias 引用独立 migration package，生成应用名长短不会改变 `app.rs` 格式。

## 后续边界

- `generate scaffold` 等非数据生成器属于 M6。
- M1 模板依赖已发布的 `openrust` 版本；仓库内验收未发布源码时通过 Cargo 临时 patch 到本地门面 crate。
