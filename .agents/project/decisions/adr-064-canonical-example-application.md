# ADR-064 统一示例应用与验收 consumer 收纳

- 状态：Accepted
- 日期：2026-10-27
- 关联：[ADR-021](adr-021-loco-style-module-layout.md)、[ADR-039](adr-039-source-workspace-distribution.md)、[ADR-062](adr-062-root-server-entry.md)、[ADR-063](adr-063-explicit-app-module-registry.md)

## 背景

根 `examples/` 混合了入门片段、框架能力演示、独立下游 consumer 与真实多进程验收项目，项目边界和用途不清楚。开发者需要一个按真实应用结构逐步展示 HTTP、配置、数据库、认证、缓存、队列、存储与通信能力的代表项目；验收专用 consumer 仍须保留独立 manifest、feature 和运行拓扑。

## 决策

- 唯一面向开发者的综合示例应用放在 `apps/example/`，按生成应用的 Cargo package、`src/`、根 `config/`、迁移和测试组织，并随 v1 能力实际接入逐步扩展。
- 根 `examples/` 不再作为可运行示例或 Cargo target 的存放目录；简单 `hello`/`queue` 用法并入综合应用。
- 需要独立依赖图、下游 API 边界或真实多进程/容器拓扑的验收工程放在 `tests/consumers/<capability>/`，保留各自 manifest、锁文件、脚本和测试；它们是验收夹具，不作为多个入门应用宣传。
- CLI 通过 Cargo metadata 和 workspace package 发现用户应用；根宿主 `apps/mod.rs` 的 Rust 模块保持编译期显式声明。目录增加文件不会自动把 Rust 代码编入二进制；运行时文件扫描无法代替 Rust 编译与依赖声明。
- 不引入 build.rs 文件扫描、链接时注册表或运行时动态库装载。build.rs 虽可生成固定目录的 `mod` 声明，但仍须约定模块 trait、冲突/顺序、feature 门控和依赖来源，隐式行为增加诊断与 IDE 成本；显式应用 package 与现有 `Application`/`AppBuilder` 是更直接、可检查的扩展方式。

## 后果与验收

- README、计划、框架结构图和所有链接统一指向 `apps/example/` 或明确标记的 `tests/consumers/`。
- Cargo 根 examples targets 移除后，裸 `cargo run` 仍只启动根 `server`；综合应用按 CLI 的包选择/工作目录运行。
- 每个迁移的 consumer 用原命令或等价 manifest 命令验证；gRPC、实时、双库/认证等真实拓扑脚本不得因目录迁移丢失。
- 综合应用通过生成器、模型/迁移/任务生成路径以及应用自己的 check/test；计划能力只有实现并实测后才能记为已接入。
