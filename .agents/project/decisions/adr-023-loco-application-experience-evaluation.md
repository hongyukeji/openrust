# ADR-023 Loco 应用扩展、CLI 与测试体验评估

> 替代范围见 [ADR-070](adr-070-laravel-first-rebuild.md#替代范围)；本文旧目录/命名/默认值不阻止全面重写。代码是否已迁移以当前实现和 todo.md 为准。

- 状态：Partially Superseded（目标设计由 ADR-070 重新开放；未替代的行为/验收事实保留）
- 日期：2026-10-25
- 依据：[ADR-001](adr-001-own-kernel-on-axum.md)、[ADR-006](adr-006-rust-native-naming.md)、[ADR-021](adr-021-loco-style-module-layout.md)、当前 `openrust::core` / `openrust::http` 与 `testing` 实现
- 参考：[Loco Hooks](https://loco.rs/docs/reference/hooks/)、[Loco CLI](https://loco.rs/docs/reference/cli/)、[Loco doctor](https://loco.rs/docs/how-to/run-doctor/)、[Loco generators](https://loco.rs/docs/reference/generators/)、[Loco request tests](https://loco.rs/docs/how-to/request-tests/)、[Loco seed data](https://loco.rs/docs/how-to/seed-data/)
- 当前参考范围按 ADR-001：V1 综合评估 Loco、Laravel、Goravel、OpenSpring，若依/芋道仅 V2；保留本 ADR 的历史 Loco 调研，不将其变成单一框架优先规则。

## 评估结论

### 1. 生命周期扩展

- Loco 的 `Hooks` 是集中式应用 trait，提供路由前后、初始化、启动前与关闭等回调；这解决了 app-level wiring 与生命周期回调分散的问题。
- OpenRust 已有应用装配入口：`Application::build` 同步注册插件、路由、服务和命令；`finalize` 逐批按注册顺序执行 `Plugin::build`（build 期间追加的插件随后处理），任一 build 错误即停止；全部 build 成功后冻结成 `App`，再按 build 顺序逐个 await `Plugin::finish`，任一 finish 错误即停止后续 finish。插件经 `AppState` 共享服务，不提供通用的任意阶段回调。
- `HttpPlugin` 在 build 阶段将 route builders 追加至 `RouteRegistry`；`build_router` 按注册顺序折叠 route builders，再按注册顺序应用已注册 Tower router layers 和默认 HTTP middleware。它们是路由/请求链扩展点，不是应用启动或关闭回调。
- 当前没有通用 `before_routes` / `after_routes` / `before_run` 回调，也没有 Plugin shutdown callback。HTTP `serve` 监听 Ctrl-C/SIGTERM，并通过 Axum `with_graceful_shutdown` 停止接收新连接、等待在途请求处理；serve 结束后不通知插件执行异步清理。
- **决策：拒绝新增与 Plugin 并列的 `Hooks` trait 或通用路由前后钩子。** 这会造成第二套生命周期注册面；现有同步 build、异步 finish、RouteRegistry 和 Tower layer 已覆盖通用装配需求。
- **停机方案已由 ADR-037 定稿：** worker/scheduler 命令持有 supervisor、任务集合和信号，负责停止领取/扫描、合作取消、有界 drain、abort/join 与 ownership-token/lease 恢复。不新增通用 Plugin shutdown，也不再把它列为 M4 实施前未决条件。HTTP 在途请求排空与 worker/scheduler 停机分别验收；真实信号/强杀证据以当前 queue/schedule 事实为准。
- 借鉴来源与适用前提：Loco Hooks 集中入口适用于拥有单一应用上下文生命周期的框架，并由其 serve 流程在 graceful shutdown 后调用 `on_shutdown`；OpenRust 通过分层 Plugin 和显式 AppState 实现应用装配，更符合现有 Rust 结构。Loco 的 shutdown 回调不代表 OpenRust 已有相同行为。验证方式是 M4 多 worker/scheduler 的真实停机与恢复测试。

### 2. CLI 诊断与生成器

- 当前全局 `openrust` CLI 提供 `new`、`dev`、`serve`、`generate model/migration/mailer/policy`、`key generate`，未知命令转交当前应用二进制；应用运行时命令通过 `Command`/Plugin 注册，`help` 与脱敏 `config show` 来自 core，迁移和 seed 由数据库能力提供。`dev` 当前仅选择开发环境并启动 serve，尚未完成依赖编排/监听重启；doctor 与剩余生成器仍待实施，具体种类和事实见 [CLI](../core/cli.md)。
- Loco 把运行期命令与生成器作为 app 开发主入口，并提供配置/数据库诊断、controller/model/scaffold、task/worker/mailer 等代码生成。适用前提是每个生成物都接入实际运行机制并有模板快照与生成项目验证。
- **决策：不复制 Loco 命令体系；保留 OpenRust `Command` 和现有 CLI 的单一入口。** `mailer` / `policy` 已补交；`job` 按 M4、controller/scaffold 与诊断按 M6 交付。V1 还须提供应用模板覆盖、导出/预览与版本检查，见 [补齐计划](../plans/v1-foundation-completion.md)。每个生成器只在对应能力 contract 与生成项目验收可用时实现。
- `doctor` 列入 M6，限定为只读诊断：读取并校验当前项目配置；在数据库已配置时报告后端 feature/当前开发目标版本检查、连通性和迁移状态；对可选依赖报告未配置/未启用/不可达及可操作建议。不得自动安装服务、运行迁移/seed、改写配置或业务数据，也不得输出密钥。`config show` 已承担脱敏配置查看，doctor 只报告配置校验结果与键路径，不重复打印配置值。
- task/worker 不因参考框架有同名命令就新增平行生成器；复用现有 Job、Command、Scheduler 契约，`job` 与 `command` 的生成范围按活跃待办交付。避免生成无法构建或未注册到运行时的空文件。
- 来源：Loco 官方 CLI 与 generator reference；解决从空项目接入常见能力的发现成本。适用前提是模板可以随稳定公共 API 测试，不要求照搬其 Rails 风格资源结构。

### 3. 测试开发体验

- `testing` 公开 `TestDatabase::create::<M>`、`create_with_table_names`、`url`、`connection`、`close`，覆盖 PostgreSQL 迁移模板克隆和 MySQL 隔离库迁移、清理及 URL 脱敏；隔离契约见 [ADR-038](adr-038-isolated-test-databases.md)。尚无 TestApp、HTTP 断言或轻量 fixture 工具；能力 fake 目前由对应模块提供。
- 框架现有底层可复用 `lifecycle::finalize`、`build_router`、Tower `ServiceExt::oneshot`、`TestDatabase`、各能力 fake driver。`根目录集成宿主` 和 HTTP 模块测试当前直接组合这些入口，证明单请求测试可行，但应用作者仍需自行接线；无网络 socket 的 HTTP 测试尚未形成稳定公开 helper。
- **决策：采纳一个薄的 `testing` 应用/HTTP 测试入口，列入 M6 测试工具闭环；不新增第二套 server、ORM fixture 系统或依赖真实端口的测试框架。** `TestApp` 应复用 `finalize`、公开 `build_router` 与 Tower `ServiceExt::oneshot`，能以显式配置和 plugin/fake 组装应用；响应断言覆盖 OpenRust JSON envelope、状态、headers 和 JSON path。数据库隔离仍复用 `TestDatabase`，按用户选择的测试数据库运行真实驱动。
- V1 必须交付显式轻量 factory/fixture 工具，复用实体、普通 Rust builder 和 `TestDatabase`，构造与持久化分开，不增加隐式 Factory trait、自动清库或第二套 ORM；具体范围见补齐计划。能力 fake 断言仍随对应 contract 添加，不作为 TestApp 隐式行为。M6 验收须有独立 Application 测试，证明无需真实 socket 即可调用路由，验证响应、错误与显式 fake/plugin 替换；fixture 的可复现数据、关系约束、失败和隔离须在 PostgreSQL/MySQL 执行。
- 来源：Loco 官方 `boot_test` / request test / seeded database 体验；OpenRust 采用显式 Rust builder 和现有工具组合，避免复制 Loco `AppContext` 或自动 truncate 语义。

## 项目文档与计划同步

- 当前配置目录原则沿 [ADR-021](adr-021-loco-style-module-layout.md)：Rust 配置逻辑进入框架/生成应用 `src/config/mod.rs` 模块目录；运行时 TOML/YAML 始终位于项目根 `config/`。本 ADR 不新增配置布局决策。
- `todo.md` 是唯一活跃任务队列。原 M3 的 mailer/policy 已补交，M4 的 job 生成器仍待交付；M6 完成其余生成器、模板覆盖、fixtures 和完整生成项目验收，不能因“原阶段完成”漏项。停机 contract 已由 ADR-037 定稿，剩余工作是实施与验收，不能再次要求设计通用 shutdown hook。
- 本评估不代表 doctor、TestApp 或待补生成器已经交付；实现及实际验证记录见对应 `core/`、Git 与 `memory/`。

## 实际代码核验

- `src/core/app.rs`：`Application::build` 与 `AppBuilder` 注册插件、命令、服务；`src/core/lifecycle.rs`：`finalize` 的 build/finish 顺序及首个错误即返回；`src/core/plugin.rs`：`Plugin` 仅有 build/finish。
- `src/http/plugin.rs`：`RouteRegistry`、router layer registry、`build_router`；`src/http/serve.rs`：Axum graceful shutdown。
- `tools/cli/src/main.rs`：全局命令与 generator enum；`src/core/command.rs` 及 `src/db/{migration,seed}.rs`：运行时命令入口。
- `tools/testing/src/lib.rs` 与 `tools/testing/README.md`：测试 package 保留 `testing::TestDatabase` 入口，当前公开测试 API 仅提供数据库隔离工具；实现由根 `openrust::testing` 提供。
