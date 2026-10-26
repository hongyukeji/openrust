# apps/server

> 受众：AI Agent
> 更新：2026-10-26
> 摘要：仓库内默认启动应用与跨能力集成验证宿主；可选能力的测试装配不等于默认 `serve` 已全部接入。

## 边界

`openrust-server` 是不发布的二进制 crate，正式依赖只有门面 `openrust`。不承载业务资源；后续能力按 [ADR-007](../decisions/adr-007-aggregate-server-entry.md) 接入。

## 当前实现

| 文件 | 内容 |
| --- | --- |
| `src/main.rs` | 调用 `openrust::run::<Server>()` |
| `src/app.rs` | `Server` 显式安装 `HttpPlugin`、`I18nPlugin`、`MailerPlugin`、`SmsPlugin`；`encryption` feature 下额外安装 `EncryptionPlugin`；注册框架验证路由 |
| `src/routes.rs` | `/` 返回服务名和版本的 JSON；`HttpPlugin` 提供 `/health` |
| `config/*.toml` | 与框架默认一致的应用、监听、压缩和 CORS 配置；仓库 `.cargo/config.toml` 为根目录 Cargo 命令指定配置目录 |
| `tests/server/aggregate.rs` | 默认 Server、邮件/SMS 服务、health 与请求 ID、根路由、资源宏、命令遮蔽及配置加载等集成测试 |
| `tests/auth/`、`tests/database/`、`tests/http/`、`tests/queue/`、`tests/schedule/` 等 | 按 feature 构建各自测试 Application，显式注册数据库、认证、迁移、队列与调度；数据库测试还需真实服务配置 |

启用 `db-postgres` / `db-mysql` / `auth` / `queue` feature 只开放相关编译能力，不会让默认 `Server::build` 自动注册它们。默认 `serve` 没有数据库、认证、队列或调度装配；完整认证 HTTP 业务流程另由 CLI 生成项目验收。

## 后续边界

目录收尾评估结论为保留 `apps/server`：真实服务二进制、配置目录、双库与隔离库测试、worker/scheduler 子进程验收目前没有根示例或公共 API tests 的等价替代。其保留不代表默认 server 已自动装配数据库、认证、队列或调度能力。

## 验证

`cargo test -p openrust-server`；`cargo run` 后请求 `http://127.0.0.1:3000/health`。
