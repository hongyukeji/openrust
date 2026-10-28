# 当前交付与验证

> 受众：AI Agent
> 更新：2026-10-29
> 摘要：R0 已固定上游；R1-S、R2-A/A2/A3、R2-B/C/D/M 已按子簇完成适用门禁；R1-E 综合示例与真实 CRUD 已以 bc3fa01 提交。R2 后继内核和完整 Laravel/laravel-modules 行为矩阵继续推进。

当前根 openrust 同 package 提供框架 lib 和唯一默认 server，应用职责回根；modules/example 是普通业务模块，经生产 `Application::modules` 编译清单与 FileModuleActivator 状态接入。根无示例 build.rs 或构建依赖；protobuf 工具和锁仍在模块内。

结构子步骤已实际通过 fmt、默认/full Clippy/tests/docs、deny、no-default；默认 263/22、full 486/32（passed/ignored）。根裸 cargo run 的健康/API/公开 HTML/业务响应与正常退出、根 CLI 转发、PG18.6/MySQL8.4.13 各 12 targets/48 项及 testing 1 项、三类生成项目、最小 orders 模块仅改应用清单的真实 HTTP 闭环通过。CI 双库六组、MSRV1.97、八个独立 consumer 与额外 gRPC 生成项目均通过；不同模型最终源码复审无 blocker/should-fix，独立 CI 复核无 blocker/should-fix，已本地提交34dfb5e；最新事实见 [testing](memory/testing.md#r1-当前工程边界与验收入口)。

R1-E 真实 CRUD/当前公开能力示例已按适用范围验收并以 bc3fa01 提交，附件 deadline/EOF 竞态修复经独立复审和真实 MinIO 双库场景验证；限定的跨系统故障边界仍记录在 [testing](memory/testing.md#r1-e-审查修复与追加验收2026-10-29)。这只关闭当前综合示例子簇，不代表 Laravel/laravel-modules 全行为矩阵完成。后继按 [待办](todo.md) 领取。

目录目标为 `modules/<模块名>/` 与唯一 `modules/example/`，见 [ADR-074](decisions/adr-074-laravel-modules-directory.md)。R2-M 当前实现与真实验收单独记录；完整管理命令、发布流程和 CRUD 未交付。计划修订与文档检查不能代替这些代码验收。

当前规划记录已统一：ADR-074、总计划、活跃队列、执行说明与 README 区分 app/example 过渡事实和 modules/example 目标；此前规划文档检查覆盖 63 个变更 Markdown、1222 个本地链接和 53 个锚点，无断链；749 个 Laravel 稳定 ID 与 35 个模块入口 ID 未改状态。文档检查不是代码门禁，代码门禁已补齐，限定提交仍须完成最终审查与提交树核对。

R2-A 容器基元已实际完成15项回归与默认279/22、full502/32及双Clippy/docs、deny、MSRV/no-default。公开下游consumer1项与根裸run/HTTP通过，不同模型最终复审无blocker/should-fix；已提交26df75c。生产启动仍未切换；Provider、异步资源生命周期、配置合并与模块系统留后继，不把基元当整体R2完成。

R2-A2已实际通过25项配置定向、默认286/22与full509/32及对应双库/生成/consumer/HTTP门禁；来源错误不再退默认，Debug/日志不输出目录原值，已提交2003f0a；独立A3补齐容器发布/构造帧竞态回归，继续Provider/配置契约，不把源加载修复当完整内核完成。

R2-A3已实际修复缓存发布早于构造帧结束导致误报cycle，以及scope/root reset锁内通知导致同步Wake重入死锁。19项定向、默认290/22与full513/32、双Clippy/docs、deny、MSRV1.97/no-default通过；公开下游check/test1项通过，根裸run的health/HTML/API/example200及SIGINT0通过；已提交。旧生产 Plugin 路径已由 R2-B 替换，不把此前基元证据转记新架构验收。

R2-B：生产启动已改为全部 register→基础配置→顺序 boot→最终校验/required 解析；HTTP 请求、命令及领域服务使用唯一 Container，旧 AppBuilder/Plugin 启动循环和领域 Plugin 类型已移除。受管资源覆盖启动失败/取消与 App/HTTP scope 关闭。默认/all-features workspace 测试、Clippy、Rustdoc、deny、MSRV1.97、无默认 feature 均已实际通过；PG18/MySQL8.4 各 12 targets/54 项，独立生成双库应用、八个消费者及根默认 server 真实 HTTP/退出的证据见 [testing](memory/testing.md#r2-b-统一-provider-内核验收证据)。Figment 来源字典顺序和第三方自定义 driver 校验仍有明确差异；不宣称 R2-M 模块管理或 R1-E CRUD。

R2-C：HTTP 请求 scope 延续到 stream/SSE body 结束或断连；命令、队列每次 attempt、调度每次到期 slot 各有独立 scope。受管任务在该 scope/显式 metadata 中构造和执行，关闭先封解析/取消任务，再由 App 持有的单一清理 driver 有界回收；普通 spawn 不继承身份。真实 TCP 三项、PostgreSQL 18/MySQL 8.4 各 queue 3 项和 schedule 7 项、Redis 8 三项、公开自定义 driver binary、双库实际生成应用的 check 与真实 auth HTTP 各一项均通过。默认/全特性 workspace fmt/Clippy/tests/Rustdoc、deny、no-default、MSRV1.97、根实际 HTTP 及退出也通过；完整命令、初次 gRPC 5 秒超时后的单项与全量复跑、忽略项和日志见 [testing](memory/testing.md#r2-c-入口-scope-与受管清理验收证据)。当前仍是 `app/example` 过渡模块，R2-D/M 与 R1-E 待后继。

R2-D：`TestApp<A>` 与生产应用共用 Provider 清单和单一 Container/Router 构建路径，支持注册前移除 Provider、全 register 后覆盖 typed 绑定、有界进程内请求及安全响应断言；完整 reset 先关闭旧 App。Drop 即时封闭新解析，存活运行时才可托管异步兜底清理，显式 shutdown 可等待错误与资源。综合示例、公开 consumer、普通生成项目及真实 PostgreSQL/MySQL 认证生成项目均已运行；共享关闭状态机在双库 queue/schedule 和 Redis 上重新验证。默认/全特性 workspace、MSRV1.97、deny、no-default、根默认 HTTP/退出证据及一次未归因的 all-features 初始失败见 [testing](memory/testing.md#r2-d-testapp-验收证据)。`app/example` 仍是过渡模块，R2-M 与 R1-E 未交付。

R2-M：编译期 `ModuleDescriptor` 与运行时 FileModuleActivator 快照进入生产/测试共用 ProviderBuilder；模块配置在实际 boot 时点 overlay，命名空间 view/lang、模块资源与统一迁移历史均由公开能力贡献。原示例完整迁至 modules/example，根和生成应用从运行根读取状态。PostgreSQL/MySQL 隔离库的全局批次/禁用历史/错库锁/单连接池与失败重启、Redis/队列/调度、真实部署目录 HTTP、普通及双库认证生成应用均执行，完整结果见 [R2-M testing](memory/testing.md#r2-m-模块清单资源与统一迁移验收证据)。MySQL DDL 部分提交及 R13 管理/发布与 R1-E CRUD 保留边界；本簇限定本地提交收口。

R1-E（已验收并提交 bc3fa01）：`modules/example` 已接真实 Article CRUD、模块业务服务与当前公开能力的实际调用；PG18/MySQL8.4 的 Article、tenant/MinIO/Redis、队列与 Scheduler、OIDC/浏览器 WebSocket 等有定向证据。Collector PostgreSQL 活跃日志路径已校正，隔离 sink delivery/unavailable 各 1/1；16 MiB 与隐私检查保留，共享 Collector 未改。CLI 生成器重生十种变体，生成源码格式、基础/认证/MFA真实 HTTP、gRPC TLS Echo/HTTP/停机均实际验收，测试 0 ignored；CLI 20/20。默认/全特性 workspace Clippy/tests/Rustdoc、deny、no-default 与 MSRV 适用门禁均通过；默认测试 454 passed/23 ignored，全特性 990 passed/90 ignored。附件 AtomicU8 仲裁 deadline 与 EOF；PG18/MySQL8.4 全模块附件各 15/15，期限释放锁用例与真实 MinIO CopyObject 延迟响应删除竞态各按双库 1/1。独立复审确认并发 blocker 已消除；真实 Keycloak `AlwaysOk` 错误密码、已消费 state 与成功夹证 PG/MySQL 各 1/1；gRPC TLS 错误标识验证成功 RPC 前后、错误 hostname 与网络故障负例；浏览器拒绝事件结合真实 TCP 精确状态测试分别验证，未声称浏览器暴露状态码。远端结果不确定后的迟到发布、App 关闭期限和进程强杀仍不保证跨系统原子性/无孤儿。提交后任务专属 root 与临时 Cargo targets 均已清理。下一簇为 R2-N 模块硬依赖图与确定性启动顺序，具体契约见 [todo](todo.md#当前-r2-n-spec模块硬依赖图与确定性启动顺序)；R1-E 完成不代表 Laravel/laravel-modules 全体完成。
