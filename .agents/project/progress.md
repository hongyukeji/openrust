# 当前交付与验证

> 受众：AI Agent
> 更新：2026-10-28
> 摘要：R0 已固定上游；R1-S 根结构与过渡模块的实际门禁已通过，已本地提交34dfb5e；R1整体及R2/模块系统仍未完成，当前R2-A/A2/A3基元与配置来源子簇已验收，统一Provider未切换。

当前根 openrust 同 package 提供框架 lib 和唯一默认 server，应用职责回根；app/example 是通过现有公开 Plugin 注册的普通业务模块，已规范分离 providers/routes/http/database/resources/tests。框架库实现与依赖/feature 语义未改，根显式 34 targets 保留原有效覆盖。根无示例 build.rs 或构建依赖；protobuf 工具和锁都在模块内。

结构子步骤已实际通过 fmt、默认/full Clippy/tests/docs、deny、no-default；默认 263/22、full 486/32（passed/ignored）。根裸 cargo run 的健康/API/公开 HTML/业务响应与正常退出、根 CLI 转发、PG18.6/MySQL8.4.13 各 12 targets/48 项及 testing 1 项、三类生成项目、最小 orders 模块仅改应用清单的真实 HTTP 闭环通过。CI 双库六组、MSRV1.97、八个独立 consumer 与额外 gRPC 生成项目均通过；不同模型最终源码复审无 blocker/should-fix，独立 CI 复核无 blocker/should-fix，已本地提交34dfb5e；最新事实见 [testing](memory/testing.md#r1-当前工程边界与验收入口)。

R1 总体保持 in_progress：结构提交只是前置子步骤，不代表真实 CRUD、所有当前公开能力示例、Laravel/modules 等价或 ServiceProvider 生命周期已完成。后继 app/example/extensions.rs 未挂载/未验收，单独保留 WIP。后继依赖与 Provider contract 按最新 [计划](plan.md) 和 [待办](todo.md) 推进。

最终目标已收敛为 `modules/<模块名>/` 与唯一 `modules/example/`，见 [ADR-074](decisions/adr-074-laravel-modules-directory.md)。当前证据仍归 app/example 过渡代码；modules 目录、完整 ServiceProvider、模块管理和 CRUD 未交付。计划修订与文档检查不能代替这些代码验收。

当前规划记录已统一：ADR-074、总计划、活跃队列、执行说明与 README 区分 app/example 过渡事实和 modules/example 目标；此前规划文档检查覆盖 63 个变更 Markdown、1222 个本地链接和 53 个锚点，无断链；749 个 Laravel 稳定 ID 与 35 个模块入口 ID 未改状态。文档检查不是代码门禁，代码门禁已补齐，限定提交仍须完成最终审查与提交树核对。

R2-A 容器基元已实际完成15项回归与默认279/22、full502/32及双Clippy/docs、deny、MSRV/no-default。公开下游consumer1项与根裸run/HTTP通过，不同模型最终复审无blocker/should-fix；已提交26df75c。生产启动仍未切换；Provider、异步资源生命周期、配置合并与模块系统留后继，不把基元当整体R2完成。

R2-A2已实际通过25项配置定向、默认286/22与full509/32及对应双库/生成/consumer/HTTP门禁；来源错误不再退默认，Debug/日志不输出目录原值，已提交2003f0a；独立A3补齐容器发布/构造帧竞态回归，继续Provider/配置契约，不把源加载修复当完整内核完成。

R2-A3已实际修复缓存发布早于构造帧结束导致误报cycle，以及scope/root reset锁内通知导致同步Wake重入死锁。19项定向、默认290/22与full513/32、双Clippy/docs、deny、MSRV1.97/no-default通过；公开下游check/test1项通过，根裸run的health/HTML/API/example200及SIGINT0通过。生产仍是旧Plugin，完整Provider/资源异步关闭和模块/CRUD未实现，extensions保持后继WIP。
