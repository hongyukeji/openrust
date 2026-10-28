# 验收口径

## R2-C 四类入口 scope 的证据边界

真实 TCP HTTP stream/SSE 断连、连续命令与取消、真实 PostgreSQL 18/MySQL 8.4 队列和调度、Redis 8 attempt、公开 custom driver、双库生成项目及根实际 HTTP 均已执行；命令与测试结果见 [testing](testing.md#r2-c-入口-scope-与受管清理验收证据)。调度 trigger 的精确 scoped ID 由真实数据库事务加公开 custom driver 观测，后续 job attempt 由真实数据库队列消费验证；两个入口的证据组合不冒称同一 driver 路径。自定义 driver 必须在确认/重试前显式 await `JobAttempt::shutdown`；外层 future abort 的 Drop 仅请求受管回收，进程强杀依赖租约接管。R2-D TestApp、R2-M 模块目录、R1-E CRUD 以及完整 Laravel 逐项等价不在本簇完成范围。

R2-D 的生产与测试同清单启动、全 register 后覆盖、Provider 移除、完整 reset、进程内请求有界断言，以及关闭后的重新解析拒绝已由核心、`app/example`、公开 consumer 和 CLI 实际生成项目验证；普通生成应用 1/1，PostgreSQL 18/MySQL 8.4 的认证产物各真实 2/2，详见 [R2-D 测试证据](testing.md#r2-d-testapp-验收证据)。共享关闭状态机变更后另重跑双库队列/调度与 Redis，分别 3+7、3+7、3 项，零忽略。TestApp 的进程内 Router 请求不计真实 TCP；旧服务 `Arc` 的调用方所有权、运行时退出后的异步清理、R2-M 与 CRUD 仍属明确边界。all-features workspace 一次 `internal_error` 已保留首轮与复跑日志，根因尚未证实，不把推测当结论。

## 主分支整合核对

2026-10-28 按用户明确授权整合 `9b6268e` / `eeba3e1` 与 main 原有三笔设计提交；冲突仅在 plan/progress/todo，保留最新设计要求及 R0/R1 实际进度。与 eeba3e1 对比，源码、manifest、lock、模板、脚本、CI 和测试内容一致，额外变化仅为项目文档/协作规则。R1 代码门禁使用该提交的实际执行记录，本次整合不宣称重新执行构建或双库验收。

## R2-A3 缓存完成顺序验收边界

19项容器回归及默认290/22、full513/32、双Clippy/docs、deny、MSRV1.97/no-default通过；同步Wake实际证明成功发布、取消及scope/root reset通知不持内部锁且允许重入。公开consumer check/test1项通过；根裸run/health+HTML+API+example200/SIGINT0通过；独占target已清理15.3GiB，完整边界见[testing](testing.md#r2-a3-缓存发布与锁外唤醒)。不宣称生产Provider已切换或异步清理完成。

## R2-A2 配置来源验收边界

目录fail-closed与fallible figment调用传播、来源Debug/log哨兵回归通过；默认286/22与full509/32、双Clippy/docs、deny、MSRV/no-default、双库各48+testing1、三类生成/八consumer/根HTTP实际通过。默认值、优先级和锁未改；完整Provider/schema与配置合并仍待后继，容器发布竞态已在独立A3子簇验证修复。详见[testing](testing.md#r2-a2-配置目录与来源失败语义)。

## R2-A 基元验收边界

15项容器回归、默认279/22与全feature502/32、双Clippy/docs、deny、MSRV1.97/no-default、独立公开API consumer1项、根cargo run真实HTTP/正常退出通过。不同模型最终静态审查无blocker/should-fix；资源清理与早期失败修复记录见[testing](testing.md#r2-a-类型化容器基元)。本次未切换生产Provider，没有异步清理或自动请求scope，不宣称完整R2/模块配置/CRUD通过。

## R1-S 当前根结构与过渡模块

根 openrust 同 package 提供框架 lib 与默认 server，应用职责回根，当前 app/example 通过现有公开 Plugin 清单装配。按 ADR-074，modules/example 仍为后继 R2-M 目标；当前验收不等于该目录、统一 Provider 或完整 CRUD 已实现。

R1-S 实际重新通过默认/full workspace fmt/Clippy/tests/docs、deny、no-default 与 MSRV1.97；默认 263/22，全 feature 486/32（passed/ignored）。PG18.6/MySQL8.4.13 各 12 targets/48 项及 testing 1 项；最终 CI db/auth/example 三组两库均通过。四类生成项目（普通/auth双库/gRPC）、普通模块复制与直接 serde/chrono 依赖、根无参数运行/HTML/API/退出及八个消费者的适用门禁通过。gRPC 生成项目仅 compile/test（无业务用例），真实 RPC 来自明确运行的 consumer，不能混淆。

框架领域 .rs、dependencies/features 与 14c143f 等值，根 34 test targets 完整保留原覆盖；根 registry 无新增或升级，consumer 锁不变。模块自有 proto 工具锁定且重生字节一致。审查静态证据与实现者实际运行分开记录；结果及保留的外部/资源限制见 [testing](testing.md#r1-当前工程边界与验收入口)。完整模块能力、配置改进及 Provider 生命周期依后继合同，R1 总体未完成。

## R1 ADR-072 历史验收事实

根 openrust/src 与 examples/demo 位于同一仓库、同一 Cargo workspace，共用根 Cargo.lock；根 default-members=["examples/demo"] 默认启动 server。CLI 模板是新应用骨架唯一来源，ADR-072 替代 ADR-071 布局。新布局已重新执行 fmt、默认/all-features workspace Clippy/test/doc、deny、MSRV/no-default、八个 consumer、三类生成项目、根/demo 默认 cargo run 与 HTTP。默认 361/26、全 feature 481/32（passed/ignored），显式库单测 146/0；双库各 11 targets/46 项真实集成与 testing 1 项，最终 CI workspace db/auth 组分别 373/26 与 392/26。详细命令、真实服务和证据边界见 [testing](testing.md#r1-adr-072-历史验收事实)。

最终不同模型 gpt-6-sol 只读审查无 blocker/should-fix；其静态核对不替代实现者实际执行记录。仅目录职责、路径与 Cargo 工作流在本簇完成，不代表 R2 内核或 Laravel 全功能等价；忽略项、复制/完整集群、SQLite 全支持及供应商外部条件保持后继门禁。

旧 ADR-071 的根应用/独立 framework 的命令行与真实服务记录见 [testing](testing.md#r1-旧布局验收事实adr-071)，仅作为迁移回归输入。

## 当前已有证据

[ADR-039](../decisions/adr-039-source-workspace-distribution.md) 的源码交付与包改名已完成实现验收，干净源码生成/双应用隔离/双库证据见 [testing.md](testing.md)。旧包名和旧模板 patch 仅为历史记录。

| 范围 | 已有通过记录 | 证据边界 |
| --- | --- | --- |
| Laravel 式全面重构规划 | ADR-070、R0–R16、33 类能力与执行任务已记录；38 个 Markdown 文档的本地链接/锚点检查与 diff 检查通过；gpt-6.1-sol 只读计划审查发现的阶段门禁循环已修正 | 仅计划/规则/技能文档；未实施 R0/R1，未运行代码、双库或真实服务验收；旧专项证据不转记为重写通过 |
| M6 开发体验规划 | DX1–DX8 已记录依赖、交付范围和验收条件；diff 与旧引用检查通过；81 个本地链接目标、10 个锚点通过；8 个官方链接经浏览工具解析（直连返回 403） | 仅计划文档；不代表 TestApp、生成器、进程编排或源码升级已实现/验收 |
| C1 默认文档入口中文化 | 根 README、贡献/安全/行为准则/变更日志及 `src/**/README.md`、工具/宿主 README；13 个改动 Markdown 的相对链接与锚点检查、`git diff --check`、warnings-denied `cargo doc -p openrust --no-deps`、`cargo test -p openrust --doc` 通过 | 本簇仅整改默认入口说明，不覆盖 Rustdoc/源码注释（C2）或 CLI/生成模板说明（C3）；英文许可、引用和协议原文保留 |
| C2 Rustdoc 与源码/配置注释中文化 | 注释-only 改动；workspace fmt、clippy、test、warnings-denied doc、`cargo deny check`、`git diff --check` 通过；PostgreSQL 18/MySQL 8.4 各以 `auth,queue,db-<backend>` 执行 `auth_database`（6）、`db_connection`（4）、`db_migration`（1）、`queue`（1）、`queue_crash_recovery`（2）、`schedule`（4），均全部通过 | 不改 API/运行时文案/行为；生成入口 Rustdoc 改动同步快照；CLI 帮助、生成说明和模板注释仍属 C3 |
| C3 生成说明与 CLI 帮助中文化 | CLI 15 项测试覆盖中文帮助/生成模板；workspace fmt、clippy、test、warnings-denied doc、`cargo deny check`、`git diff --check` 通过；在干净源码副本中用本地 workspace/path 依赖生成无数据库、PostgreSQL、MySQL 应用，各自 `cargo check` 与 `cargo test` 通过 | 真实渲染根命令、`new`、`generate` 帮助；快照取自生成器真实产物；未改变邮件/SMS 等运行时语言策略，也不表示数据库集成测试已在该副本执行 |
| V2 前端选型文档 | pure-admin 精简国际化版 + Element Plus 选型同步；102 个本地链接目标、11 个锚点和 2 个上游链接检查通过；diff 空白检查与旧选型引用扫描通过 | 仅文档验证；未创建前端应用、安装依赖或执行前后端运行验收，v2 仍须待 v1 完整通过后启动 |
| 根模块/配置/目录 | 本次验证：`cargo fmt --all --check`、workspace Clippy、`cargo test --workspace`、`cargo doc --workspace --no-deps` 通过；生产 `.env` 加载、进程变量优先、格式错误安全失败 5 项配置单测通过，CLI 项目模板测试通过。目录迁移前后 5 packages、265 test-list entries 一致为既有验收 | 不等于 v1 全部能力、最低版本或集群验收 |
| HTTP/出站/i18n | 路由、响应、CORS、request ID；本机 TCP/fake client、受界 body/retry；locale 并发与生成项目中文错误 | 不等于第三方供应商发送、OpenAPI或实时通信验收 |
| HTTP 路由限流 | 公开 `RateLimitLayer` 支持 trusted IP、auth user、匿名 fallback 与自定义 scope/key；HTTP 行为测试加 PG18/MySQL8.4 真实隔离库 store/路由层各通过 | 未包裹路由不自动限流；完整跨节点压测仍属集群门禁 |
| M3 数据输出/查询边界 | 排序 parser 单测、严格 DTO/显式过滤与关联示例；PG18/MySQL8.4 CLI 生成 UUIDv7/软删除项目的格式、check 和真实隔离库 CRUD 通过 | allowlist/授权由应用定义；软删除 scope 是显式调用，不自动覆盖 raw SeaORM/SQL |
| M4 类型化事件 | 顺序/短路/类型隔离、fake queue payload/error 单测；临时下游 consumer API check；PostgreSQL 18.6 与 MySQL 8.4 的真实隔离库事件 rollback/commit/worker 消费各通过；workspace default/all-features tests、all-target/all-features clippy、warnings-denied docs、deny、fmt 通过 | 不覆盖事件跨节点广播、exactly-once 或完整集群 |
| ADR-041 事务与认证流程事件 | PG18/MySQL8.4 临时隔离服务各通过 `db_connection transaction_events_rollback_before_commit_and_ignore_after_commit_errors` 与 `auth_database auth_flow_events_share_account_and_refresh_transactions`：提交前拒绝回滚且不发 after event；提交后 listener 错误不改变已提交返回；注册校验拒绝不建账户；泛化登录失败事件触发；登录成功事件失败不留 refresh token | direct after-commit 通知是尽力语义；不验证 Twilio 等外部副作用；当前新增四项双库服务验收已执行，完整 workspace 门禁在本功能簇最终提交前执行 |
| 双库与读写路由 | PG18/MySQL8.4 真实数据库迁移/事务/隔离；真实复制暂停、primary scope 隔离与追平 | 串行 SELECT 1 微基准不是业务性能证明；不等于完整集群 |
| M3 认证/安全 | 两库 auth_database/auth_runtime 每库各 6 项；新生成 auth 项目格式/check/真实 TCP auth_flows；Mailer/Policy 生成器双库项目 check/test | 不包含业务流程扩展点或跨节点完整场景 |
| Mail/SMS | Mailpit v1.32.0 本机 SMTP 实收；Twilio transport 协议/错误/超时/no retry；模板 check/test | 没有实际 Twilio 账号投递或送达证据 |
| 加密 | AES-256-GCM-SIV 已知向量、轮换/AAD/篡改/配置脱敏、生成项目 | 算法标准化不等于 crate 实现经过完整独立审计；发布前复核审计证据和平台性能 |
| 审计/签名 | 双库同事务审计提交/回滚/失败回滚/清理，签名用途/TTL/轮换/共享撤销，生成 auth 流程 | 不承诺审计不可篡改、远端 sink 可靠投递或 JWT access token 即时撤销 |
| 队列基础/唯一键与 worker 停机 | 双库并发领取、事务派发、失败/手动重试、超时后继续处理、强杀进程后 lease 恢复、活动唯一键并发/同事务去重与终结释放；另以 PG18/MySQL8.4 共享隔离库真实子进程 SIGTERM 验证合作 handler 完成确认、不合作 handler 有界超时并释放 lease、同队列 pending job 不再领取、worker pool 不可用时传播错误并保留 lease；接管过期 lease 后旧 token 不能确认新 worker 结果 | 数据库不可达通过关闭 worker pool 模拟连接故障，未中断数据库服务本身；生产多实例完整集群仍未覆盖 |
| 有限任务链/批次 | 双库任务顺序、停止/继续、并发上限、事务回滚与进度测试已有记录 | 单个持久 parent job；重试可重跑全部子任务，不提供逐子任务独立 checkpoint |
| Scheduler | 两库独立进程同 slot 竞争、提交后异常退出去重、once 和 SIGTERM；UTC interval/daily 边界 | 触发去重不是副作用 exactly-once；不是生产负载/完整集群验收 |

## 尚未关闭的门禁

- 目录收尾通过 5-package/265-test-list 对比、workspace 门禁、双库 server 集成、生成项目及实际 hello TCP HTTP smoke；当时保留 `apps/server` 有明确评估证据，现按 ADR-062 迁入根 package。ADR-039 源码交付已另行通过干净副本和双库验收。以上不等同于 v1 集群或性能门禁。
- M4 共享锁与请求幂等已按 ADR-042 完成 PostgreSQL 18/MySQL 8.4 双 pool 隔离库验收；Worker 停机/lease 修正、F1 日历调度、F2 国际化、F3 邮件模板与 M4 通知/生成组合均以 `todo.md` 中的专项记录为准，已完成项不重复列为未实现。旧活跃范围为 M5 可观测性与生产资源；原 M6 包括开发编排、生成器/测试工具、正式文档、源码升级和完整产品集群验收。
- PostgreSQL/MySQL 负载均衡下至少两个 API 与多 worker/scheduler 的共享认证状态、配额/幂等、任务竞争、跨节点在线投递、共享文件、滚动升级及故障恢复。
- [底层补齐 F1–F10](../plans/v1-foundation-completion.md) 仍须按顺序交付：F1 日历调度已实现并有双库专项证据；邮件模板/i18n/cache、查询/租户/身份/MFA、CSV/XLSX 与 fixtures 各自仍须完成公开入口、真实失败/并发、有效组合和生成验收。
- gRPC 四类真实调用、TLS/mTLS、认证/Status、deadline/流取消/背压、health/reflection、生成项目、双节点滚动与 proto 新旧互通，已按 [ADR-040](../decisions/adr-040-grpc-integration.md) 完成 G1–G3c 验收；依据为当前 `progress.md` 与 `todo.md` 中的真实 TCP、独立 consumer、Envoy 双节点和 workspace 门禁记录。M6 仍须复验其与完整产品集群、滚动升级的组合行为。跨协议验证/上下文、出站策略、迁移互斥与有界退出已有对应完成记录，不再列为待实现；若这些验收证据变化，应同步更新本节与功能事实。
- Redis 与对象存储真实驱动语义；代表性业务负载 p50/p95/p99、吞吐、CPU、内存和资源上限；发布前设定基于实测的回归门槛。
- 独立 consumer 的配置/注册/执行契约，构建与依赖成本，C2/C3 完成后的中文文档独立走查、保留业务的源码升级回归及平台/维护约定；详细验收只在 [补强计划](../plans/v1-performance-and-readiness.md) 维护。本次文档工作不新增代码行为测试记录。
- 暂定最低 PostgreSQL 14+ / MySQL 8.0+ / Redis 6.2+ 的完整承诺路径；日常只用 PG18/MySQL8.4/Redis8，额外矩阵按大跨度/明确兼容风险与发布门禁触发。运行时版本检查通过不等于最低版本功能已全部验证。
- Twilio 实发需要开发者授权账号、凭据与目标；未满足则外部验收保持未通过，不能虚报投递成功。发布/tag/上线仍由独立授权决定。

## 报告规则

每簇报告源码/WIP/本地提交状态、真实执行命令及退出结果、使用的服务与隔离范围、跳过/缺失项。历史记录只作为已有证据，变更影响对应行为时重新运行；无需为文档改动重复全量构建。

无数据库 URL 的 test skip、all-features 编译、fake transport、单机多 App、各自独立数据库的两个进程，都不能代替所声称的真实共享/多节点路径。目录收尾前 CI 的服务作业未包含 queue feature；本簇已在工作区补入，仍须验证实际执行后才可报告 M4 双库 CI 覆盖完整。
