# OpenRust Framework

框架 Cargo 包名为 `openrust-framework`，库导入名为 `openrust`。根应用通过同一个可执行文件进入 HTTP 或 Artisan。框架以固定 Laravel 版本为行为基线，应用装配参考固定 Goravel 版本。 [上游基线](../../../.agents/project/plans/laravel-rebuild-baseline.md) · [启动架构](../../../.agents/project/architecture.md)

按[安装指南](../../../docs/zh-Hans/getting-started/installation.md)初始化应用密钥和 SQLite 后，从仓库根目录运行以下命令，分别启动 Web 服务和查看 Artisan 命令。

```sh
cargo run
cargo run -- artisan list
```

Web 服务默认监听 `127.0.0.1:8000`，可用 `/up` 检查服务存活。发布布局和独立二进制运行见[部署指南](../../../docs/zh-Hans/getting-started/deployment.md)。

## 公开接口入口

| 职责 | Rust 路径 | 正文 |
|---|---|---|
| 应用装配 | `openrust::foundation::{Application, runApplication}` | [请求生命周期](../../../docs/zh-Hans/architecture-concepts/lifecycle.md) |
| 容器 | `openrust::container::Container` | [服务容器](../../../docs/zh-Hans/architecture-concepts/container.md) |
| Provider | `openrust::support::ServiceProvider` | [服务提供者](../../../docs/zh-Hans/architecture-concepts/providers.md) |
| 公开契约 | `openrust::contracts` | [契约](../../../docs/zh-Hans/digging-deeper/contracts.md) |
| 应用门面 | `openrust::support::facades` | [门面](../../../docs/zh-Hans/architecture-concepts/facades.md) |
| Artisan | `openrust::console` | [Artisan](../../../docs/zh-Hans/digging-deeper/artisan.md) |

[English](../../../docs/en/README.md) · [简体中文](../../../docs/zh-Hans/README.md) · [繁體中文](../../../docs/zh-Hant/README.md)

[交付计划与依赖](../../../.agents/project/plan.md) · [Laravel 文档对照](../../../.agents/project/plans/laravel-docs-delivery-map.md)
