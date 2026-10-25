# ADR-007 聚合启动入口 `apps/server`

- 状态：Accepted
- 日期：2026-10-25

## 背景

仓库里只有 `crates/openrust/examples/hello.rs` 这一个最小入口。框架各项能力（HTTP、数据库、认证、队列等）由不同 crate 提供。开发者希望在仓库根目录用一条命令，把已实现的全部能力组装成一个应用并启动，不必逐个启动子项目，用来本地运行、调试和做集成验证，也方便其他贡献者上手。

## 决策

### 定位

- `apps/server` 是框架仓库的**聚合启动入口**和**集成验证宿主**。它对应 Laravel 应用的 `app` 入口和 Spring Boot 多模块项目里的聚合启动模块，在 Rust 中的写法是一个可执行 crate，在 `app.rs` 中用 `app.plugin(..)` 组装各能力插件。先例：Bevy 的 `App` + `Plugin`、reth 的 `bin/` + `crates/` 布局。
- 它只组装框架能力，不写业务代码。完整业务示例另由 `examples/blog`（M6）承担。
- 它不是用户项目的依赖，也不对外发布。用户项目由 `openrust new` 生成，结构见 ADR-006 §1。

### 目录与命名

```text
apps/
└── server/
    ├── Cargo.toml          # package = "openrust-server"，publish = false，只依赖门面 openrust
    ├── README.md           # 启动命令、可选能力 feature、所需外部服务
    ├── config/
    │   ├── default.toml
    │   └── development.toml
    ├── src/
    │   ├── main.rs         # 只调用 openrust::run::<Server>()
    │   ├── app.rs          # impl Application for Server：按 feature 组装插件、服务、路由
    │   └── routes.rs
    └── tests/              # 启动聚合应用后的集成测试
```

- 根 `Cargo.toml` 中设 `members = ["crates/*", "apps/*"]`，并设 `default-members = ["apps/server"]`，在根目录直接 `cargo run` 就会启动聚合入口。
- 应用类型命名为 `Server`，不叫 `App`，避免与 `openrust::App` 冲突（ADR-006 §4）。
- 启动命令：
  - `cargo run`，等价于 `cargo run -p openrust-server`，默认执行 `serve`；
  - `cargo run -p openrust-server -- config show`、`-- help`；
  - `cargo test -p openrust-server`。

### 能力接入规则

- **只接已实现的能力**，不预留空插件、空目录或空 feature。每个里程碑交付新能力时，在同一个功能簇里把它接入 `apps/server`，并补集成测试。这是该能力完成的条件之一。
- 不需要外部服务的能力始终启用，包括 HTTP、进程内缓存、本地存储、开发邮件驱动。
- 需要外部服务、或需要在编译期二选一的能力，用 `apps/server` 的 feature 控制，feature 名转发到门面同名 feature，例如 `db-postgres`、`db-mysql`、`redis`。
  - `default` feature 只包含不需要外部服务的能力。这样 `cargo run` 不需要任何外部服务也能启动；需要数据库时用 `cargo run --features db-postgres`。
  - 必须允许各 feature 同时开启，因为 CI 会用 `--all-features` 构建。同时开启多个数据库驱动时，运行期由 `DATABASE_URL` 选择用哪一个。
- 外部服务由 `apps/server/compose.yaml` 提供，从 M2 起随数据库能力加入。

### 依赖方向与发布

- `openrust-server` 位于依赖图顶层，只依赖门面 `openrust`。与用户项目一样受「生成的应用只依赖门面」的约束，以此验证门面暴露的 API 足够用。没有任何框架 crate 依赖它。
- `publish = false`，不进入 crates.io 发布范围，不参与 `cargo semver-checks`。

### 与其他入口的分工

| 位置 | 职责 |
| --- | --- |
| `apps/server` | 一条命令启动全部已实现能力；做集成验证；作为 IDE 的默认运行配置 |
| `crates/openrust/examples/hello.rs` | 最小片段，演示几行代码就能跑起来。保留，不扩展 |
| `crates/openrust-*` | 能力实现以及各自的单元和集成测试 |
| `templates/`（`openrust-cli` 内置） | `openrust new` 输出的项目骨架 |
| `examples/blog`（M6） | 带业务代码的完整示例，也是文档的代码来源 |
| 用户项目 | 业务代码。大型项目可以把业务按模块拆成 workspace：每个模块是一个 crate，各自导出 `Plugin`，由项目的可执行 crate 统一 `app.plugin(..)` 组装，与 `apps/server` 的模式相同 |

### CI

`apps/*` 是 workspace 成员，现有的 `cargo clippy/test/doc --workspace --all-features` 和 MSRV、deny 等 job 会自动覆盖它。从 M2 起，`databases` job 增加 `cargo test -p openrust-server --features db-postgres` 与 `--features db-mysql` 两项。

## 影响

- 根 `Cargo.toml` 新增 `apps/*` 成员和 `default-members`。
- `architecture.md` 的分层图在最上层加入 `apps/server`。`conventions.md`「框架规范」加入接入规则。各能力的完成定义增加一条：已接入 `apps/server` 并有集成测试。
