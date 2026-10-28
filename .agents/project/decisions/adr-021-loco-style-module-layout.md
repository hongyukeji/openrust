# ADR-021 主框架模块、工具包与目录职责

> 替代范围见 [ADR-070](adr-070-laravel-first-rebuild.md#替代范围)；本文旧目录/命名/默认值不阻止全面重写。代码是否已迁移以当前实现和 todo.md 为准。

- 状态：Partially Superseded（目标设计由 ADR-070 重新开放；未替代的行为/验收事实保留）
- 日期：2026-10-25
- 更新：2026-10-27（将独立工具 package 收纳至 `tools/`，保留 package 名与 Cargo 入口）
- 参考：按 [分版本对标规则](../plans/framework-design.md#框架参考与对标)，V1 综合评估 Loco、Laravel、Goravel、OpenSpring，若依、芋道仅作为 V2 参考；本决策最初取材于 [Loco 仓库结构](https://github.com/loco-rs/loco)。
- 替代：[ADR-019](adr-019-neutral-crate-directories-and-app-package.md)、[ADR-020](adr-020-workspace-layout-and-loco-practices.md) 的执行方案；吸收中性目录、配置/源码分离和示例独立组织原则。ADR-007 的宿主验证职责在被等价接替前继续有效。

## 背景

初始 workspace 将多数能力拆成独立 package，并包含 cache/events/queue/storage 空壳。它们共享版本、发布节奏、扩展模型和入口，单独维护 manifest 没有相应收益。主框架现已收敛至根 package；用户进一步要求清理顶层 `crates/`、按项目/能力组织 examples/tests，并尽快评估 `apps/server` 职责。目录简化仍须保留 Rust 技术边界和验收覆盖。

Loco 的框架主体是一个 Cargo package，在 `src/` 下按功能组织 Rust 模块；它把必要的生成器/维护工具作为额外 package，并把示例应用与测试单独组织。OpenRust 应借鉴这个 package 与 module 的分工，同时保留 Rust 中确有必要的 proc-macro 和工具边界。

## 决策

- OpenRust 的核心框架收敛为主 Cargo package `openrust`，其实现放在仓库根 `src/`，能力按 `src/core/`、`src/http/`、`src/auth/`、`src/db/`、`src/cache/`、`src/queue/`、`src/events/`、`src/storage/`、`src/mail/` 等职责组织为 Rust modules。具体模块名称以公共 API contract 和现有代码评估为准，不为目录对称创建空模块。
- 仓库顶层目录职责明确：根 `src/` 放 OpenRust 框架 modules 与唯一启动入口；`apps/mod.rs` 显式装配根宿主的 Rust 业务模块，`apps/<name>/` 也可作为生成的独立应用；唯一综合用户示例放 `apps/example/`，独立验收 consumer 放 `tests/consumers/`；根 `tests/` 验证公共 API 与宿主集成。独立生成项目仍有自己的 `src/`、`config/`。模块装配规则见 ADR-063，示例目录规则见 ADR-064。
- 主框架包内使用 `pub mod` / 私有子模块与明确的 facade/re-export 控制公共 API；内部能力间仍须遵循依赖方向和 feature 边界，不因合并 package 而把实现全部塞进 `lib.rs` 或形成不可分辨的全局模块。
- 保留 CLI（二进制与模板）、proc-macro（编译技术边界）、测试工具（应用 dev-dependency、单向依赖框架）和性能基准的独立 package。不为消除 `crates/` 目录而把 proc-macro 塞入普通库、让生产库反向依赖测试包或复制实现。四个工具 package 集中放入 `tools/{cli,macros,testing,performance}/`，减少根目录平铺；中性 package 名称及禁用发布按 ADR-039，其他能力使用根模块。
- 根 `tests/` 承担主框架公共 API 与宿主集成测试；unit tests 放在对应源文件或 package 的 `tests/`。综合用户应用位于 `apps/example/`；需独立 manifest 或真实拓扑的下游验收放 `tests/consumers/`，见 ADR-064。
- 遵循 Cargo target 约定：主 package 的集成测试放在根 `tests/`；综合示例 package 位于 `apps/example/`，独立验收 consumer 位于 `tests/consumers/`；基准套件归入 `tools/performance/` workspace package，并在该 package 内使用 `benches/` target。性能验证已有独立计划和真实服务配置，集中到专用包便于隔离运行入口与扩展对照基准；不再给主 package 增加基准 target。只有存在明确依赖、构建隔离或运行维护边界时才为其他 target 拆包；`tools/testing/` 因可被下游应用作为 dev-dependency 而保留独立 package 边界。
- 按 [ADR-062](adr-062-root-server-entry.md)，根 package 的 `src/main.rs` 提供 `server` bin，仓库根目录可直接 `cargo run`；根 `examples/demo/tests/server/` 保留聚合集成与进程验收，独立 `apps/server` package 不再保留。根宿主对 `apps/` 业务模块的显式编译期组合见 ADR-063。
- 框架与用户生成项目的 Rust 配置逻辑都采用 `src/config/` 模块目录（以 `src/config/mod.rs` 为入口，按真实复杂度拆分子模块）；运行时 TOML/YAML 配置文件继续放在项目根 `config/`，不放入编译源码目录。两处 `config/` 含义不同：源码目录放 Rust 代码，项目根目录放可部署配置文件。
- 目录收口作为独立底层功能簇执行：先保护并安全闭环正在开发的 WIP，再校准路径和测试入口；不得将已完成的根模块迁移重复列为新任务，也不得把目录变化等同于功能验收。

## 影响与迁移

- 迁移前写独立 5–15 行 Spec，盘点 package/features、依赖环、宏 crate、测试工具、模板、生成项目、CI、文档与所有硬编码路径；设计从源码 workspace 中业务开发者的角度确认依赖仍以单一 `openrust` 门面为主。
- 确认迁移后的根 `src/` 属于框架 package 与默认集成宿主，`apps/` 留给开发者业务应用；保持二者职责清楚。
- 一次性合并当前能力源码与 manifest 配置，逐个保持 feature 启停、可选依赖、公开 re-export 和错误行为；按 `git mv` 保留历史可追踪性。未发布的 `openrust-*` capability packages 可在迁移中移除；保留工具包的中性命名按 ADR-039 执行。
- 更新根 workspace/package manifest、所有保留工具 crate 的依赖、CI、命令、模板、README、ADR 与 `.agents/project/core/` 事实。移除 package 前先确认没有外部用户依赖；当前版本尚未发布，不需要维护旧 package 的运行时兼容层。
- 若某能力后来确有编译隔离或依赖边界需求，可在证据充分后从模块提取为内部 package；不恢复独立发布前提，不预先拆分。

## 当前迁移实施

- 根目录现在同时是 `openrust` package 与 workspace root；成员为 `openrust`、`cli`、`macros`、`testing`、`performance`，默认成员仅 `openrust`，保证裸 `cargo run` 启动根 `server` bin；四个独立 package 位于 `tools/`。中性命名已按 ADR-039 完成。各保留理由见 [迁移 Spec](../plans/module-layout-migration-spec.md)。
- 已迁入 core/config、HTTP、DB、auth、mail 源码；HTTP 测试在根 `tests/`，hello 与 queue 用法整合到 `apps/example/`（ADR-064），聚合 DB/auth 测试在根 `examples/demo/tests/server/`。
- 所有 capability packages 均未发布；模块迁移后旧 package manifests 和空壳已移除。`testing` 保持单向依赖，未造成根 package 反向依赖。
- 独立 `apps/server` package 已按 ADR-062 移除；启动、feature 组合、真实进程与双库验收仍在根 package 保留。

## 后续执行

目录与分组测试迁移已完成，证据见 [目录 Spec](../plans/module-layout-migration-spec.md)；包改名、禁用发布及 apps 生成流程亦已按 [ADR-039](adr-039-source-workspace-distribution.md) 完成。新增底层能力按 [V1 补齐计划](../plans/v1-foundation-completion.md) 放入既有模块/工具边界，不重新移动目录或预建空 package；旧迁移验收不能替代新能力验证。
