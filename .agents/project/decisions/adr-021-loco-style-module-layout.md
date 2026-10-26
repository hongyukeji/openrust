# ADR-021 主框架模块、工具包与目录职责

- 状态：Partially Superseded（模块/目录与宿主职责有效；工具发布身份和独立生成项目由 [ADR-039](adr-039-source-workspace-distribution.md) 替代）
- 日期：2026-10-25
- 参考：按 [分版本对标规则](../plans/framework-design.md#框架参考与对标)，V1 综合评估 Loco、Laravel、Goravel、OpenSpring，若依、芋道仅作为 V2 参考；本决策最初取材于 [Loco 仓库结构](https://github.com/loco-rs/loco)。
- 替代：[ADR-019](adr-019-neutral-crate-directories-and-app-package.md)、[ADR-020](adr-020-workspace-layout-and-loco-practices.md) 的执行方案；吸收中性目录、配置/源码分离和示例独立组织原则。ADR-007 的宿主验证职责在被等价接替前继续有效。

## 背景

初始 workspace 将多数能力拆成独立 package，并包含 cache/events/queue/storage 空壳。它们共享版本、发布节奏、扩展模型和入口，单独维护 manifest 没有相应收益。主框架现已收敛至根 package；用户进一步要求清理顶层 `crates/`、按项目/能力组织 examples/tests，并尽快评估 `apps/server` 职责。目录简化仍须保留 Rust 技术边界和验收覆盖。

Loco 的框架主体是一个 Cargo package，在 `src/` 下按功能组织 Rust 模块；它把必要的生成器/维护工具作为额外 package，并把示例应用与测试单独组织。OpenRust 应借鉴这个 package 与 module 的分工，同时保留 Rust 中确有必要的 proc-macro 和工具边界。

## 决策

- OpenRust 的核心框架收敛为主 Cargo package `openrust`，其实现放在仓库根 `src/`，能力按 `src/core/`、`src/http/`、`src/auth/`、`src/db/`、`src/cache/`、`src/queue/`、`src/events/`、`src/storage/`、`src/mail/` 等职责组织为 Rust modules。具体模块名称以公共 API contract 和现有代码评估为准，不为目录对称创建空模块。
- 仓库顶层目录职责明确：根 `src/` 只放 OpenRust 框架代码；`apps/` 放同一仓库/workspace 内开发和集成的应用 package，作为应用目录约定保留；`examples/` 放可复用的示例应用；根 `tests/` 验证框架公共 API。按 ADR-039，默认获取整套源码，在 `apps/<name>/` 创建业务应用；其内部仍有自己的 `src/`、`config/`。
- 主框架包内使用 `pub mod` / 私有子模块与明确的 facade/re-export 控制公共 API；内部能力间仍须遵循依赖方向和 feature 边界，不因合并 package 而把实现全部塞进 `lib.rs` 或形成不可分辨的全局模块。
- 保留 CLI（二进制与模板）、proc-macro（编译技术边界）、测试工具（应用 dev-dependency、单向依赖框架）的独立 package。不为消除 `crates/` 目录而把 proc-macro 塞入普通库、让生产库反向依赖测试包或复制实现。目录采用 `cli/`、`macros/`、`testing/`，中性 package 目标及禁用发布按 ADR-039；其余能力使用根模块。
- 根 `tests/` 承担主框架公共 API 的集成测试；crate/unit tests 放在对应源文件或 package 的 `tests/`。根 `examples/` 放可运行的完整示例应用（如 blog/demo），承担面向用户的示范和端到端 smoke test。测试或示例不能仅为复刻旧目录而复制一套 server harness。
- 当前 `apps/server` 是框架自己的聚合启动/集成验证宿主，不等同于用户业务应用。只有在根 `tests/` 与 `examples/` 覆盖它的启动、默认 feature、插件组合、生成项目、数据库双库等验收后，才可移除这个具体 package；这不改变 `apps/` 作为仓库内应用目录约定。不得先删除现有宿主再补测试。根 package 承载框架库；明确选择应用启动，避免根 workspace 多二进制歧义，用户应用在 `apps/<name>/src/main.rs` 统一启动。
- 框架与用户生成项目的 Rust 配置逻辑都采用 `src/config/` 模块目录（以 `src/config/mod.rs` 为入口，按真实复杂度拆分子模块）；运行时 TOML/YAML 配置文件继续放在项目根 `config/`，不放入编译源码目录。两处 `config/` 含义不同：源码目录放 Rust 代码，项目根目录放可部署配置文件。
- 目录收口作为独立底层功能簇执行：先保护并安全闭环正在开发的 WIP，再校准路径和测试入口；不得将已完成的根模块迁移重复列为新任务，也不得把目录变化等同于功能验收。

## 影响与迁移

- 迁移前写独立 5–15 行 Spec，盘点 package/features、依赖环、宏 crate、测试工具、模板、生成项目、CI、文档与所有硬编码路径；设计从源码 workspace 中业务开发者的角度确认依赖仍以单一 `openrust` 门面为主。
- 确认迁移后的根 `src/` 属于框架 package，`apps/` 仍可承载同 workspace 的用户/集成应用；不把二者合并为一个目录，也不因移除 `apps/server` 而取消 `apps/` 约定。
- 一次性合并当前能力源码与 manifest 配置，逐个保持 feature 启停、可选依赖、公开 re-export 和错误行为；按 `git mv` 保留历史可追踪性。未发布的 `openrust-*` capability packages 可在迁移中移除；保留工具包的中性命名按 ADR-039 执行。
- 更新根 workspace/package manifest、所有保留工具 crate 的依赖、CI、命令、模板、README、ADR 与 `.agents/project/core/` 事实。移除 package 前先确认没有外部用户依赖；当前版本尚未发布，不需要维护旧 package 的运行时兼容层。
- 若某能力后来确有编译隔离或依赖边界需求，可在证据充分后从模块提取为内部 package；不恢复独立发布前提，不预先拆分。

## 当前迁移实施

- 根目录现在同时是 `openrust` package 与 workspace root；成员为 `openrust`、`cli`、`macros`、`testing`、`server`，中性命名已按 ADR-039 完成。各保留理由见 [迁移 Spec](../plans/module-layout-migration-spec.md)。
- 已迁入 core/config、HTTP、DB、auth、mail 源码；HTTP 测试在根 `tests/`，完整 hello 示例在根 `examples/`，真实 DB/auth 测试在仍保留的 `apps/server/tests/`。
- 所有 capability packages 均未发布；模块迁移后旧 package manifests 和空壳已移除。`testing` 保持单向依赖，未造成根 package 反向依赖。
- `apps/server` 暂不移除，因它仍承担聚合启动、feature 组合、生成项目与双库验收。

## 后续执行

目录与分组测试迁移已完成，证据见 [目录 Spec](../plans/module-layout-migration-spec.md)；包改名、禁用发布及 apps 生成流程亦已按 [ADR-039](adr-039-source-workspace-distribution.md) 完成。新增底层能力按 [V1 补齐计划](../plans/v1-foundation-completion.md) 放入既有模块/工具边界，不重新移动目录或预建空 package；旧迁移验收不能替代新能力验证。
