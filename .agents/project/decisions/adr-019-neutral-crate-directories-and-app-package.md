# ADR-019 中性 crate 目录与聚合应用包名

- 状态：Superseded（由 [ADR-021](adr-021-loco-style-module-layout.md) 统一管理目录职责与迁移）
- 日期：2026-10-25

## 原决策与替代原因

原方案将各能力从 `crates/openrust-<capability>` 改为 `crates/<capability>`，保留发布包名，并将内部 `apps/server` 包改名为 `server`。它试图减少目录中的品牌重复，但仍维持过多独立能力 package。

用户随后要求以根 `src/` 的 Rust modules 组织框架。ADR-021 将能力收敛进根 `openrust`，吸收中性目录原则，取消逐能力 crates 及 `server` 包名迁移。因此本 ADR 不再提供可执行改名清单。

## 当前入口

- 模块与目录职责按 [ADR-021](adr-021-loco-style-module-layout.md)；其后的整套源码交付、中性 package 名及 server 改名按 [ADR-039](adr-039-source-workspace-distribution.md)，不恢复原逐能力 crates 拓扑。
- 默认应用配置命名空间和表前缀仍由 [ADR-016](adr-016-neutral-application-namespaces.md) 决定。
