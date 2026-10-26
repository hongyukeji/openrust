# 当前进度

> 受众：AI Agent
> 更新：2026-10-26
> 摘要：根 `openrust` 模块迁移已独立提交；M2 读写分离已实现，并在 PostgreSQL 18.6 / MySQL 8.4.13 真实复制拓扑验证。模块化配置与 `.env` 功能簇已实现并通过 workspace 验证；M3 总验收与完整集群验收未关闭。日常开发基线为 PostgreSQL 18 / MySQL 8.4 LTS / Redis 8；PostgreSQL 16 测试仅作历史专项证据。

| 项 | 状态 |
| --- | --- |
| v1 版本计划与框架设计 | 已定稿，见 `plans/` |
| 架构决策 | 以 `decisions/` 中有效状态为准；缓存标准应用默认 Redis（ADR-015），driver/store contract 见 ADR-014，均不代表缓存已实现 |
| 仓库基线（许可证、README、工具链声明、Agent 契约、CHANGELOG、贡献与安全政策） | 已就位 |
| workspace、lint 基线、CI 工作流 | 已就位；CI 双库 job 已启用，MSRV 为 Rust 1.97 |
| `openrust::core` / `openrust::config` | 已迁入根 package 并测试：`APP_` 默认及自定义配置命名空间、配置分层与 `.env` 引导、错误和响应包装、服务注册表、Plugin 级联、嵌套命令与固定窗口限流契约 |
| 配置模块整理 | `src/config/{app,api,auth,database,logger,mailer,server}.rs`；Auth/Database/Mail 旧路径 re-export；生成 `.env.example` 含本地覆盖示例，不含可用密钥。production 跳过 `.env`，进程环境变量优先；workspace default/all-features 门禁通过 |
| `openrust::http` | 已迁入根 package 并测试：`Routes`、`Resource` / `ResourceActions`、统一响应与分页、OpenRust 提取器、`Inject`、`Validated`、404/405/408/413 包装、中间件、`/health` 与 `serve` |
| `openrust-macros` | `#[openrust::resource]` 从控制器动作关联函数生成 `Resource` 实现，并由 `apps/server` 集成测试覆盖 |
| `openrust` / `openrust-cli` | facade `run` 与数据库/auth plugin facade；`new` 支持无数据库、PostgreSQL、MySQL 和 `--auth` 模板；认证模板生成 Auth flows routes；`generate model` / `generate migration`、`dev` / `serve` 与运行命令代理可用 |
| `openrust::db` | `DatabaseConfig` / `DatabasePlugin` / `Db`、主库与显式只读副本池、轮询选择、task-local primary scope、自定义 SeaORM 初始化、事务、迁移/seed 命令、UTC/JSON 方言 helper、table resolver、双库、MySQL Rustls TLS、共享固定窗口限流存储 |
| `openrust-test` | `TestDatabase` 为每个用例创建并迁移独立库；支持 `create_with_table_names` 以相同 resolver scope 执行迁移；PostgreSQL 迁移模板克隆，MySQL 独立建库；`close().await` 清理；基线两库集成测试通过 |
| `openrust::mail` | `Email` / `Mailer`，log、fake、可选 SMTP Rustls driver；已接入 facade、server 与项目模板 |
| `apps/server` | M1 聚合入口；`auth` feature 集成测试验证 facade PAT 与 JWT 路由、JWT 不依赖 SessionPlugin，并与 `Inject<T>` 共存。生成 AuthFlows HTTP 真机测试现由 CLI 模板提供；默认数据库认证器的 PAT 入口在生成模板增加，MySQL e2e 通过 |
| `openrust::auth` | AuthPlugin/custom Authenticator、Session/PAT/JWT 路由认证、CurrentUser/OptionalUser、共享 Session/CSRF、refresh/verification/Policy migrations、Argon2id、AuthFlows 与基础 Policy/RBAC 已实现。M3 仍有双库/生成项目/文档验收，不因移动模块而关闭 |
| 配置与数据命名 | `APP_` 默认、自定义单一命名空间、环境/`.env` 引导、配置目录和前缀覆盖已实现并通过 core/CLI/workspace 与双库生成项目 check/test；数据库表前缀/映射实现已接入 Db、MigrationPlugin、TestDatabase 与 Auth SQL；PostgreSQL/MySQL 真实隔离库矩阵与迁移 runner 验证通过，工作区全量 fmt/clippy/test/doc 通过 |
| 后续能力模块 | `cache` / `queue` / `events` / `storage` 尚未实现；按 M4/M5 计划直接在根 package 创建非空模块，不保留空壳 package |
| v1 新增范围 | 读写分离已按 ADR-029 实现并通过复制延迟验收；流程钩子、缓存、可观测性及最终集群按专题计划补做 |
| 验证证据 | 根迁移与 M2 相关默认/all-features 检查通过；PG18.6 / MySQL8.4.13 真实复制测试覆盖暂停/追平、主库新读和两个独立 App scope；500 次串行 `SELECT 1` 路由微基准已记录。真实 SMTP 送达和完整多节点集群仍无验收证据 |
| M2 验收 | 双库数据库测试、model/migration 生成项目编译与测试、生成迁移 up/status/down、默认与 all-features workspace 验证通过；详见 `memory/testing.md` |
