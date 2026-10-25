# 当前进度

> 受众：AI Agent
> 更新：2026-10-25
> 摘要：M0、M1、M2 已完成；M3 邮件能力已实现，认证 API contract 已定义、实现待完成；读写分离为新增数据层待办。

| 项 | 状态 |
| --- | --- |
| v1 版本计划与框架设计 | 已定稿，见 `plans/` |
| 架构决策 | 以 `decisions/` 中有效状态为准；缓存标准应用默认 Redis（ADR-015），driver/store contract 见 ADR-014，均不代表缓存已实现 |
| 仓库基线（许可证、README、工具链声明、Agent 契约、CHANGELOG、贡献与安全政策） | 已就位 |
| workspace、lint 基线、CI 工作流 | 已就位；CI 双库 job 已启用，MSRV 为 Rust 1.97 |
| `openrust-core` | 已实现并测试：配置分层、错误与 `ApiResponse<T>` 包装渲染、注册表、plugin 级联、空格嵌套命令 |
| `openrust-http` | 已实现并测试：`Routes`、`Resource` / `ResourceActions`、`Created` / `NoContent` / `PageParams` / `Paginated`、OpenRust `Json` / `Path` / `Query`、`Inject`、`Validated`、404/405/408/413 包装兜底、`Created::with_location`、含 gzip/br 与 CORS 的中间件栈、`/health`、`serve` |
| `openrust-macros` | `#[openrust::resource]` 从控制器动作关联函数生成 `Resource` 实现，并由 `apps/server` 集成测试覆盖 |
| `openrust` / `openrust-cli` | 门面 `run` 与数据库插件 facade；`new` 支持无数据库、PostgreSQL、MySQL 模板；`generate model` / `generate migration` 与 `g` 别名可用；`dev` / `serve` 与运行命令代理可用 |
| `openrust-db` | `DatabaseConfig` / `DatabasePlugin` / `Db`、自定义 SeaORM 连接插件入口、事务提交与回滚、`MigrationPlugin<M>` 的 up/down/status 命令、`SeedPlugin` 的 `db seed`、未知 backend 明确拒绝的 UTC timestamp / JSON 方言 helper、服务端最低版本启动校验、可选 PostgreSQL / MySQL features；MySQL 强制 Rustls TLS；PostgreSQL 14 / MySQL 8.0 集成验证通过 |
| `openrust-test` | `TestDatabase` 为每个用例创建并迁移独立库；PostgreSQL 迁移模板克隆，MySQL 独立建库；`close().await` 清理；两库集成测试通过 |
| `openrust-mail` | `Email` / `Mailer`，log、fake、可选 SMTP Rustls driver；已接入 facade、server 与项目模板 |
| `apps/server` | M1 聚合入口已实现：默认 HTTP、配置文件、`/health`、根路由与无端口集成测试 |
| `openrust-auth` / `openrust-cache` / `openrust-queue` / `openrust-events` / `openrust-storage` | 当前均为 crate 骨架；认证与缓存 contract 已定义，尚未实现对应运行能力 |
| v1 新增范围 | 基础读写分离、流程钩子及集群等按专题计划补做，不能用原 M2 完成标签代替验收 |
| 验证证据 | 既有测试记录见 `memory/testing.md`；文档校准不代表重新执行全量构建，真实 SMTP 投递及完整集群/复制拓扑尚无验收证据 |
| M2 验收 | 双库数据库测试、model/migration 生成项目编译与测试、生成迁移 up/status/down、默认与 all-features workspace 验证通过；详见 `memory/testing.md` |
