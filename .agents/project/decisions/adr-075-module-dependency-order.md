# ADR-075：已编译模块的硬依赖与启动顺序

- 状态：Accepted
- 日期：2026-10-29
- 前置：[ADR-074](adr-074-laravel-modules-directory.md)、[R2-N Spec](../todo.md#当前-r2-n-spec模块硬依赖图与确定性启动顺序)

## 决策

模块通过 `ModuleDescriptor::requires(name)` 声明编译期硬依赖。仅当依赖模块也已编译并在本次启用快照中启用时，依赖方才能启动；框架不递归更改 `modules_statuses.json`，不替开发者启用依赖。身份匹配沿用现有大小写不敏感/Studly 查找行为。

准备阶段拒绝无效、未编译、未启用、重复、自身和循环依赖，错误码稳定且诊断不包含路径、配置秘密。合法图采用稳定 Kahn 拓扑排序：依赖先于依赖方；多个当前可启动模块按升序 `priority`、再按编译声明顺序选择。依赖关系优先于 `priority`。

排序结果供同一个 `ProviderBuilder` 完成模块 Provider 的 register 与 boot。不会新增第二个模块内核。此契约不含模块版本求解、安装、运行时热加载或管理状态写入；这些能力另按 laravel-modules 固定矩阵与 R13 验收。Cargo 仍统一解析单应用依赖与 features。

## 验收边界

- 模块清单单测覆盖依赖图有效/错误输入与确定性顺序。
- ProviderBuilder 集成测试证明依赖顺序在 register 和 boot 两阶段生效，且依赖优先于冲突 priority。
- `modules/example` 与 CLI 文档展示模块依赖声明；临时 `orders` 消费者依赖生产 `example` 模块，在 PostgreSQL 18/MySQL 8.4 隔离数据库上分别启动并通过 in-process HTTP 路由测试各 1/1，数据库连接另执行 `SELECT 1`。生成应用 check/test（含公开 `.requires` 调用）与根默认 `cargo run` TCP `/health`、`/api/example` 200 提供独立证据。
- 默认/all-features workspace Clippy、tests、warnings-denied Rustdoc，no-default all-target check、Rust 1.97 all-features/no-default check、`cargo deny check` 和 fmt 均通过；cargo-deny 保留既有 duplicate crate 与未匹配 license allowance 警告。
