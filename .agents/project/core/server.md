# 根目录集成宿主

> 受众：AI Agent
> 更新：2026-10-27
> 摘要：根 `openrust` package 同时提供框架库与名为 `server` 的本地集成宿主；从仓库根目录可直接 `cargo run`。

## 边界

仓库不再保留 `apps/server` package。根 `src/main.rs` 是唯一启动入口；根 `apps/mod.rs` 组合集成宿主与显式业务模块。源码 workspace 开发者可在 `apps/` 添加 Rust 模块并通过现有 `Application` / `AppBuilder` 注册。`openrust new` 生成的 `apps/<name>/` 仍是独立 Cargo 项目，不自动编入根宿主。workspace package 包含 `openrust`、`cli`、`macros`、`testing` 与独立性能验证包 `performance`。

## 当前实现

| 路径 | 内容 |
| --- | --- |
| `src/main.rs` | 唯一 `server` bin 启动入口，调用 `openrust::run::<Server>()` |
| `apps/mod.rs` | 根宿主的应用注册表；显式组合 HTTP、I18n、Mailer、SMS，以及按 feature 启用的 Encryption、Cache、Storage；业务模块在此声明并注册 |
| `apps/home.rs` | 默认根路由模块：`/` 返回应用配置名称与版本；HTTP 插件提供 `/health` |
| `config/*.toml` | 根目录宿主运行配置；Rust 配置代码仍在 `src/config/` |
| `tests/server/` | 按能力组织聚合集成、真实数据库、worker/scheduler 进程及其他应用级验收 |

根 package 的 feature 直接控制宿主和集成测试，不再经由 `server` package 转发。启用数据库、认证、队列等 feature 不会自动安装对应插件；各测试 Application 显式组装。

Rust 模块通过 `apps/mod.rs` 在编译期显式声明和注册；运行时配置不会扫描或加载代码。新增模块不增加 bin target，也不改变启动命令。用户指南见 [`apps/README.md`](../../../apps/README.md)，决策见 [ADR-063](../decisions/adr-063-explicit-app-module-registry.md)。

## 命令

```bash
cargo run
cargo run -- config show
cargo test -p openrust --test server
```

默认监听 `127.0.0.1:3000`。可用 `APP_SERVER__PORT=4000 cargo run` 或 `PORT=4000 cargo run` 覆盖端口；配置仍从仓库根目录的 `config/` 加载。
