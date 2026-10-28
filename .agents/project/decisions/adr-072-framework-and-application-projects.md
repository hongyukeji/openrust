# ADR-072：同仓框架与应用 workspace

- 状态：Superseded；布局由 [ADR-073](adr-073-root-application-and-example-module.md) 替代；下文保留原决定与当时验收事实
- 日期：2026-10-28
- 前置：[ADR-070](adr-070-laravel-first-rebuild.md)
- 替代：[ADR-071](adr-071-application-framework-layout.md) 的目录、包身份与根默认启动约束

## 决策

框架和应用始终在同一仓库、同一 Cargo workspace。根 package/lib 为 `openrust`，领域实现位于根 `src/`；`examples/demo/` 是根 workspace 的应用成员，package 为 `demo`，唯一默认 bin 为 `server`。工具位于 `tools/`，保留独立 package 与编译边界。

根 `default-members = ["examples/demo"]`，在仓库根无参数 `cargo run` 默认启动 demo。框架测试显式使用 `-p openrust`，完整门禁使用 `--workspace`；默认成员不能代替完整 workspace 验证。demo 继承 workspace 的共同 package/dependencies/lints，只使用根 `Cargo.lock`，没有嵌套 workspace 或独立 lock。

demo 保留 app/bootstrap/config/database/routes/resources/public/storage/tests 与薄 src/main.rs，完整承担原 HTTP/认证/双库/真实服务回归。框架公开 API 测试和 consumers 留根 tests；应用进程及装配测试迁入 demo。CI 的 workspace 门禁包含 demo，真实服务 target 仍按 feature 与测试连接显式执行。

应用骨架对应 `laravel/laravel`，唯一维护来源是 `tools/cli/templates/` 与生成器。CLI `new` 按用户指定路径生成自有名称、workspace/lock 和框架依赖的用户应用，是框架能力；不改变本仓框架与 demo 同一 workspace 的组织方式。本轮仅在 `/tmp` 验证生成能力，不新建外部源码仓库或持久工程。

`examples/example/` 保留完整能力演示；demo 验证框架聚合装配与进程行为，两者不作为第二份可生成骨架。后继能力按 contract 逐步去重，不在本簇改写行为。

## Rust 与工具适配

应用职责目录通过静态模块声明接入薄 Rust 入口，保持类型、所有权、async、Cargo。根与 demo 目录无参数 `cargo run`、真实 HTTP、配置与资源路径须实际验证。Cargo 从 workspace 根启动成员时保留根工作目录，因此 demo 薄入口仅在该场景切至应用目录，使 config/public/storage/.env 相对路径属于应用；从其他目录直接部署启动保持既有工作目录约定，不修改框架配置 API。

本簇保持 `openrust::` 导入与框架语义，仅迁移工程职责及依赖路径。ADR-071 及中间独立 demo 布局的命令行与真实服务验收保留为历史证据，不能作为同一 workspace 已通过；新布局已重新通过 workspace、生成应用、双库和 CLI/HTTP 门禁及不同模型只读审查，实际执行和外部边界见 [测试基线](../memory/testing.md#r1-adr-072-历史验收事实)。
