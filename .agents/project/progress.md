# 当前进度

> 受众：AI Agent
> 更新：2026-10-25
> 摘要：M0、M1 已完成；M2 `openrust-db` 连接池与事务辅助已实现并完成双库验证，后续数据层功能仍在进行。

| 项 | 状态 |
| --- | --- |
| 第一代计划与框架设计 | 已定稿，见 `plans/` |
| ADR-001 至 ADR-008 | 已接受 |
| 仓库基线（许可证、README、工具链声明、Agent 契约、CHANGELOG、贡献与安全政策） | 已就位 |
| workspace、lint 基线、CI 工作流 | 已就位；CI 双库 job 已启用，MSRV 为 Rust 1.97 |
| `openrust-core` | 已实现并测试：配置分层、错误与 `ApiResponse<T>` 包装渲染、注册表、plugin 级联、空格嵌套命令 |
| `openrust-http` | 已实现并测试：`Routes`、`Resource` / `ResourceActions`、`Created` / `NoContent` / `PageParams` / `Paginated`、OpenRust `Json` / `Path` / `Query`、`Inject`、`Validated`、404/405/408/413 包装兜底、`Created::with_location`、含 gzip/br 与 CORS 的中间件栈、`/health`、`serve` |
| `openrust-macros` | `#[openrust::resource]` 从控制器动作关联函数生成 `Resource` 实现，并由 `apps/server` 集成测试覆盖 |
| `openrust` / `openrust-cli` | 门面 `run`、无数据库项目生成、`dev` / `serve` 与运行命令代理可用 |
| `openrust-db` | `DatabaseConfig` / `DatabasePlugin` / `Db`、事务提交与回滚、可选 PostgreSQL / MySQL features；MySQL 强制 Rustls TLS；PostgreSQL 14 / MySQL 8.0 集成测试通过 |
| `apps/server` | M1 聚合入口已实现：默认 HTTP、配置文件、`/health`、根路由与无端口集成测试 |
| 其余 8 个 crate | 空壳，README 标注规划 surface 与里程碑 |
| 验证证据 | 本机 `cargo fmt / clippy -D warnings / test / doc` 通过，见 `memory/testing.md` |
