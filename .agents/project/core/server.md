# 默认应用当前事实

> 受众：AI Agent
> 更新：2026-10-28
> 摘要：根 application package 只有 server bin；框架库位于 framework。

## 职责与装配

| 路径 | 内容 |
| --- | --- |
| `src/main.rs` | 薄入口，调用 `openrust::run::<bootstrap::Server>()` |
| `bootstrap/app.rs` | 使用公开 API 组合 HTTP、I18n、Mailer、SMS；按 feature 显式组合 Encryption、Cache、Storage |
| `routes/web.rs` | 应用路由注册，当前复用 framework_routes；HTTP 插件提供 health/readiness |
| `app/` | 应用业务模块；不扫描或动态加载源码 |
| `config/*.toml` | 应用运行配置；配置类型/加载机制位于 framework/src/config |
| `tests/server/` | 公开 API、真实数据库、worker/scheduler 与子进程回归 |

根 feature 转发到 openrust 并控制集成测试，启用 feature 不自动安装数据库/认证/队列插件。默认根 `cargo run` 不依赖外部服务；`APP_SERVER_PORT=4000 cargo run` 可覆盖端口。集成测试命令为 `cargo test -p application --test server`；库单元测试为 `cargo test -p openrust --lib`。

目录/生成分发见 [ADR-071](../decisions/adr-071-application-framework-layout.md) 与 [architecture](../architecture.md)。新的生命周期、HTTP、认证语义留对应后继阶段；真实验证见 [testing](../memory/testing.md)，IDE 验收单列。
