# ADR-023 Loco 应用扩展、CLI 与测试体验评估

- 状态：Accepted（借鉴边界已定；实现按现有里程碑推进）
- 日期：2026-10-25
- 依据：[ADR-001](adr-001-own-kernel-on-axum.md)、[ADR-006](adr-006-rust-native-naming.md)、[ADR-021](adr-021-loco-style-module-layout.md)、当前 `openrust::core` / `openrust::http` 与 `openrust-test` 实现
- 参考：[Loco Hooks](https://loco.rs/docs/reference/hooks/)、[Loco CLI](https://loco.rs/docs/reference/cli/)、[Loco doctor](https://loco.rs/docs/how-to/run-doctor/)、[Loco generators](https://loco.rs/docs/reference/generators/)、[Loco request tests](https://loco.rs/docs/how-to/request-tests/)、[Loco seed data](https://loco.rs/docs/how-to/seed-data/)

## 评估结论

### 1. 生命周期扩展

- Loco 的 `Hooks` 是集中式应用 trait，提供路由前后、初始化、启动前与关闭等回调；这解决了 app-level wiring 与生命周期回调分散的问题。
- OpenRust 已有应用装配入口：`Application::build` 同步注册插件、路由、服务和命令；`finalize` 逐批按注册顺序执行 `Plugin::build`（build 期间追加的插件随后处理），任一 build 错误即停止；全部 build 成功后冻结成 `App`，再按 build 顺序逐个 await `Plugin::finish`，任一 finish 错误即停止后续 finish。插件经 `AppState` 共享服务，不提供通用的任意阶段回调。
- `HttpPlugin` 在 build 阶段将 route builders 追加至 `RouteRegistry`；`build_router` 按注册顺序折叠 route builders，再按注册顺序应用已注册 Tower router layers 和默认 HTTP middleware。它们是路由/请求链扩展点，不是应用启动或关闭回调。
- 当前没有通用 `before_routes` / `after_routes` / `before_run` 回调，也没有 Plugin shutdown callback。HTTP `serve` 监听 Ctrl-C/SIGTERM，并通过 Axum `with_graceful_shutdown` 停止接收新连接、等待在途请求处理；serve 结束后不通知插件执行异步清理。
- **决策：拒绝新增与 Plugin 并列的 `Hooks` trait 或通用路由前后钩子。** 这会造成第二套生命周期注册面；现有同步 build、异步 finish、RouteRegistry 和 Tower layer 已覆盖通用装配需求。
- **缺口与后续决策点：** M4 worker/scheduler 与集群停机需要有界取消、任务 join、租约处理及插件资源关闭语义。M4 实现前应基于 worker/scheduler 的实际任务所有权与运行模型，决定是否扩展既有 `Plugin` 增加异步 shutdown 生命周期；若采纳，contract 必须规定关闭顺序（通常与启动依赖反向）、错误聚合、单插件超时、取消传播、join、租约释放及多插件失败后的继续策略。此项目前未决，不等同于现有 HTTP graceful shutdown。
- 借鉴来源与适用前提：Loco Hooks 集中入口适用于拥有单一应用上下文生命周期的框架，并由其 serve 流程在 graceful shutdown 后调用 `on_shutdown`；OpenRust 通过分层 Plugin 和显式 AppState 实现应用装配，更符合现有 Rust 结构。Loco 的 shutdown 回调不代表 OpenRust 已有相同行为。验证方式是 M4 多 worker/scheduler 的真实停机与恢复测试。

### 2. CLI 诊断与生成器

- 当前全局 `openrust` CLI 实际提供 `new`、`dev`、`serve`、`generate model/migration`，未知命令转交当前应用二进制；应用运行时命令通过 `Command`/Plugin 注册，`help` 与脱敏 `config show` 来自 core，迁移和 seed 由数据库能力提供。尚无全局 `doctor`，也无 controller/scaffold/task/job/mailer/policy 生成器。
- Loco 把运行期命令与生成器作为 app 开发主入口，并提供配置/数据库诊断、controller/model/scaffold、task/worker/mailer 等代码生成。适用前提是每个生成物都接入实际运行机制并有模板快照与生成项目验证。
- **决策：不复制 Loco 命令体系；保留 OpenRust `Command` 和现有 CLI 的单一入口。** `doctor` 已纳入 M6；常见 API 资源生成沿 v1 计划推进，`mailer` / `policy` 按 M3、`job` 按 M4、controller/scaffold 与诊断按 M6 交付。每个生成器只在对应能力 contract 与生成项目验收可用时实现。
- `doctor` 列入 M6，限定为只读诊断：读取并校验当前项目配置；在数据库已配置时报告后端 feature/当前开发目标版本检查、连通性和迁移状态；对可选依赖报告未配置/未启用/不可达及可操作建议。不得自动安装服务、运行迁移/seed、改写配置或业务数据，也不得输出密钥。`config show` 已承担脱敏配置查看，doctor 只报告配置校验结果与键路径，不重复打印配置值。
- task/worker 生成器暂不单独加入当前待办：M4 先定义 Job、Command、Scheduler 的实际注册与运行契约，再判断各自是否有高频模板价值。避免生成无法构建或未注册到运行时的空文件。
- 来源：Loco 官方 CLI 与 generator reference；解决从空项目接入常见能力的发现成本。适用前提是模板可以随稳定公共 API 测试，不要求照搬其 Rails 风格资源结构。

### 3. 测试开发体验

- `openrust-test` 当前公开 API 只有 feature-gated `TestDatabase::create::<M>`、`url`、`connection`、`close`，覆盖 PostgreSQL 临时迁移库克隆和 MySQL 隔离库重放、清理与 URL 脱敏；它没有实现 `TestApp`、HTTP 断言、Factory 或 fake-driver 断言。
- 框架现有底层可复用 `lifecycle::finalize`、`build_router`、Tower `ServiceExt::oneshot`、`TestDatabase`、各能力 fake driver。`apps/server` 和 HTTP crate 当前直接组合这些入口，证明单请求测试可行，但应用作者仍需自行接线；无网络 socket 的 HTTP 测试尚未形成稳定公开 helper。
- **决策：采纳一个薄的 `openrust-test` 应用/HTTP 测试入口，列入 M6 测试工具闭环；不新增第二套 server、ORM fixture 系统或依赖真实端口的测试框架。** `TestApp` 应复用 `finalize`、公开 `build_router` 与 Tower `ServiceExt::oneshot`，能以显式配置和 plugin/fake 组装应用；响应断言覆盖 OpenRust JSON envelope、状态、headers 和 JSON path。数据库隔离仍复用 `TestDatabase`，按用户选择的测试数据库运行真实驱动。
- Factory 与 fake 断言仅在对应能力驱动 contract 定稿后逐项添加，不作为 TestApp 的隐式自动行为。M6 公共验收须有独立 Application 测试，证明无需真实 socket 即可经 `TestApp` 启动并调用路由，验证统一响应、状态、headers、JSON path、错误响应和显式 fake/plugin 替换；配置隔离数据库的路径继续使用 `TestDatabase` 并在 PostgreSQL/MySQL 执行。
- 来源：Loco 官方 `boot_test` / request test / seeded database 体验；OpenRust 采用显式 Rust builder 和现有工具组合，避免复制 Loco `AppContext` 或自动 truncate 语义。

## 项目文档与计划同步

- 当前配置目录原则沿 [ADR-021](adr-021-loco-style-module-layout.md)：Rust 配置逻辑进入框架/生成应用 `src/config/mod.rs` 模块目录；运行时 TOML/YAML 始终位于项目根 `config/`。本 ADR 不新增配置布局决策。
- `todo.md` 仅新增 M4 worker/scheduler shutdown contract 与 M6 doctor/TestApp 闭环；`mailer` / `policy` / `job` / controller / scaffold 等生成器保留在 `gen1-plan.md` 原里程碑，不复制成新的平行计划或待办。M6 CLI 对已有和新生成器的验证继续按 gen1-plan 执行。
- 此评估仅记录设计边界，不代表尚未实现的 shutdown hook、doctor、TestApp 或生成器已经交付。

## 实际代码核验

- `src/core/app.rs`：`Application::build` 与 `AppBuilder` 注册插件、命令、服务；`src/core/lifecycle.rs`：`finalize` 的 build/finish 顺序及首个错误即返回；`src/core/plugin.rs`：`Plugin` 仅有 build/finish。
- `src/http/plugin.rs`：`RouteRegistry`、router layer registry、`build_router`；`src/http/serve.rs`：Axum graceful shutdown。
- `crates/openrust-cli/src/main.rs`：全局命令与 generator enum；`src/core/command.rs` 及 `src/db/{migration,seed}.rs`：运行时命令入口。
- `crates/openrust-test/src/lib.rs`、`src/database.rs` 与 `README.md`：当前公开测试 API 仅为数据库隔离工具。
