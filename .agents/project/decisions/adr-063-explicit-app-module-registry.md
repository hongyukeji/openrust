# ADR-063 仓库宿主的显式业务模块注册

- 状态：Accepted
- 日期：2026-10-27
- 补充：[ADR-062](adr-062-root-server-entry.md) 的根宿主布局；保持 [ADR-021](adr-021-loco-style-module-layout.md) 的框架与应用边界。

## 背景

根 `server` 需要一个清楚的应用装配位置，使开发者可以在源码 workspace 的 `apps/` 中编写业务模块，同时保留唯一启动入口。Rust 业务代码必须参与 Cargo 编译；运行时扫描目录无法发现并加载未编译的 Rust 源码，也会引入不必要的动态装载模型。

## 决策

- `src/main.rs` 是唯一进程入口，只调用 `openrust::run::<...>()`。
- 根 `server` 通过 `apps/mod.rs` 编译应用模块。开发者显式声明模块，并在应用的 `Application::build` 中通过 `AppBuilder` 注册模块提供的 `Plugin`、服务或路由。
- 复用现有 `Application`、`AppBuilder`、`Plugin` 与路由注册机制；不新增平行 Hooks、服务定位器、运行时目录扫描或动态库加载机制。
- 模块选择是编译期行为；缺少 `mod` 声明的文件不会参与构建。配置文件与 `.env` 只提供运行参数，不负责加载 Rust 代码。
- 根 `apps/` 下的源码模块属于仓库根 `server`；`openrust new` 生成的 `apps/<name>/` 是独立 Cargo package，具有自己的 `Cargo.toml`、`src/` 和根 `config/`，不会自动编译进根宿主。
- 生成的应用默认仍是当前源码 workspace 成员，共用 workspace 依赖约束与 `Cargo.lock`；CLI 当前不支持仓库外生成目标。因此“独立 package”不等于“独立升级/锁文件边界”。需要完全独立维护的应用必须使用框架仓库之外的应用 workspace；根 `apps/*.rs` 则适合源码仓库内的业务模块开发。
- 框架能力仍实现于根 package 的 `src/`；`apps/` 只承载应用组合与业务代码，不反向成为框架实现目录。

## 后果与验收

- 增加业务模块需要添加 Rust 源文件、在 `apps/mod.rs` 声明它，并显式注册其公开扩展；Cargo target 与启动命令保持不变。
- 根宿主的集成测试验证 `src/main.rs` 启动和路由组合；生成应用继续由自己的 manifest 和现有生成项目验收负责。
- 不承诺任意未编译目录或运行时放入的源码会被自动发现；依赖项仍需由 Cargo manifest 管理。
