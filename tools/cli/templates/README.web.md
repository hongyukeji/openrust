# Web 认证应用

本应用选择官方 Web/Fortify 默认机制：`web` Guard、受保护 Cookie、Session、CSRF 与现代双因素认证。`--auth` 与 `--fortify` 选择相同脚手架，可同时使用。

应用账户、Provider、配置和迁移均在本项目内编辑。登录、密码确认、双因素挑战与管理路由由 Fortify Provider 登记；业务无状态注册、邮件验证及密码重置示例位于 `routes/api.rs`。本模式不默认提供 JWT 登录与刷新接口。

先配置 `.env` 的数据库与部署密钥，再运行标准迁移及测试。SQLite 安装会创建隔离数据库并执行标准迁移。`tests/auth_flows.rs` 使用真实账户和 Router 验证 Web 管理、Cookie、Session、CSRF 与账户 Provider。

需要完全无 Session/Cookie 的 JWT API 应用时，创建项目时显式选择 `openrust new <应用名> --profile jwt-api`，不与 `--fortify` 共选。
