# ADR-021 主框架包与 Loco 风格的能力模块布局

- 状态：Accepted（目标架构已确认；迁移待 M3 认证功能簇闭环后实施）
- 日期：2026-10-25
- 参考：[Loco 仓库结构](https://github.com/loco-rs/loco)
- 替代：[ADR-019](adr-019-neutral-crate-directories-and-app-package.md) 的“逐能力 crate 目录迁移”与 [ADR-020](adr-020-workspace-layout-and-loco-practices.md) 的“保持当前多能力 package 布局”部分；保留其配置文件与 Rust 配置模块分离原则。

## 背景

OpenRust 当前 workspace 有 14 个 package：一个门面、多个按能力拆出的库、CLI、宏、测试工具和聚合 server。当前四个能力 crate（cache、events、queue、storage）只有空壳；其他能力共享版本、发布节奏、扩展模型和应用入口。按每种能力设置独立 Cargo package 增加 manifest、依赖边界、文档与接入维护成本，却没有为用户提供相应的独立版本或发布价值。用户希望能力仍按清晰目录组织，但不需要把每个模块都提升为 Cargo package。

Loco 的框架主体是一个 Cargo package，在 `src/` 下按功能组织 Rust 模块；它把必要的生成器/维护工具作为额外 package，并把示例应用与测试单独组织。OpenRust 应借鉴这个 package 与 module 的分工，同时保留 Rust 中确有必要的 proc-macro 和工具边界。

## 决策

- OpenRust 的核心框架收敛为主 Cargo package `openrust`，其实现放在仓库根 `src/`，能力按 `src/core/`、`src/http/`、`src/auth/`、`src/db/`、`src/cache/`、`src/queue/`、`src/events/`、`src/storage/`、`src/mail/` 等职责组织为 Rust modules。具体模块名称以公共 API contract 和现有代码评估为准，不为目录对称创建空模块。
- 仓库顶层目录职责明确：根 `src/` 只放 OpenRust 框架代码；`apps/` 放同一仓库/workspace 内开发和集成的应用 package，作为应用目录约定保留；`examples/` 放可复用的示例应用；根 `tests/` 验证框架公共 API。通过 `openrust new` 创建的独立用户项目有自己的项目根 `src/`、`config/`，通常不属于框架仓库的 `apps/`。
- 主框架包内使用 `pub mod` / 私有子模块与明确的 facade/re-export 控制公共 API；内部能力间仍须遵循依赖方向和 feature 边界，不因合并 package 而把实现全部塞进 `lib.rs` 或形成不可分辨的全局模块。
- 只在技术或分发边界真实需要时保留单独 package：至少 `openrust-cli`（独立命令行工具）和 `openrust-macros`（proc-macro crate）；`openrust-test` 是否保留为独立测试工具包，在实施 Spec 中核实它的用户依赖、依赖图和测试编译成本。迁移前先确认，没有独立需求的能力 crate 不再单独发布。
- 根 `tests/` 承担主框架公共 API 的集成测试；crate/unit tests 放在对应源文件或 package 的 `tests/`。根 `examples/` 放可运行的完整示例应用（如 blog/demo），承担面向用户的示范和端到端 smoke test。测试或示例不能仅为复刻旧目录而复制一套 server harness。
- 当前 `apps/server` 是框架自己的聚合启动/集成验证宿主，不等同于用户业务应用。只有在根 `tests/` 与 `examples/` 覆盖它的启动、默认 feature、插件组合、生成项目、数据库双库等验收后，才可移除这个具体 package；这不改变 `apps/` 作为仓库内应用目录约定。不得先删除现有宿主再补测试。框架仓库本身是库，不要求根 `cargo run` 启动一个伪产品应用；开发启动示例使用明示的 example 命令，生成的用户应用仍在其 `src/main.rs` 统一启动。
- 用户生成项目保持 `src/main.rs`、`src/app.rs`、`src/config.rs` 的 Rust 代码布局；运行时配置文件继续放在项目根 `config/`，不放入编译源码目录。`src/config.rs` 是配置加载/类型代码，不是 TOML/YAML 文件目录。
- M3 认证功能簇结束前不搬动它当前的代码 WIP。其闭环后按独立架构迁移功能簇执行，保持发布包版本、feature 行为、公共 `openrust::...` 导入、生成项目和双库/集群验收一致。

## 影响与迁移

- 迁移前写独立 5–15 行 Spec，盘点 package/features、依赖环、宏 crate、测试工具、模板、生成项目、CI、文档与所有硬编码路径；设计从 crates.io 用户角度确认依赖仍以单一 `openrust` 门面为主。
- 确认迁移后的根 `src/` 属于框架 package，`apps/` 仍可承载同 workspace 的用户/集成应用；不把二者合并为一个目录，也不因移除 `apps/server` 而取消 `apps/` 约定。
- 一次性合并当前能力源码与 manifest 配置，逐个保持 feature 启停、可选依赖、公开 re-export 和错误行为；按 `git mv` 保留历史可追踪性。未发布的 `openrust-*` capability packages 可在迁移中移除；CLI/macros 等保留 package 的发布名不变。
- 更新根 workspace/package manifest、所有保留工具 crate 的依赖、CI、命令、模板、README、ADR 与 `.agents/project/core/` 事实。移除 package 前先确认没有外部用户依赖；当前版本尚未发布，不需要维护旧 package 的运行时兼容层。
- 若某能力后来确有独立发布、编译隔离或扩展生态需求，可在证据充分后从模块提取为 package；不预先拆分。

## 验收

- `cargo metadata` 显示主框架实现为根 `openrust` package，剩余 package 各有明确技术/分发理由；不存在无用途的能力空壳 crate。
- 主框架公共路径保持 `openrust::...` 稳定；`default`、各可选 feature 与 `--all-features` 的依赖启停行为均有测试。
- 根 `cargo test --workspace` 覆盖模块单元测试、公共 API 集成测试、数据库双库行为、示例与生成项目；旧 `apps/server` 的每项验收都有明确的新位置和通过证据后才能移除。
- 架构文档明确区分“框架源码 `src/`”与“仓库内应用 `apps/`”；即使没有 `apps/server`，`apps/` 的用途仍然清楚。
- 按 AGENTS.md 对源码改动完成 fmt、clippy、workspace tests、doc、双库和生成项目验收；用 `rg` 清查旧 crate/import/路径及所有文档引用。
