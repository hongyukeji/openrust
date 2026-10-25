# 根 `openrust` package 模块迁移 Spec

> 状态：源码与 workspace 迁移、适用验证及路径文档收敛已完成，并按独立功能簇提交。执行契约见 [ADR-021](../decisions/adr-021-loco-style-module-layout.md)。

## Spec

1. 将已实现框架能力并入根 `openrust` package，保持 `openrust::...` 公共路径、feature 行为、错误语义与生成项目契约。
2. `src/config/`、`src/core/`、`src/db/`、`src/http/`、`src/auth/`、`src/mail/` 是当前有实现的模块；不创建未实现能力空模块。
3. 保留独立 CLI、proc-macro、测试工具和聚合应用 package；其余 capability package 已由根模块吸收或因空壳移除。
4. Rust 配置代码位于 `src/config/mod.rs`；生成项目的 TOML/YAML 仍在项目根 `config/`。
5. 通过文件迁移保留实现，检查模块内引用、facade re-export、可选依赖、宏展开和模板。
6. HTTP 集成测试迁至根 `tests/`；完整示例迁至根 `examples/`；数据库/认证的真实双库验收留在 `apps/server/tests/`，避免 root 对 `openrust-test` 的反向依赖。
7. 根 `tests/` 与 `examples/` 尚未替代 `apps/server` 的启动、feature、插件组合、生成项目及双库职责，故继续保留 `apps/server`。
8. 验收覆盖 workspace metadata、default/关键 feature/all-features、双库集成、生成项目、examples、fmt/clippy/test/doc 与旧路径扫描。

## Workspace/package 盘点与处理

`cargo metadata --no-deps --format-version 1` 当前 workspace 为五个 package：

| Package | 保留理由与职责 |
| --- | --- |
| `openrust`（根目录） | 唯一框架 package；`src/` 下承载能力模块与 facade API。 |
| `openrust-cli`（`crates/openrust-cli`） | 独立 `openrust` 命令行分发物与项目模板。 |
| `openrust-macros`（`crates/openrust-macros`） | Rust proc-macro 必须独立为 `proc-macro` package。 |
| `openrust-test`（`crates/openrust-test`） | 用户 dev-dependency；独立提供 PostgreSQL/MySQL 测试数据库隔离，并单向依赖 `openrust`。 |
| `openrust-server`（`apps/server`） | 非发布的聚合启动与集成验证宿主；在验收逐项被根 tests/examples 替代前保留。 |

原 core/http/db/auth/mail 能力源码已迁至根 `src/`，相应 packages 已移除。此前仅有空 crate root 与规划 README 的 cache/events/queue/storage packages 也已移除；这些能力仍按 M4/M5 规划开发，届时直接实现为根 package 的模块，不预先建空模块。

## 迁移映射

| 原边界 | 根模块/验证位置 |
| --- | --- |
| core 与 Rust config | `src/core/`、`src/config/`，保持 `openrust::core` / `openrust::config` |
| HTTP | `src/http/`；路由/资源集成测试在根 `tests/http_*.rs` |
| database | `src/db/`；PostgreSQL/MySQL 连接、迁移及 auth 集成测试在 `apps/server/tests/` |
| auth | `src/auth/`；功能 WIP 随文件迁移且未改其产品契约，数据库测试在聚合宿主 |
| mail | `src/mail/`；`mail-smtp`、`test-util` 仍为根 package features |
| facade example | `examples/hello.rs` |
| CLI/templates | 保留 `crates/openrust-cli`；生成清单只依赖公开 `openrust` API 与 `openrust-test` dev tool |
| test tool | 保留 `crates/openrust-test`；连接 `openrust` 的数据库 facade，不被根 package 反向依赖 |
| workspace host | 保留 `apps/server` 和 `apps/` 约定，数据库/auth/启动覆盖继续在其独立测试中 |

## 验收记录

- 已验证根 package 名为 `openrust`，workspace 恰有上述五个成员。
- 已验证根 package `openrust` 和五个 workspace 成员；无默认、默认、关键独立 feature、all-features 编译及 workspace default/all-features test/clippy、rustdoc warnings-denied 通过。
- 已在 PostgreSQL 16 与 MySQL 8.4 对迁移后的 auth/database 集成测试实跑通过；两种 `--auth` 生成项目的真实 HTTP/database e2e 通过。
- 根 hello 示例使用隔离端口实跑并返回统一 JSON 200；CI package 路径、双库 Compose、内部 Markdown links/anchors 与旧 package/path 扫描已核对。`apps/server` 保留。
