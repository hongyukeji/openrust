# 架构

> 受众：AI Agent
> 更新：2026-10-28
> 摘要：同一 workspace 内的框架 library 与应用；当前业务目录边界见 [ADR-001](decisions/adr-001-framework-principles.md)；完整 laravel-modules 能力按矩阵逐项验收。

## 已确认目标与当前实现

正式定位为 OpenRust = Laravel + laravel-modules，目标边界见 [ADR-001](decisions/adr-001-framework-principles.md)。根`app/`支持Laravel常规Controller/Model/Provider应用开发，`modules/<模块名>/`是可选并存的模块化组织；唯一综合示例仍为`modules/example/`。根bootstrap承担启动职责，app/mod.rs只提供必要Rust模块导出。用户已撤回旧“业务只能modules、app只装配”限制。Provider/Container、模块资源注册和配置能力须统一实施，不能仅迁移目录。

以下 Workspace 段记录已迁移的 `modules/example/` 与当前 R2-M 模块内核；完整 laravel-modules 能力仍按矩阵继续验收。

## 运行时配置与调度目标边界

历史决策 080 与 [运行时配置与动态调度计划](plans/runtime-configuration-scheduling.md)记录尚未实现的V1必交底层（宿主启用可选）：`.env`/config/ConfigRepository/Provider唯一配置体系，安全白名单DB值原子发布；同一Queue/Scheduler承载代码注册任务与DB管理员计划。DB权威、通知加轮询、默认DB队列及明确事务/迁移边界；Redis仅通知缓存可丢，显式Redis队列仍须迁其自身数据。基础设施凭据/部署密钥/feature/路由/任务代码/监听不热换；业务渠道secret经加密和支持driver受控重建，否则pending_restart；V2表单消费V1同一受权API，配置保存不冒充节点已应用；普通开发者沿用Laravel类型读取，不使用新配置平台。现有进程内版本、freeze和静态schedule贡献不是该能力完成证据。

## Workspace 与职责

当前已提交基线4463238已拆为根应用openrust与vendor/openrust/framework独立openrust-framework包，根仅保留薄src/main.rs。最新批准目标是独立根应用包与 `vendor/openrust/framework/` 框架包，采用标准 `src/lib.rs`，不是平铺 lib.rs；独立包迁移已通过日常适用验收，模块扩展已由6c21bab0限定验收拆包，命名空间后继仍在唯一todo。根 default-members=["."]、default-run="server"，无参数 cargo run 启动唯一应用，应用继续通过 openrust:: 导入框架。

app/bootstrap/config/database/public/resources/routes/storage/tests 是根目录职责。唯一综合示例 modules/example 由根 server 通过模块清单统一装配，贯穿框架能力；不保留独立 demo/example 应用或根 examples 目录。框架src保持通用实现，根app可承载宿主业务；不因仅保留一个综合示例而禁止根app开发。示例专属迁移、种子、模板、协议/生成脚本及测试 fixture 归模块内部；根 bootstrap/config/routes/database/resources 等仅保留通用加载与公开挂载，不含样例特判。根不因示例保留 build.rs 或新增 build 依赖。

框架库领域实现已物理迁入vendor/openrust/framework/src/；新布局已通过独立日常适用验收。根项目对应laravel/laravel应用骨架，同一Git仓库维护vendor框架与扩展，不另开框架仓库。第一方扩展位于 vendor/openrust/<扩展>/，依赖方向应用→扩展→框架及应用→框架；核心国际化与 SQLite 后端归框架，可选数据库翻译来源归扩展。框架及第一方扩展参与 workspace 门禁，第三方vendor已按 Laravel/Composer 发布者/包名布局（seaql/sea-orm、seaql/sea-orm-migration、launchbadge/sqlx-mysql），同仓不同crate平级且包内保原布局，Cargo依赖解析不会自动改为vendor安装，许可证、版本、原始SHA与真实repository来源保持，不因目录拆分冒称独立上游仓库。工具保留 tools/ 独立编译边界，tests/consumers 保留公开 API 与真实拓扑验收。应用通过 openrust:: 公共 API 使用框架，框架不反向依赖业务示例。

