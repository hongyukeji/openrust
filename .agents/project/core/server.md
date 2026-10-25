# apps/server

> 受众：AI Agent
> 更新：2026-10-25
> 摘要：仓库内的聚合启动入口与 M1 集成验证宿主。

## 边界

`openrust-server` 是不发布的二进制 crate，正式依赖只有门面 `openrust`。不承载业务资源；后续能力按 [ADR-007](../decisions/adr-007-aggregate-server-entry.md) 接入。

## 当前实现

| 文件 | 内容 |
| --- | --- |
| `src/main.rs` | 调用 `openrust::run::<Server>()` |
| `src/app.rs` | `Application` 实现，显式安装 `HttpPlugin` 并注册框架验证路由 |
| `src/routes.rs` | `/` 返回服务名和版本的 JSON；`HttpPlugin` 提供 `/health` |
| `config/*.toml` | 与框架默认一致的应用、监听、压缩和 CORS 配置；仓库 `.cargo/config.toml` 为根目录 Cargo 命令指定配置目录 |
| `tests/server.rs` | 4 个测试覆盖 health 与请求 ID、根路由、命令遮蔽及配置加载 |

## 验证

`cargo test -p openrust-server`；`cargo run` 后请求 `http://127.0.0.1:3000/health`。
