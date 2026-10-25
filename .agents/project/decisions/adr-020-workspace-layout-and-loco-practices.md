# ADR-020 Workspace 布局与 Loco 借鉴边界

- 状态：Accepted
- 日期：2026-10-25
- 参考：[Loco 仓库](https://github.com/loco-rs/loco)
- 关联：[ADR-006](adr-006-rust-native-naming.md)、[ADR-007](adr-007-aggregate-server-entry.md)、[ADR-019](adr-019-neutral-crate-directories-and-app-package.md)

## 背景

Loco 的框架主体是仓库根 Cargo package，因此根 `src/`、根 `tests/` 与 `examples/` 服务于该主包；workspace 另有 `loco-gen`、`xtask`，并排除独立 starter 与 examples。OpenRust 则是多个独立发布的能力 crate 加一个内部聚合应用组成的 Cargo workspace。两者都使用 Rust 常规的包内 `src/`、`tests/`、`examples/`，但不能仅凭目录名称相同就把整套布局互换。

## 决策

- 保持 OpenRust workspace 结构：能力 crate 位于 `crates/<capability>/`，内部聚合启动与集成验证宿主位于 `apps/server/`。每个 Cargo package 自己拥有 `src/`，需要集成测试的 package 自己拥有 `tests/`；workspace 根目录不新增一个承载框架主体的 `src/` 或泛化 `tests/`。
- 保留 `apps/server`。它不是用户业务应用的替代物，而是只依赖 `openrust` 门面的聚合启动入口和集成验证宿主。移除它会丢失统一启动、门面 API 的端到端验证及能力接入验收锚点；这些职责需要显式承接后，才有理由重新评估其位置。
- 独立业务示例统一放仓库根 `examples/`，例如计划中的 `examples/blog`；示例应用应能通过明确的 Cargo workspace/package 配置独立构建和测试。单个 crate 的最小 API 示例仍可遵循 Cargo 约定放在该 crate 的 `examples/`。
- 应用的 TOML/YAML 等运行时配置文件放在应用根目录 `config/`，与 `src/` 并列，使配置可独立挂载、编辑和部署；`src/config.rs` 或 `src/config/` 用于 Rust 配置加载、类型和校验代码，不用于存放运行时配置文件。
- 借鉴 Loco 的独立业务示例、清晰的应用级测试、可预测的配置发现和生成项目启动验收；不照搬其单主包拓扑，也不因此把 OpenRust 的多个框架 crate 收并到一个 package。

## 影响与迁移

- 本决策确认当前架构边界，不触发大规模目录搬迁。既有 `apps/server`、各 crate 的 `src/` / `tests/` 及应用根 `config/` 按现状保留。
- M6 的完整示例采用根 `examples/`；示例项目需纳入生成/构建/测试验收，不复制框架实现代码。
- ADR-007 继续有效；其 `apps/server` package 命名按 ADR-019 更新，职责不变。

## 验收

- workspace 根 manifest 继续只承担 workspace/共享配置，不定义框架主 package；根目录不产生与 crate 或应用测试重复的测试宿主。
- `cargo run` 继续启动默认成员 `apps/server`；`cargo test --workspace` 覆盖所有 crate 与聚合应用测试。
- 生成项目中运行时配置保持 `config/` 根目录，Rust 配置加载代码位于 `src/`；示例应用集中于根 `examples/` 并有可执行的构建与测试入口。
