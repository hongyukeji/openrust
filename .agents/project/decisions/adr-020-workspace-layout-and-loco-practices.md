# ADR-020 Workspace 布局与 Loco 借鉴边界

- 状态：Superseded（由 [ADR-021](adr-021-loco-style-module-layout.md) 替代；有效配置原则由 ADR-021/030 承接）
- 日期：2026-10-25
- 参考：[Loco 仓库](https://github.com/loco-rs/loco)

## 原决策与替代原因

原方案保留多个能力 package、各自 `src/`/`tests/` 和 `apps/server`，不在根建立框架 package。后续用户明确要求根 `src/` 模块化、简化 workspace，因此上述拓扑由 ADR-021 替代。不能因历史方案仍可读取而重建旧多 crate 架构。

## 已保留的原则

- 框架代码、应用、示例与测试各有职责；必要工具包保留 Rust 技术边界，具体目录与宿主覆盖门槛见 ADR-021。
- 运行时 TOML 配置位于应用根 `config/`，Rust 类型与校验位于 `src/config/`；具体分层/`.env` 规则见 [ADR-030](adr-030-modular-configuration.md)。
- 示例按项目组织，测试迁移必须保持 Cargo 发现与原验收覆盖；不以删除目录代替完整职责整合。
- 当前参考按版本划分：V1 综合评估 Loco、Laravel、Goravel、OpenSpring，若依、芋道仅作为 V2 参考，见 [框架设计](../plans/framework-design.md#框架参考与对标)；本历史评估不再作为单独偏重 Loco 的执行方案。
