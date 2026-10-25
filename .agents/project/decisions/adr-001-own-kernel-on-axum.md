# ADR-001 自有应用内核，基于 axum

- 状态：Accepted
- 日期：2026-10-25

## 背景

早期方案以 Loco 为基础生成项目。核实 Loco 1.2.0：基于 axum + SeaORM 2.0，feature 只有 `db-sqlite` 与 `db-postgres`，队列后端为 Redis / Postgres / SQLite，没有 MySQL 路径。若在 Loco 之上构建，OpenRust 只剩模板层，目录、配置、CLI、认证与错误模型全部由 Loco 决定，无法保证统一的设计风格；支持 MySQL 意味着长期维护 fork。

## 决策

OpenRust 自研应用内核（`Application` / `AppBuilder` / `ServiceProvider` / `AppState` / `Config` / `Error` / 生命周期），直接建立在 `tokio` + `axum` + `tower` 之上，复用成熟 crate 承担 HTTP、ORM、会话、邮件、存储等底层能力。Loco 与 Rails / Laravel 作为设计参考与对标对象，不作为依赖。

## 影响

- 自研面集中在应用层约定与 CLI；底层协议、驱动与中间件不自研。
- 需要自行维护项目模板、生成器与文档站。
- 可完全控制双库支持、feature 粒度与公共 API 风格。
