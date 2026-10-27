# 根目录集成宿主

> 受众：AI Agent
> 更新：2026-10-27
> 摘要：根 `openrust` package 同时提供框架库与名为 `server` 的本地集成宿主；从仓库根目录可直接 `cargo run`。

## 边界

仓库不再保留 `apps/server` package。根 `src/main.rs` 是框架开发和集成验收用的示例入口，不是用户业务应用；`apps/` 仍留给开发者自己的应用。workspace package 包含 `openrust`、`cli`、`macros`、`testing` 与独立性能验证包 `performance`。

## 当前实现

| 路径 | 内容 |
| --- | --- |
| `src/main.rs` | 名为 `server` 的 bin target，调用 `openrust::run::<Server>()` |
| `src/app.rs` | 显式安装 HTTP、I18n、Mailer、SMS；按 feature 添加 Encryption、Cache、Storage |
| `src/routes.rs` | `/` 从 `AppState.config().app.name` 返回配置名称，版本取 `CARGO_PKG_VERSION`；HTTP 插件提供 `/health` |
| `config/*.toml` | 根目录宿主运行配置；Rust 配置代码仍在 `src/config/` |
| `tests/server/` | 按能力组织聚合集成、真实数据库、worker/scheduler 进程及其他应用级验收 |

根 package 的 feature 直接控制宿主和集成测试，不再经由 `server` package 转发。启用数据库、认证、队列等 feature 不会自动安装对应插件；各测试 Application 显式组装。

## 命令

```bash
cargo run
cargo run -- config show
cargo test -p openrust --test server
```

默认监听 `127.0.0.1:3000`。可用 `APP_SERVER__PORT=4000 cargo run` 或 `PORT=4000 cargo run` 覆盖端口；配置仍从仓库根目录的 `config/` 加载。
