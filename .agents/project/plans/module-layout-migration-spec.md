# 根 `openrust` package 模块迁移 Spec

> 状态：前置盘点进行中；尚未迁移源码。执行契约见 [ADR-021](../decisions/adr-021-loco-style-module-layout.md)。

## 迁移 Spec

1. 将已实现的框架能力并入仓库根 `openrust` package，保留 `openrust::...` 公共 API、feature、错误语义和生成项目契约。
2. 将 core/config、HTTP、DB、auth、mail 能力分别落入根 `src/` 的实际模块；空壳 cache/events/queue/storage 不迁移成空模块。
3. 保留独立 CLI 与 proc-macro package；评估并保留面向用户的 `openrust-test`，保留 `apps/server` 直至其验收有等价证据替代。
4. 配置 Rust 代码归 `src/config/mod.rs`；生成项目 TOML/YAML 继续位于项目根 `config/`。
5. 迁移以文件重命名为主，分批校验宏展开路径、feature 可选性、依赖环、CLI、模板与 CI 命令。
6. 根 `tests/` 与 `examples/` 逐项接替 server 聚合、feature 组合、生成项目及 PostgreSQL/MySQL 验收后，方可评估移除 `apps/server`。
7. 本 Spec/盘点先行；源文件迁移仍受 ADR-021 的 M3 认证闭环门槛约束，禁止把未提交认证功能作为结构迁移内容提交。
8. 验收须覆盖 metadata 成员理由、default/关键 features/all-features、双库真实测试、生成项目和所有旧路径扫描。

## 当前 workspace 盘点

`cargo metadata --no-deps --format-version=1` 当前得到 14 个成员：

| Package | 当前职责 / feature | 迁移处理 |
| --- | --- | --- |
| `openrust` | facade；`auth`、双数据库、`mail-smtp`、`test-util` | 根 package，吸收已实现框架 modules 并保持对外 facade |
| `openrust-core` | App、Plugin、Command、Config、Error、RateLimiter | `src/core/` 与 `src/config/` |
| `openrust-http` | Axum 路由、提取器、中间件、serve | `src/http/` |
| `openrust-db` | SeaORM、双库 feature、migration/seed、方言、表名、rate limit | `src/db/`，feature 依赖并入 root manifest |
| `openrust-auth` | auth、JWT、Session、refresh、PAT、验证码、Policy | `src/auth/`；迁移时须保留当前认证 WIP 原样 |
| `openrust-mail` | Email、Mailer、log/fake/可选 SMTP | `src/mail/`，保留 `mail-smtp` 与 `test-util` 行为 |
| `openrust-cache`、`openrust-events`、`openrust-queue`、`openrust-storage` | 当前为空壳 package；后续能力已有 ADR/计划 | 删除无实现 package；未来能力直接在 root package 实现，不建空模块 |
| `openrust-cli` | 独立全局命令工具；含生成模板 | 保留 package；模板 facade 依赖继续指向 crates.io `openrust` |
| `openrust-macros` | proc-macro，实现 `#[openrust::resource]` | 保留 package；检查宏展开中 crate path 与 facade re-export |
| `openrust-test` | 用户 dev-dependency；`TestDatabase`、PG/MySQL 隔离 | 暂保留独立包；依赖 root `openrust` DB API/features，不能让 root 反向依赖它 |
| `openrust-server` | `apps/server` 聚合应用与集成验收，非发布 | 暂保留；所有职责经根 tests/examples 等价替代后另行决定 |

## 迁移风险与待核查项

- 当前根 `Cargo.toml` 只有 workspace；`crates/openrust/Cargo.toml` 才是 facade package manifest。迁移需将 package 元数据/feature 并入根 manifest，并使 workspace 成员路径继续覆盖保留工具与应用。
- 当前 `openrust` facade 通过 path dependencies 聚合 `core/http/db/auth/mail/macros`；并入 modules 后必须删除内部能力依赖边，保留可选数据库/SMTP 依赖与公共 re-export。
- `openrust-auth` 的 `db-postgres` / `db-mysql` features 还开启 dev-dependency `openrust-test` 的对应 feature；合并后不得让生产 `openrust` 依赖测试工具。保留 `openrust-test -> openrust` 单向依赖，把需 `TestDatabase` 的双库整合验收放在该测试工具或 `apps/server` 验收中，避免 root package ↔ `openrust-test` 依赖环。
- 当前 workspace 内部依赖 DAG 为 db/http/mail → core，auth → core/db/http，facade 聚合 core/http/db/auth/mail/macros，server 依赖 facade；`openrust-test` 依赖 core/db，auth 仅在测试期依赖它。合并时按模块边界保留单向 Rust module 引用，不保留 crate 间 facade 反向引用。
- `openrust-macros`、CLI 与模板的依赖路径、宏展开路径、CLI manifest 生成段、README/AGENTS/CI/compose/`.cargo/config.toml` 中 package 命令和配置目录是迁移必查项。
- 宏展开硬编码 `::openrust::http::{Resource, ResourceActions}`，依赖 facade 使用端，不依赖能力 crate；保留 macro re-export 并增加外部示例编译覆盖。当前唯一 facade example 在 `crates/openrust/examples/hello.rs`，迁至仓库根 `examples/`。
- `apps/server/tests/{server,auth}.rs`、`openrust-db/tests/*`、`openrust-auth/tests/*`、`openrust-test/tests/*` 和 CLI 快照必须逐项映射到根 `tests/`、根 `examples/` 或保留工具包测试；未映射项禁止删 server。
- 当前 CI 以 package 名执行 PG/MySQL db、TestDatabase 与 server 测试；迁移后逐条映射为 root feature 集成验证、`openrust-test` 数据库工具测试和保留 server 组合测试。根 package 同时成为 workspace root 后需重定 `default-members`，确认裸 `cargo test` 的覆盖范围。
- 目录迁移不能与当前未提交 M3 认证功能提交混合；当前工作树含 102 项并行 WIP，须在实施前重新盘点差异并制定仅路径迁移的隔离方案。