CLI 模板与生成器是用户应用骨架的唯一维护来源。CLI new 可生成用户指定的应用，自有 workspace/lock；本轮只在 /tmp 验证，不形成额外长期源码仓库，不改变本仓 main 的开发约束。

## 验证边界

R1-S旧过渡结构以34dfb5e提交；统一 Provider 已在 R2-B 验收，modules/example 和模块契约在当前 R2-M 实施。默认 cargo run、workspace 门禁、根 package 真实服务测试、consumer 及生成应用分别实际验证；历史决策 072 与更早布局的通过记录仅作历史输入。

## 当前状态

| 边界 | 当前实现与事实 |
| --- | --- |
| `openrust::core`、`openrust::config` | Application/AppBuilder/AppState/Plugin/Command、错误与生命周期、分层配置；见 [core/core.md](core/core.md) |
| `openrust::http`、`openrust::i18n` | 路由、提取器、响应、中间件、health/serve、可选出站 HTTP 与请求级国际化；通用 HTTP 限流层、OpenAPI、ready 和实时通信仍待补；见 [core/http.md](core/http.md)、[core/i18n.md](core/i18n.md) |
| `openrust::db` | SeaORM 双库、连接池、事务、migration/seed、方言与表名解析；显式只读副本路由和 task-local 主库 scope 已实现，完整集群验收仍未完成；见 [core/db.md](core/db.md) |
| `openrust::audit` | 显式事件、详情白名单/脱敏、数据库与自定义 sink、事务内写入和有界清理；见 [core/audit.md](core/audit.md) |
| `openrust::auth` | JWT-first、可选 Session、refresh、PAT、验证码、AuthFlows、Policy/RBAC；现有运行时双库及生成 HTTP 验收有记录，流程扩展与其余补交项另行验收；见 [core/auth.md](core/auth.md) |
| `openrust::mail`、`openrust::sms` | log/fake、可选 SMTP/Twilio、显式 locale；Mailpit 本地实收有记录，Twilio 实发待授权；队列通知编排未完成；见 [core/mail.md](core/mail.md) |
| `openrust::security`、`openrust::signing` | 可选本地应用加密/密钥轮换、用途隔离签名链接及共享撤销；见 [core/security.md](core/security.md)、[core/signing.md](core/signing.md) |
| `openrust::queue`、`openrust::schedule` | 数据库队列/worker 与 UTC 调度已有实现；任务编排、事件、完整故障/集群验收的提交及 WIP 边界以 Git、[core/queue.md](core/queue.md)、[core/schedule.md](core/schedule.md) 为准，不能据此关闭 M4 |
| `cli` | new/dev/generate 与模板，见 [core/cli.md](core/cli.md) |
| `macros` | `#[openrust::resource]` proc-macro，见 [core/macros.md](core/macros.md) |
| `testing` | PostgreSQL/MySQL `TestDatabase`，见 [core/test.md](core/test.md) |
| 根 openrust 的 `server` bin | 聚合入口及启动/数据库/认证集成验证，见 [core/server.md](core/server.md) |
| cache/events/storage 等剩余能力 | 尚未完整实现；按 [当前计划](plan.md) 与 [能力矩阵](plans/laravel-rebuild-matrix-1.md) 逐簇推进 |

模块化专属机制已归vendor/openrust/modules/src（openrust-modules），由6c21bab0限定验收提交推送，默认集成仍属独立扩展。发现、manifest、依赖、activator、资源与模块CLI归modules；通用Container/Provider/config/routes及ProviderRegistration扩展留framework。应用通过ModuleProviderBuilderExt与Application/TestApp配置hook安装模块，共用唯一容器和生命周期；依赖方向为应用→modules→framework，framework无反向正常或build依赖。完整Modules与命名空间仍按唯一todo推进。
