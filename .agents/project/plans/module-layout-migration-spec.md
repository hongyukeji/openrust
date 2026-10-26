# 根模块与目录收尾 Spec

> 受众：AI Agent
> 状态：工具 package、示例和集成测试目录已迁移；apps/server 评估结论为保留。workspace 与真实服务验收正在执行。
> 决策：[ADR-021](../decisions/adr-021-loco-style-module-layout.md)。执行优先级见 [plan.md](../plan.md#执行校准)。

## Spec

1. 保持根 `openrust` 框架模块与公共 API；不重新拆成逐能力 crate，也不改变业务功能。
2. 完成剩余顶层 `crates/` 职责迁移，目标 `cli/`、`macros/`、`testing/`；Cargo package 名保持，目录名与包名分开处理。
3. examples 按 `examples/<name>/`、tests 按能力目录组织，显式保证 Cargo target 发现、feature 门控与用例覆盖。
4. 对 `apps/server` 做职责映射与替代评估；替代覆盖不足时保留，不能以删除宿主为验收前提。`apps/` 仍是应用目录约定。
5. 同步 path dependencies、workspace、宏展开路径、模板嵌入、CI、脚本和文档，不创建空兼容包，不混入新业务能力。
6. 用 metadata/测试清单对比、workspace/feature 门禁、双库集成和生成项目证明迁移无覆盖损失；通过后独立提交。

## 当前与目标

| Package/入口 | 当前路径 | 目标/处理 |
| --- | --- | --- |
| `openrust` | 根目录，能力在 `src/` | 保持；配置代码 `src/config/`，运行时配置在应用根 `config/` |
| `openrust-cli` | `cli/` | 已迁移；保留独立 CLI 分发、模板和测试 |
| `openrust-macros` | `macros/` | 已迁移；保留 Rust proc-macro 独立 package |
| `openrust-test` | `testing/` | 已迁移；用户 dev-dependency，单向依赖框架 |
| `openrust-server` | `apps/server` | 先评估下表；保留至职责替代验收通过，包名不为目录美观单独改名 |
| hello 示例 | `examples/hello/main.rs` | 已迁移并显式保留 `hello` target 名 |
| 公共 API 测试 | `tests/http/*.rs` | 已按 HTTP 能力分组并显式声明原 target 名 |
| 宿主集成测试 | `apps/server/tests/{auth,database,http,i18n,queue,schedule,server}/` | 已按能力分组并显式声明原 target 名；宿主职责保留 |

目标不是减少 Cargo package 数，而是消除冗余层级并让开发者看清框架/工具/应用边界。程序宏与 CLI 不因“统一 src”失去独立构建/分发要求。文件已在工作区移动；本簇仍待全部验证和独立提交，不以目录存在替代验收。

## apps/server 职责评估

| 当前职责 | 替代必须提供的证据 |
| --- | --- |
| 根 cargo run / config / help 和基础 HTTP | 明确的新启动命令、工作目录/配置定位、健康响应、默认端口和环境覆盖 |
| facade、Plugin 组合与 feature forwarding | 默认/关键/all-features 组合编译运行、显式服务依赖失败行为 |
| auth/database/queue/schedule 等真实双库测试 | 同一用例或等价更完整断言、隔离数据库生命周期、feature 和环境变量入口 |
| worker/scheduler 独立进程与异常退出测试 | 测试二进制路径、信号、lease恢复和进程回收；不能退化为单进程 fake |
| 生成项目与示例对接 | 下游公开API、模板路径/本地patch、两种数据库生成项目check/test与HTTP流程 |

评估结论：保留 `apps/server`。`tests/server/aggregate.rs` 通过 `CARGO_BIN_EXE_openrust-server` 启动真实服务；服务器使用自己的配置目录。双库、进程信号/崩溃、feature forwarding 与隔离测试数据库验收仍依赖此 package。根 HTTP 测试和 hello 示例不等价覆盖这些职责；后续若迁移，须先提供可运行二进制与同等双库/进程测试宿主。

## 执行顺序与验证

1. 领取前记录 Git/WIP、`cargo metadata --no-deps --format-version 1` 的 package/target/feature 清单，以及 `cargo test --workspace --all-features -- --list`。迁移前后均为 5 个 workspace package、265 个可列测试条目，目标名和测试名保持一致。
2. 工具 package、examples/tests 已迁移；后续验证相对路径与模板测试，比较 target 和用例清单，并保留 server。
3. 验证 `cargo fmt --all --check`、workspace default/all-features clippy/test、warnings-denied rustdoc、`cargo deny check`；关键独立 features 和 no-default-features 编译不因整理回归。必要时分批验证后统一收口。
4. 按 [testing.md](../memory/testing.md) 在 PostgreSQL 18/MySQL 8.4 实跑数据库、认证、队列、调度及崩溃恢复；例子实际启动；两种数据库生成项目 check/test 与已承诺的 HTTP 流程保持通过。服务先检查复用，不为结构整理重复创建数据库。
5. 同步 CI 的新路径与缺失 queue feature；证明服务作业中的测试未跳过，不通过移除用例、忽略失败或减小验证范围完成迁移。
6. 扫描旧路径，区分历史 ADR 与有效代码/命令/文档；运行本地链接/锚点和 diff 检查。用 Git 跟踪迁移/删除，不遗留旧 `crates/` 的源码或空壳。
7. 更新 architecture、core、README、testing 的真实路径，删除 todo 本项；报告保留/移除 server 的理由、测试入口映射、实跑结果与提交。完整 v1 集群与性能不因本簇通过而自动完成。
