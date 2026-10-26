# ADR-019 中性 crate 目录与聚合应用包名

- 状态：Superseded（由 [ADR-021](adr-021-loco-style-module-layout.md) 统一管理目录职责与迁移）
- 日期：2026-10-25

## 原决策与替代原因

原方案将各能力从 `crates/openrust-<capability>` 改为 `crates/<capability>`，保留发布包名，并将内部 `apps/server` 包改名为 `server`。它试图减少目录中的品牌重复，但仍维持过多独立能力 package。

用户随后要求以根 `src/` 的 Rust modules 组织框架。ADR-021 将能力收敛进根 `openrust`，吸收中性目录原则，取消逐能力 crates 及 `server` 包名迁移。因此本 ADR 不再提供可执行改名清单。

## 当前入口

- 必要工具包的目标目录为 `cli/`、`macros/`、`testing/`，发布包名保持；顶层 `crates/` 的最终移除按 ADR-021 执行。
- `apps/server` 当前包名仍为 `openrust-server`。只有职责被等价接替并验证后，才决定移除或迁移宿主；`apps/` 约定保留。
- 默认应用配置命名空间和表前缀由 [ADR-016](adr-016-neutral-application-namespaces.md) 决定，不与 Cargo 包身份混用。
