# ADR-062 根目录统一启动入口

- 状态：Accepted
- 日期：2026-10-27
- 替代：ADR-007 与 ADR-021 中保留 `apps/server` package、须另行评估移除宿主的旧约定；框架模块与用户应用分层仍遵循 ADR-021/039。

## 背景

开发者在仓库根目录运行 `cargo run -p server`，还需进入 `apps/server` 才能加载相对配置，入口和目录职责显得分散。框架自身不作为独立发布 package，根 package 已经同时是 workspace 与框架库；保留一个额外 server package 不再提供必要的隔离价值。

## 决策

- 根 package 同时提供 `openrust` library 与名为 `server` 的 binary target。宿主 Rust 代码统一位于根 `src/`，从仓库根目录直接 `cargo run` 即启动；`cargo run -- ...` 将参数传给宿主。
- workspace `default-members` 只列根 package，使裸 `cargo run` 不会与 `tools/cli` 的 `openrust` 可执行目标冲突；完整 workspace 检查显式使用 `--workspace`。
- `apps/` 只保留开发者业务应用目录约定，不再放置框架自带宿主 package。生成应用继续作为 `apps/<name>/` 下独立 package，有自己的 `src/` 与根 `config/`。
- 宿主测试保留在根 `tests/server/`，通过根 package 的 feature 和显式 target 管理；配置放根 `config/`。性能基准归独立 `tools/performance/` workspace package；框架配置 Rust 模块仍在 `src/config/`。
- `testing` package 保留为下游测试工具兼容入口。`TestDatabase` 实现在根 `src/testing/`，工具 package 只 re-export 框架实现，根集成测试直接用 `openrust::testing::TestDatabase`，避免 package 依赖环和实现重复。
- 迁移必须保留 HTTP smoke、双库隔离测试、子进程信号/崩溃、feature 组合、CLI 生成与配置隔离等现有覆盖；移除的是重复宿主 package，不是集成验收。

## 后果

- 仓库根的 `cargo run` 启动 `server`；需要显式选 feature 时使用 `cargo run --features ...`。
- 裸 `cargo test` 针对根 package；贡献者运行全部工具包和框架测试时使用 `cargo test --workspace`。
- 框架 package 与宿主共用依赖/feature manifest，宿主集成测试在根 package 运行；外部生成应用依然通过其自己的 Cargo manifest 管理依赖。
- `apps/` 可保持简洁，仅放业务应用。根 `src/` 同时包含框架 modules 与小型默认宿主；业务 handler/资源仍应放在业务应用或示例，不向框架库塞入领域功能。
