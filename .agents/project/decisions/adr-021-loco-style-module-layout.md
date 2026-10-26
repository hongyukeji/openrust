# ADR-021 主框架模块、工具包与目录职责

- 状态：Accepted（根框架模块迁移已完成；中性工具目录与分组示例/测试在工作区完成迁移、验收中；宿主暂保留）
- 日期：2026-10-25
- 参考：按 [六框架对标规则](../plans/framework-design.md#框架参考与对标) 综合评估；本决策最初取材于 [Loco 仓库结构](https://github.com/loco-rs/loco)。
- 替代：[ADR-019](adr-019-neutral-crate-directories-and-app-package.md)、[ADR-020](adr-020-workspace-layout-and-loco-practices.md) 的执行方案；吸收中性目录、配置/源码分离和示例独立组织原则。ADR-007 的宿主验证职责在被等价接替前继续有效。

## 背景

初始 workspace 将多数能力拆成独立 package，并包含 cache/events/queue/storage 空壳。它们共享版本、发布节奏、扩展模型和入口，单独维护 manifest 没有相应收益。主框架现已收敛至根 package；用户进一步要求清理顶层 `crates/`、按项目/能力组织 examples/tests，并尽快评估 `apps/server` 职责。目录简化仍须保留 Rust 技术边界和验收覆盖。

Loco 的框架主体是一个 Cargo package，在 `src/` 下按功能组织 Rust 模块；它把必要的生成器/维护工具作为额外 package，并把示例应用与测试单独组织。OpenRust 应借鉴这个 package 与 module 的分工，同时保留 Rust 中确有必要的 proc-macro 和工具边界。

## 决策

- OpenRust 的核心框架收敛为主 Cargo package `openrust`，其实现放在仓库根 `src/`，能力按 `src/core/`、`src/http/`、`src/auth/`、`src/db/`、`src/cache/`、`src/queue/`、`src/events/`、`src/storage/`、`src/mail/` 等职责组织为 Rust modules。具体模块名称以公共 API contract 和现有代码评估为准，不为目录对称创建空模块。
- 仓库顶层目录职责明确：根 `src/` 只放 OpenRust 框架代码；`apps/` 放同一仓库/workspace 内开发和集成的应用 package，作为应用目录约定保留；`examples/` 放可复用的示例应用；根 `tests/` 验证框架公共 API。通过 `openrust new` 创建的独立用户项目有自己的项目根 `src/`、`config/`，通常不属于框架仓库的 `apps/`。
- 主框架包内使用 `pub mod` / 私有子模块与明确的 facade/re-export 控制公共 API；内部能力间仍须遵循依赖方向和 feature 边界，不因合并 package 而把实现全部塞进 `lib.rs` 或形成不可分辨的全局模块。
- 保留 `openrust-cli`（独立二进制与模板）、`openrust-macros`（proc-macro 技术边界）、`openrust-test`（用户 dev-dependency、单向依赖框架）。不为消除 `crates/` 目录而把 proc-macro 塞入普通库、让生产库反向依赖测试包或复制实现。三个包的发布名不变，目标目录采用 `cli/`、`macros/`、`testing/`；其余能力使用根模块。
- 根 `tests/` 承担主框架公共 API 的集成测试；crate/unit tests 放在对应源文件或 package 的 `tests/`。根 `examples/` 放可运行的完整示例应用（如 blog/demo），承担面向用户的示范和端到端 smoke test。测试或示例不能仅为复刻旧目录而复制一套 server harness。
- 当前 `apps/server` 是框架自己的聚合启动/集成验证宿主，不等同于用户业务应用。只有在根 `tests/` 与 `examples/` 覆盖它的启动、默认 feature、插件组合、生成项目、数据库双库等验收后，才可移除这个具体 package；这不改变 `apps/` 作为仓库内应用目录约定。不得先删除现有宿主再补测试。框架仓库本身是库，不要求根 `cargo run` 启动一个伪产品应用；开发启动示例使用明示的 example 命令，生成的用户应用仍在其 `src/main.rs` 统一启动。
- 框架与用户生成项目的 Rust 配置逻辑都采用 `src/config/` 模块目录（以 `src/config/mod.rs` 为入口，按真实复杂度拆分子模块）；运行时 TOML/YAML 配置文件继续放在项目根 `config/`，不放入编译源码目录。两处 `config/` 含义不同：源码目录放 Rust 代码，项目根目录放可部署配置文件。
- 目录收口作为独立底层功能簇执行：先保护并安全闭环正在开发的 WIP，再校准路径和测试入口；不得将已完成的根模块迁移重复列为新任务，也不得把目录变化等同于功能验收。

## 影响与迁移

- 迁移前写独立 5–15 行 Spec，盘点 package/features、依赖环、宏 crate、测试工具、模板、生成项目、CI、文档与所有硬编码路径；设计从 crates.io 用户角度确认依赖仍以单一 `openrust` 门面为主。
- 确认迁移后的根 `src/` 属于框架 package，`apps/` 仍可承载同 workspace 的用户/集成应用；不把二者合并为一个目录，也不因移除 `apps/server` 而取消 `apps/` 约定。
- 一次性合并当前能力源码与 manifest 配置，逐个保持 feature 启停、可选依赖、公开 re-export 和错误行为；按 `git mv` 保留历史可追踪性。未发布的 `openrust-*` capability packages 可在迁移中移除；CLI/macros 等保留 package 的发布名不变。
- 更新根 workspace/package manifest、所有保留工具 crate 的依赖、CI、命令、模板、README、ADR 与 `.agents/project/core/` 事实。移除 package 前先确认没有外部用户依赖；当前版本尚未发布，不需要维护旧 package 的运行时兼容层。
- 若某能力后来确有独立发布、编译隔离或扩展生态需求，可在证据充分后从模块提取为 package；不预先拆分。

## 当前迁移实施

- 根目录现在同时是 `openrust` package 与 workspace root；成员为 `openrust`、`openrust-cli`、`openrust-macros`、`openrust-test`、`openrust-server`。各保留理由见 [迁移 Spec](../plans/module-layout-migration-spec.md)。
- 已迁入 core/config、HTTP、DB、auth、mail 源码；HTTP 测试在根 `tests/`，完整 hello 示例在根 `examples/`，真实 DB/auth 测试在仍保留的 `apps/server/tests/`。
- 所有 capability packages 均未发布；模块迁移后旧 package manifests 和空壳已移除。`openrust-test` 保持单向依赖，未造成根 package 反向依赖。
- `apps/server` 暂不移除，因它仍承担聚合启动、feature 组合、生成项目与双库验收。

## 后续目录收口

- 当前工作区已迁至 `cli/`、`macros/`、`testing/`，示例为 `examples/hello/main.rs`，根测试为 `tests/http/*.rs` 并显式保留 targets。结构簇验收进行中，完成证据与提交见迁移 Spec/Git，不提前宣称全部门禁通过。
- 推荐用 `git mv` 将工具包迁到 `cli/`、`macros/`、`testing/`，同步 workspace members/default-members、path dependencies、模板 patch/快照、include 路径、CI/脚本和文档。包身份与公共 Rust 路径不变；确认无剩余源码/引用后再移除顶层 `crates/`。
- 示例按用途分组，例如 `examples/hello/main.rs`；完整 `examples/demo/` 或 `examples/blog/` 按实际验收需求创建，不预建空项目。多文件 Cargo example 可使用标准 `main.rs` 布局或显式 `[[example]]`；独立应用需明确 workspace/exclude 与构建命令。
- 测试按能力放入 `tests/<capability>/`，以 `[[test]] path = "..."` 或薄顶层 harness 的 `mod` 引用保证被发现。迁移前后对照 Cargo targets、测试清单、required-features、双库环境条件和实际执行数，避免只有目录整齐而测试不再运行。
- 先审计 `apps/server` 的默认启动、已装配插件、测试专用 App、双库迁移/认证/队列、独立进程信号/崩溃恢复与生成项目职责；逐项记录接替入口和验收。根测试、examples、生成物测试具备等价覆盖后，再决定移除该 package 或迁到更合适位置；保留 `apps/` 作为应用目录约定。
- 若宿主被移除，同步调整根默认启动说明、二进制路径/测试环境、配置查找和 Agent 完成定义。不能只把 `apps/server` 改名为 example 后宣称职责已经整合。

## 验收

- `cargo metadata` 显示主框架实现为根 `openrust` package，剩余 package 各有明确技术/分发理由；不存在无用途的能力空壳 crate。
- 主框架公共路径保持 `openrust::...` 稳定；`default`、各可选 feature 与 `--all-features` 的依赖启停行为均有测试。
- 根 `cargo test --workspace` 覆盖模块单元测试、公共 API 集成测试、数据库双库行为、示例与生成项目；旧 `apps/server` 的每项验收都有明确的新位置和通过证据后才能移除。
- 架构文档明确区分“框架源码 `src/`”与“仓库内应用 `apps/`”；即使没有 `apps/server`，`apps/` 的用途仍然清楚。
- 按 AGENTS.md 对源码改动完成 fmt、clippy、workspace tests、doc、双库和生成项目验收；用 `rg` 清查旧 crate/import/路径及所有文档引用。
