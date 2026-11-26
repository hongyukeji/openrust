# 项目待办

集中列出已发现的未完成事项及条件性后续；已完成范围和验收边界见[当前交接](current.md)及对应主题记录。本页不是全框架审计已经结束的声明，接手时仍须按最新源码和逐项账本核对。

## 双机接续顺序（2026-11-26）

本轮暂停开发。恢复后，开发机按下表接续源码和测试夹具，验收机按下一表接收固定提交。两台机器可并行处理不同批次，同一候选由一台机器维护。交接时记录提交 SHA、目标或 feature、测试命令、工具链和服务版本。尚未提交的工作区改动与 `target/tmp/` 候选只在当前机器可用，须先按原交接记录恢复候选并交接，再由另一台机器验收。验收失败回原批次修正，修正后用新 SHA 复验。

### 开发顺序

| 顺序 | 开发机任务 | 可并行和前置条件 |
| --- | --- | --- |
| 1 | 按 [Artisan 与 Controllers 交接](#artisan-controllers-handoff)恢复可选 Tinker，再接续 Controllers 的批量单例、URI 动词及剩余锚点。 | 先核对保留的源码快照、已提交基线和固定上游对照，按候选分别交接。 |
| 2 | 按各自账本继续 Webhook、内容审核和观测扩展剩余契约。 | Webhook 可靠接收接缝是内容审核相关回调工作的前置。Pulse、Telescope 的独立工作可与第 1 项并行。 |
| 3 | 接续[全仓 Laravel 对齐](#laravel-对齐后续边界)的 Validation、TestResponse、Query 和 ORM 裁决。 | 可独立交付的组件分批推进，公共接口依赖固定上游裁决和共享模块的已固定提交。 |

### 验收顺序

| 顺序 | 验收机任务 | 输入和通过条件 |
| --- | --- | --- |
| 1 | 从已提交源码并行恢复跨后端与平台专项。 | [Webhook 恢复事件](#webhook-recovery-handoff) `cb8151df1` 的 SQLite 已通过，补 PostgreSQL／MySQL、Redis 队列和双 Scheduler。[Telescope 调度诊断](#observability-handoff) `5f0b2a769` 补 PostgreSQL／MySQL。Docker／Lima 就绪后，依冻结 SHA 跑较新 Linux ARM64 生产制品及 Linux 调度用户矩阵。 |
| 2 | 验收开发机交接的候选，到一批验一批。 | Tinker 跑真实 PTY、单次求值、默认关闭和部署烟测。Controllers 跑上游对照、真实 TCP、`route:list` 和三语。细项与命令见各自交接。 |
| 3 | 选定最终交付快照后，执行 [REL-V01](#平台与发布条件)及完整平台矩阵。 | 固定源码、依赖、feature 与制品，运行组合回归、迁移、启动关停及许可核对。已验旧制品的结果只适用于对应快照。 |
| 4 | [真实供应商与部署验收](#external-acceptance)。 | 授权账号、实际端点和平台齐备后按各条执行。BCP 47 实际迁移由目标环境部署者按备份和冲突审阅流程独立推进。发布须完成最终分发批准。 |

## 待认领的缺口

2026-11-26 核对本机 Codex 对话后，以下范围尚未确认有对话负责。认领前先核对当前交接、最新源码和相关对话，避免重复进行已验收或正在执行的批次。认领后在对应条目记录对话名称、承接范围和外部条件，完成后补验收证据。

| 待认领范围 | 接手入口与完成条件 |
| --- | --- |
| 全仓 Laravel 对齐收口 | 从 [Laravel 对齐后续边界](#laravel-对齐后续边界)的统一账本和公开 API 裁决队列开始，复核 Validation 的 PHP 规则、PCRE 与日期行为，以及 TestResponse、Query 和 ORM 剩余入口，逐项记录裁决及实际消费者结果。PCRE2 补丁等上游提供等价入口后再做迁移复验。`ART-01` 已由 `OpenRust · Framework` 推进。 |
| 最终交付快照与发布回归 | 接手 [REL-V01 和最终分发批准](#平台与发布条件)。选定交付组合后固定源码、依赖和目标平台，对该快照跑全量及业务端到端回归，核对依赖许可和升级说明。打 tag、推送、发布及部署按当次授权执行。 |
| 最终平台与正式部署 | 接手 [最终 Windows 矩阵](#平台与发布条件)中的 Windows x64、实际 Docker Desktop／Compose／Buildx，以及[正式 HTTPS 与文档地址](#真实服务验收等待条件不纳入本轮本地收尾)。按交付制品验证启动、关停、反向代理和文档链接。当前九扩展 Windows ARM64 制品由 `OpenRust · Plan · Webhook ...` 推进，最终快照仍需复验。 |
| 其余真实供应商验收 | 从[真实服务验收](#真实服务验收等待条件不纳入本轮本地收尾)接手支付与订阅、邮件／短信／Slack、AWS 与对象存储、托管 Pusher，以及[供应商专用能力](#供应商专用能力)的 New Relic 摄取、NRQL 和页面实测。授权账号、资源及目标端点具备后，按各驱动验收并记录结果。 |
| BCP 47 实际部署迁移 | 由目标环境部署者承接[语言标识统一迁移](#语言标识统一迁移)。取得环境授权及应用专用映射后，先盘点并审阅冲突，再备份并按升级说明执行，记录各环境结果。 |

业务容量、跨主机性能压测、数据库与 Redis 集群扩容性能及 Laravel／Loco 性能对比按用户决定暂缓。现有功能回归和故障恢复验收照常按各自条目执行。

## 用户认证入门套件

- [x] **API-A01 完整用户套件（本地契约已闭合）**：A01-01至05实现及本地可控验收完成，证据／清理与外部发布条件见[最终记录](audits/user-auth-starter/README.md#a01-final-closure)。真实供应商、实体设备及完整部署矩阵单列；不推送／发布。

## 语言标识统一迁移

- [ ] **BCP 47 实际环境升级（部署者执行）**：本仓本地 `.env` 已升级；其他部署环境配置、应用自有语言资源／设置／数据库／旧邮件通知载荷仍须先备份、盘点和冲突审阅。只读 SQL 与默认 ROLLBACK 的三库事务已交付并在隔离实例验证，未自动操作开发／生产数据。按[三语升级说明](../../docs/zh-Hans/digging-deeper/localization.md#bcp47-upgrade)排空旧任务、清旧缓存、同步部署、重建缓存并重启。需实际环境授权和应用专用映射后执行；Windows／正式域名及真实供应商仍按下方条件验收。

## Laravel 对齐后续边界

- [ ] **全仓对齐实施计划（2026-11-22，按完整对话重做）**：接续入口为[开发计划](plans/framework-parity-remediation-2026-11-22.md)（含26条需求对照、调研、核对记录及历史审计全文）和[统一数据](plans/framework-parity-data-2026-11-22.json)（193条任务、14张原始审计表及快照）；原188行方法映射保留，覆盖完整API裁决、38模块、19包与R／X／E差异。保留用户扩展；BCP 47本地迁移已实施，接续受影响回归与实际环境升级边界。用户已授权接手全部差异，正在实施；已补基础类型、部分manager/锁/进程/URL/译器接口并分批验收。实际结果与未完成项见开发计划“当前实施与验收”和统一数据，不能以本批部分通过代替全量关闭。已失效的Agent提示词按用户要求删除；任务及验收全部完成后统一归档本批材料。

L01–L22本批实现和实际验收见[审计修复节](audits/laravel-code-audit-2026-11-21.md#2026-11-21-修复与重新验收)。以下不混入已完成结论，不重开历史扩展R1–R6。

- [ ] **完整API候选核对**：以[全仓声明裁决队列（audit_tables.api_review_queue）](plans/framework-parity-data-2026-11-22.json)及[原名候选（audit_tables.api_candidates）](plans/framework-parity-data-2026-11-22.json)接续初审，按固定Framework v13.40.0核对所属类型、签名、生命周期、已有原名、宏、trait和Rust适配；Validation的PHP规则／全部PCRE选项与日期语法、完整TestResponse、其余Query／ORM接口逐项裁决，不能以关键词扫描认定缺陷或全部支持。原始计数属于旧快照，当前结论须重新核对。
- [ ] **ART-01 Artisan固定教程剩余入口（实施中／整章未闭合）**：接续[48节清单](audits/artisan-tutorial/section-map.json)和[逐簇验收](audits/artisan-tutorial/README.md)。签名／声明式依赖、CommandInput、Isolatable ID／期限／取消与六存储验证已有已验批次，不重复实现。继续核对缺失输入提示、问答／表格／进度、生成／注册、信号、事件／Dev等未闭合入口，区分未实现与已有但未验；每项从正式命令声明和实际消费者实测HTTP、独立Artisan、三语直接示例及严格门禁。Tinker仍Planned，不能计作已有REPL；Sail／PHP生态及Rust交互环境须单独核定，不暗中引入PHP运行时。保留当前其他对话的Console在途改动，不用早期簇的未完成列表覆盖后续已验结果。
- [x] **Reverb 用户登录协议**：`userRoutes`／`authenticateUser` 与 `pusher:signin`、套接字身份、定向投递及重连已通过真实 HTTP、Artisan、Echo、HTTPS 和两节点 Redis 验收。托管 Pusher 实例另按发布范围核对。[验收记录](audits/08-02/README.md#user-sign-in)
- [x] **Fortify 偶发 Session 失败复现**：旧 HTTP 测试夹具在共享 MySQL schema 删建固定会话表，复现了缺表 `1146 / 42S02` 和 HTTP 500。改用每个应用独占的 schema，三个相关测试组及跨进程回归均通过。[复现与验收](audits/laravel-code-audit-2026-11-21.md#fortify-session-夹具隔离)
- [ ] **PCRE2补丁上游跟进**：现有0.2.11本地补丁只开放原生DOLLAR_ENDONLY编译选项，上游提供等价builder方法后移除本地补丁，并复验D／mD与三语支持矩阵。实际分发继续复核MIT／Unlicense选择和notice。

## 自定义驱动与文本翻译能力（2026-11-25）

本节来自用户对“支付／短信／翻译能否像 Laravel 文件存储一样扩展驱动”的复核，属于代码与公开契约补齐，不是等待供应商账号的验收事项。初次只读审计登记后，以下各项按最新实现和验收更新。接手时先复查最新源码及固定上游，按项目原生命名和既有模块职责设计；“可配置多个名称”不等于“可注册外部实现”。用户随后明确：本节“翻译”指把输入文本翻译为指定语言的机器翻译能力，要求像短信／存储一样支持服务商配置与自定义驱动；不是Lang语言词条查询或Loader来源替换。此前误记的Loader问题移至DRV-06独立跟踪，不替代此需求。

本轮从顺序2开始持续开发，当前实现、回归、真实消费者与未验边界见[驱动扩展验收](audits/driver-extensions/README.md)；本节初始只读结论由各项最新记录补充。

- [x] **DRV-01 文件系统自定义驱动（2026-11-25 定向验收闭合，真实供应商发布边界保留）**
  - **已完成：** [FilesystemManager](../../vendor/openrust/framework/src/filesystem/filesystem_manager.rs)和Storage已提供extend／build_from_config；[FilesystemAdapter](../../vendor/openrust/framework/src/filesystem/filesystem_adapter.rs)通过custom承载外部Cloud／Filesystem实现，不要求下游修改内部枚举。具名并发缓存、失败重试、工厂替换与应用隔离、路径保护和流式取消均通过实际消费者；详见[验收](audits/driver-extensions/README.md)。
  - **处理范围：** 对照固定Laravel FilesystemManager的自定义工厂和适配层职责，补外部后端可承载的适配接缝及注册／解析入口。保留Storage、Factory、FilesystemAdapter的既有公开返回类型和行为；不能只加extend注册表，最终仍要求调用方修改内部枚举。保持local／S3的路径保护、权限、流式操作、错误策略和临时URL行为。
  - **验收：** 从独立消费crate实现测试后端，在应用Provider注册，经真实Storage具名磁盘入口完成读写、删除及错误传播；覆盖未知驱动、配置错误、同驱动多配置、实例复用、替换后缓存失效、应用隔离及配置缓存前后。云／临时URL等可选能力不支持时明确返回错误，不伪装成功。

- [x] **DRV-02 Pay 自定义支付驱动（本地交付完成）**
  - **已完成：** `PayManager::extend`、`PayDriver` 和 `DriverNotification` 接入原 `Pay`／`Gateway` 入口；独立消费者、微信与支付宝协议、三库真实 HTTP／Artisan、事务及缓存通过。见[支付验收](audits/pay/README.md#custom-drivers)。真实商户与资金验收仍在下文发布边界。
  - **处理范围：** 提供可由外部crate实现的支付契约与应用实例内工厂注册，打通Pay::driver到Gateway的实际消费者；核对[公共输入／结果与VerifiedNotification](../../vendor/openrust/pay/src/types.rs)对外部实现的可构造性和信任边界，不通过直接开放全部私有验签字段绕过既有校验。保留内置微信／支付宝、订单／金额／场景、密钥快照、错误及回调语义；不顺带承诺新币种、服务商或新支付产品。
  - **验收：** 外部测试驱动经Provider注册，使用原Pay入口执行支付、查询、关闭、退款及回调，明确不支持操作；内置协议正反例继续通过。保持DatabaseNotifications的去重、行锁、业务回调及完成标记在同一事务边界；涉及共享存储的改动按项目三库规则实测。未知驱动、工厂失败重试、具名隔离、替换／旧句柄、配置缓存、HTTP与独立Artisan均需覆盖。

- [x] **DRV-03 文本机器翻译与七个内置驱动（本地交付完成）**。命名、分批迁移及验收见[本地化与文本翻译拆分计划](plans/localization-and-text-translation.md)。
  - **已完成：** [TR-01、TR-02A／B／C](audits/localization-and-text-translation/README.md#tr-01-文本翻译底座)均已交付。独立包、外部驱动契约、七个驱动的协议验收和三语文档齐备。LibreTranslate 另通过本地真实模型翻译。
  - **需求与现状：** 接收原文与目标语言，返回翻译后的文本；源语言显式指定或按服务商能力自动识别。现有[localization 模块](../../vendor/openrust/framework/src/localization/mod.rs)负责语言资源查询、替换及复数选择，[Loader](../../vendor/openrust/framework/src/contracts/localization/loader.rs)负责词条来源。文本翻译通过独立包的 Translate 门面调用。
  - **处理范围：** 按拆分计划交付 DeepL、LibreTranslate、Azure、Google Cloud、腾讯云、阿里云及百度翻译开放平台，分批适配和逐家验收。提供异步文本翻译契约、具名服务商配置、默认驱动解析、外部crate可实现的驱动与应用Provider工厂注册；明确原文／译文、源／目标语言及供应商错误语义。支持同驱动多份配置、应用隔离和可预测的实例复用／配置刷新，复用现有HTTP Client、配置与容器机制；不预先引入复杂翻译平台，也不把网络翻译隐式接入Lang／__／Blade词条查询。
  - **验收：** 独立消费crate实现自定义驱动，经正式公开入口把文本翻译为指定语言；实测HTTP、独立Artisan和Worker装配及配置缓存前后。覆盖未知驱动、配置错误、空文本、Unicode、语言标签与不支持语言、服务商超时／限流／错误响应、工厂失败重试、同驱动具名隔离、替换／旧句柄和多应用隔离。对未实现的自动识别／批量等可选能力明确边界；模拟服务商验收记录协议与装配覆盖，真实供应商凭据就绪后另验实际译文和错误，不把模拟成功当成真实翻译效果。同步三语用法及验收记录，不改既有Lang公开契约。

- [x] **DRV-04 Cashier 具名账户 HTTP 扩展**：对照固定 Stripe／Paddle 上游后核定现有 HTTP Factory 为扩展入口，支持账户中间件与测试传输；Stripe／Paddle 的计费语义继续由现有契约处理。实际 HTTP、Artisan、账户隔离和客户端缓存回归通过。[范围与验收](audits/pay/cashier.md#account-http)

- [x] **DRV-05 短信既有扩展消费者与三语示例补齐（2026-11-25 三语、HTTP／Artisan及三库双队列闭合）**
  - **已有能力：** [SmsDriver／SmsManager::extend](../../vendor/openrust/framework/src/notifications/sms/sms_manager.rs)及[自定义驱动测试](../../vendor/openrust/framework/tests/sms_manager.rs)已存在，覆盖实例复用、显式配置刷新、失败重试、工厂替换及应用隔离。此项不是新增短信驱动引擎，也不以Sms门面没有同名extend就判为无法扩展。
  - **处理与验收：** 在[三语短信指南](../../docs/zh-Hans/digging-deeper/sms.md#custom-sms-providers)补外部SmsDriver实现、Provider解析Manager注册工厂、具名配置及Sms::driver调用的完整可编译示例，并实测HTTP与独立Artisan／队列Worker的装配。替换工厂／修改配置后，沿用forget_drivers与旧SmsSender保留快照的既有规则；常驻进程须获得自己的注册，不序列化闭包或依赖父进程内存。

- [x] **DRV-06 语言资源默认 Loader 可替换性**。L10N-01／L10N-02 已交付。按应用 Provider 注册 `lang.loader` 并配置 `lang.resources`，普通 Lang 查询使用启动时发布的来源。显式刷新保留运行时覆盖，支持失败回退。数据库包已迁名，三库 HTTP／Artisan／Worker 与配置缓存通过。见[拆分验收](audits/localization-and-text-translation/README.md#l10n-02-默认-loader-与数据库词条)。

**共同交付标准：** 每项保持既有公开契约和应用隔离；注册、解析、执行和错误路径必须由实际公开消费者验证。优先复用现有契约／Manager／Provider，不新增全局注册表，不把SDK、存储适配与业务流程重新混入模型。支持范围及Rust适配写入相关三语正文和逐项验收记录，通过相应格式／严格Clippy与相关回归后逐批本地提交、清理本批临时产物，不推送。DRV-01至DRV-03可按文件归属独立推进，复用统一的生命周期验收要求；DRV-04先核定边界，DRV-05复用现有实现，DRV-06单独处理语言资源装配。真实供应商资金、凭据、投递与正式环境验证继续归下节，不能用模拟驱动通过消除这些未验边界。

## Webhook独立扩展（持续实施中／完整收发未闭合）

- [ ] **WH-00 Webhook接收／发送独立扩展与成熟完整能力（实施中）**：接续[Webhook开发计划](plans/webhooks.md)。用户要求独立实现Spatie Webhook能力并完整补齐计划；以固定Spatie Client／Server全部公开类型／配置为对照，分别规划openrust-webhook-client接收与openrust-webhook-server发送。WC-01至WC-13、WS-01至WS-12及其他公开入口均须逐项裁决／验收，含原始载荷／附件、多配置路由、自定义验证器／Signer／Profile／Response／Job／Backoff、同步／异步／条件派发、TLS／mTLS／代理、事件、可靠持久化／去重／重试／重放、端点／订阅、限流／清理／可观察性。按WH-00至WH-07实施，WH-V01至WH-V27验收；Svix等外部后端／FIFO／独立UI另列WH-08条件性后续，不冒充原生已完成。内容审核复用接收包而非再建inbox，Pay／Cashier事务与公开契约保留。七项Webhook审计欠缺已按WD-01至WD-07补齐决定：信任分类／Deferred、当前端点许可、统一领取令牌、有界原子扇出、人工重放代次／预算及首次发送安全门禁，详见计划第13节。用户另明确客户端与服务端均须同一套实现支持单机和集群，计划第14节已明确共享数据库／Queue／锁／限流／Storage、双拓扑与故障／扩缩容验收，集群不是WH-08后续项；完整收发与双拓扑验收仍未完成。用户现已启用持续目标并授权实现；两包配置、签名、退避、扩展契约、四种启用组合及HTTP／Artisan／三库Worker基础已验，详见[实施记录](audits/webhooks/README.md)及[逐类型清单](audits/webhooks/contracts.json)。手动Processor、按范围隔离的原文接收记录与独立Worker读取已验，见[接收记录批次](audits/webhooks/receiver.md)；[公开路由与具名工厂](audits/webhooks/routes.md)已通过三库HTTP／Artisan／Worker及缓存门禁；[解析载荷和数据库附件读取](audits/webhooks/payload.md)已接入；[原生初始化](audits/webhooks/installation.md)及[迁移发布](audits/webhooks/migration-publication.md)已接入；[已安装应用双协议收发](audits/webhooks/installation.md#installed-roundtrip)通过原生 HTTP、Artisan 和独立 Worker；附件跨主机共享磁盘及远端驱动／[原生处理 Job](audits/webhooks/jobs.md)剩余生命周期门禁与跨后端恢复。已接入[同库原子入队](audits/webhooks/atomic-enqueue.md)和[接收 Worker 租约](audits/webhooks/processing-lease.md)，并经三库并发进程与崩溃重领验收。已接入[已验签事件去重](audits/webhooks/verified-event-deduplication.md)。[同库持久 Deferred](audits/webhooks/deferred-processing.md)已接入。[逐次领取审计和旧 Worker 写回隔离](audits/webhooks/processing-attempts.md)已验，已接入[关联就绪主动唤醒](audits/webhooks/deferred-wakeup.md)。已接入[无稳定 ID 的摘要窗口去重](audits/webhooks/body-deduplication.md)和[接收记录保留期清理](audits/webhooks/retention.md)和[通用模型清理入口](audits/webhooks/retention.md#model-prune)均已接入，另补[逐实例清理钩子和失败报告](audits/webhooks/retention.md#prunable)与[批量清理及 PostgreSQL 分区边界](audits/webhooks/retention.md#mass-prunable)。已接入[无签名查单提示](audits/webhooks/unverified-hints.md)，Hive 领域消费者及[有界提示清理](audits/webhooks/unverified-hints.md#lookup-retention)已验，多主机仍待验收。已接入[验签判定审计与清理](audits/webhooks/verification-evidence.md)。已接入[私有 Storage 附件引用](audits/webhooks/attachment-storage.md)及[过期对象清理与暂存补偿](audits/webhooks/attachment-cleanup.md)。[接收提交后的持久派发恢复](audits/webhooks/dispatch-recovery.md)已接入。[入列后丢失任务的有界补派](audits/webhooks/processing-recovery.md)已接入。[跨队列后端的持久业务延期](audits/webhooks/durable-deferred-recovery.md)已接入。[耗尽诊断与受控接收重放](audits/webhooks/manual-replay.md)已接入。[发送 Builder 快照与 HTTP 固定目的地](audits/webhooks/send-builder.md)已接入。[发送持久领取与首次安全投递](audits/webhooks/first-delivery.md)已接入；[Builder 派发和发送事件](audits/webhooks/dispatch-events.md)已接入；[出站定时恢复](audits/webhooks/outbound-recovery.md)已接入；[具名发送 Job 扩展](audits/webhooks/custom-job.md)已接入；[错误契约与持久序列化](audits/webhooks/errors.md)已核对，并补发送 Job 未注册时的拒绝门禁；[出站人工重放](audits/webhooks/outbound-replay.md)已接入；[端点当前版本许可](audits/webhooks/endpoints.md)已接入；[出站共享限额](audits/webhooks/delivery-limits.md)已接入；[订阅有界扇出](audits/webhooks/subscription-fanout.md)已接入；[出站 TLS 与客户端身份](audits/webhooks/outbound-tls.md)已接入；[具名 SOCKS5 出站代理](audits/webhooks/outbound-proxy.md)已接入；[出站诊断](audits/webhooks/outbound-diagnostics.md)已接入；[出站快照与尝试留存](audits/webhooks/outbound-retention.md)已接入；[共享发布正文回收](audits/webhooks/publication-retention.md)已接入；[出站业务保留](audits/webhooks/retention-hold.md)已接入；[幂等键与持久 ID](audits/webhooks/idempotency-identity.md)已分离；[幂等键到期释放](audits/webhooks/idempotency-expiry.md)已接入；[投递与发布历史回收](audits/webhooks/history-retention.md)已接入；[重放审计保留期](audits/webhooks/replay-retention.md)已接入；[固定地址 HTTP CONNECT](audits/webhooks/http-connect.md)已接入；[HTTPS CONNECT 独立信任](audits/webhooks/https-connect.md)已接入；[出站代理映射、排除列表与 IPv6](audits/webhooks/proxy-map.md)已验三库，外部代理及固定代理的明文 HTTP 目标仍开放；[出站事件的传输统计和失败类别](audits/webhooks/transport-events.md)已验三库，进程崩溃补记和外部拓扑单列；[可信旧版附件恢复](audits/webhooks/legacy-attachments.md)验证独立列优先和载荷回退，在线 JSON 保持原语义；[单次投递传输诊断](audits/webhooks/transport-diagnostics.md)补持久查询、迟到写回隔离与前版 Worker 接续；[接收 Worker 显式终态](audits/webhooks/processing-terminal-control.md)覆盖原生失败控制、成功保持与重复领取；[接收 Job 原生终态回调](audits/webhooks/processing-failed-hook.md)验证终态时机、当前重放许可和记录身份；[接收摘要和处理尝试分页](audits/webhooks/receipt-diagnostics.md)通过 HTTP、Artisan 和 Worker 三库验收；[已验签接收额度](audits/webhooks/verified-rate-limit.md)已验证数据库和 Redis 双节点共享、隔离、429 及缓存故障；[原生处理限流与并发锁](audits/webhooks/processing-middleware.md)已验证跨 Worker 释放和重领；[出站在途上限](audits/webhooks/delivery-concurrency.md)已在三库验证范围／端点竞争、预算回滚及崩溃恢复；[接收模型更新时间](audits/webhooks/receipt-update-time.md)补旧行、异常和 Worker 时间边界；[接收结果观察](audits/webhooks/receipt-observation.md)提供无敏感值的分类与耗时；[接收补派积压预览](audits/webhooks/dispatch-backlog.md)可按范围只读查询；[出站恢复积压](audits/webhooks/outbound-backlog.md)覆盖补派和收尾候选；[受信 HTTP 收件端固定 CONNECT](audits/webhooks/connect-http-origin.md)通过三库、代理 TLS 和 IPv6 验收；[自定义发送 Job 设置](audits/webhooks/job-options.md)可读取投递快照；[接收配置五类工厂名](audits/webhooks/config-factory-names.md)已有类型化读取及实际装配验收；原生安装装配已验；[五个目标的依赖许可及 ARM64 Release 镜像核对](audits/webhooks/artifact-licenses.md)已完成，其他发布制品另验；[单机与双节点拓扑](audits/webhooks/topology.md)已验五个本地组合；[共享后端断连](audits/webhooks/backend-interruption.md)已在双节点覆盖 PostgreSQL、MySQL 和 Redis；[共享 S3 附件](audits/webhooks/shared-object-storage.md)已通过同主机双节点验收；[双节点调度与授权故障](audits/webhooks/cluster-ownership.md)已验 Scheduler 退出、旧领取回归及端点和凭据失效；[两个实际版本滚动升级](audits/webhooks/rolling-upgrade.md)已在同主机补验；继续核对真实跨主机、托管对象存储、其余集群故障门禁和代理映射；不擅自部署／付费调用，每批实测HTTP／Artisan／Worker、三语及验收记录、本地提交，不推送。

## 内容安全与内容审核（实施中）

- [ ] **CM-00 文本／图片／视频内容审核（四家已确认，实施中）**：接续[内容安全开发计划](plans/content-moderation.md)。[工程核对和外部驱动验收](audits/content-moderation/README.md)已记录。[四家协议复验](audits/content-moderation/four-provider-recheck.md)覆盖12条文本／图片／点播路径并记录真实项目条件。用户已确认国内阿里云＋腾讯云、海外Sightengine＋Hive全部接入，并再次明确外部自定义驱动是必交付能力：独立crate实现公开契约，经Provider注册与具名配置走同一调用通道，无需修改框架枚举；验收包含部分能力、公开结果构造及异步任务跨进程恢复。其余采用计划建议：独立可选包、Manager／Provider／外部自定义驱动、强类型异步契约，复用HTTP Client／Storage／Queue，分离审核与发布，未完成／失败保持待审，Review交人工。按CM-00落实工程细节，再按CM-01至CM-05分批实施，CM-V01至CM-V19按共享／逐家能力分项验收；不能将Hive降为备选、只做文本就关闭全媒体范围或以共享Manager测试代替四家消费者。七项审计欠缺已按D-01至D-07补齐决策：配置／凭据版本绑定、可靠回调接收与提前通知补偿、父子任务终态、四家必需能力闭合、凭据引用与输出保护、有限限流／查询预算及批次顺序，详见计划第8节；用户随后要求独立Webhook扩展，接收基础按WH计划复用，CM-03依赖可靠接收接缝，联合消费者不设循环前置。持续目标已开始实施。外部驱动底座和阿里云／腾讯云文本及图片的本地协议验收已记录，Sightengine 与 Hive 的文本和视觉图片已通过本地协议验收。Hive 独立 OCR 已补验，阿里云、腾讯云及 Sightengine 点播已完成本地协议验收。四家驱动共用的[凭据挂载与修订](audits/content-moderation/README.md#credential-revisions)已通过缓存、HTTP、Artisan 和独立 Worker；[固定配置修订查单](audits/content-moderation/README.md#profile-revisions)已补验。[父审核与点播检查同库落单](audits/content-moderation/README.md#review-storage)已通过三库 HTTP、Artisan 和独立 Worker；[点播领取、送审与失联补偿](audits/content-moderation/README.md#submission-lease)已通过三库。[查单租约、持续预算和必检覆盖](audits/content-moderation/README.md#query-budget)已实测；[Scheduler 定时补派](audits/content-moderation/README.md#review-recovery)已通过数据库／Redis 队列；[Sightengine 账户回调验签与通用接收](audits/content-moderation/README.md#sightengine-callback-signature)以及[提前回调延期、关联和原任务查单](audits/content-moderation/README.md#sightengine-callback-association)已通过三库。[Hive V2 点播提交](audits/content-moderation/README.md#hive-video-submission)及[显式 V1 查单](audits/content-moderation/README.md#hive-video-summary)已通过三库本地协议验收，[点播 OCR 与语音](audits/content-moderation/README.md#hive-video-modalities)也已补验；[腾讯云 VM 点播回调](audits/content-moderation/README.md#tencent-video-callback)和[阿里云增强版点播回调](audits/content-moderation/README.md#aliyun-video-callback)已通过三库。[Hive 无签名查单提示](audits/content-moderation/README.md#hive-lookup-hint)已通过三库。[复合审核多项必检](audits/content-moderation/README.md#review-groups)已通过三库。[单项点播取消](audits/content-moderation/README.md#review-cancellation)已补验。[复合审核原子取消](audits/content-moderation/README.md#group-cancellation)已通过三库。[人工裁决与内容修订发布门禁](audits/content-moderation/README.md#review-decision)已通过三库。[查单 Retry-After](audits/content-moderation/README.md#query-retry-after)已通过三库。[共享账户速率](audits/content-moderation/README.md#account-rates)已通过三库。[账户在途租约](audits/content-moderation/README.md#account-in-flight)已通过三库。[Sightengine 短视频语音](audits/content-moderation/README.md#sightengine-short-speech)已通过三库。[迟到结果诊断](audits/content-moderation/README.md#late-video-notices)已通过三库。[Sightengine 长视频可信分段覆盖](audits/content-moderation/README.md#sightengine-speech-segments)已通过三库。[内容修订撤回门禁](audits/content-moderation/README.md#revision-withdrawal)已通过三库。[已撤回审核清理及回调关联](audits/content-moderation/README.md#withdrawn-review-purge)已通过三库。Hive 正式账号及公开回调、真实模态覆盖、其它回调及四家真实供应商验收继续推进。缺真实账号或合同的范围单列。机器翻译DRV-03、Lang Loader DRV-06与本计划独立；病毒扫描、PII、哈希、自托管、直播等不作为首版默认依赖。

## 供应商专用能力

- [x] **LOG-S01 New Relic APM 独立扩展及协议模拟验收**，按用户确认的范围闭合。[逐项验收记录](plans/log-supplier-integrations.md#合成业务数据与实际-otlp-模拟验收)
- [ ] **LOG-S01 发布实机验收**，使用授权租户核对 New Relic 摄取、NRQL、APM 与错误和日志页面；[Windows ARM64](plans/log-supplier-integrations.md#windows-arm64-otlp) 已通过原生 OTLP 协议验收；Windows x86_64 发布制品及托管 SQS 身份按发布环境复核。[验收边界](plans/log-supplier-integrations.md#合成业务数据与实际-otlp-模拟验收)

## 条件性后续与已知限制（不是新增功能缺陷）

以下是最近几轮已发现、此前仅留在专项记录的限制索引。当前原任务已验收的范围保持；这些条件项不授权自动扩展任务、启用原型或发布产品。

- **路由编译原型后续评估（仅在继续推进或评估生产接入时）**：原型优化本轮目标已通过，正式Router未替换；[同基线三版记录](audits/route-registration-allocations/compiled-prototype/refinement/README.md)仍保留退化：万条overlap靠后静态优先命中22.08→77.5ns、万条static首次编译573.79→851.88µs、128条static完整注册相对基线增加13.02%。后续若推进，先定位这些代价，再补真实业务命中权重、分散前缀、多种前缀长度／根动态密集、多方法／域名、并发、旧路由缓存冷热、跨平台与长期RSS测量；保持注册／重复覆盖、方法／Allow、约束、默认值、绑定、Unicode／解码、隔离及缓存格式。不得把旧固定负载的零预热分配当成整个HTTP零分配，或直接据此更换生产Router。
- **crates.io等注册表发布（仅在另行确定发布方式后）**：[统一版本管理](audits/workspace-versions/README.md)已完成，21包继承、业务版本独立、三语自动片段及叶子包归一化／独立构建已验；不是待重做的版本同步任务。当前publish=false是保留策略，现有path依赖没有全部声明注册表版本约束，也未验证20个官方包逐包独立发布。若以后选择注册表分发，另行核对依赖版本来源／发布顺序、归档包含文件与许可、工作区元数据归一化及独立消费者，再按当次授权处理；不能仅打开publish或搬出成员目录就称可发布，不要重复手填散落版本号。

<a id="external-acceptance"></a>

## 真实服务验收（等待条件，不纳入本轮本地收尾）

2026-11-21 用户确认真实服务验收目前无法执行，待条件具备后继续。以下项目均未验收；本地协议模拟、测试容器或浏览器虚拟设备不能替代实际目标结果。每项使用授权的隔离账号和测试数据，记录运行版本、失败恢复及资源收尾。

- [ ] **支付与订阅**：提供微信支付、支付宝、Stripe、Paddle 的授权测试商户、回调地址和适用客户端；验证下单、支付／SCA、订阅变更、退款、签名与重复／乱序通知、最终账单和资金结果。涉及真实资金的操作单独取得授权。
- [ ] **身份登录与生产 SSO**：提供已登记回调的 OAuth／OIDC、微信／QQ／小程序测试应用和受控账号；验证真实授权、撤销、账户归属、会话及受支持的退出边界。Passkeys 使用实际设备／平台验证器，不以虚拟认证器代替真机。
- [ ] **邮件、短信与通知**：提供已验证的发件域名、SMTP／供应商凭据及受控邮箱、号码或 Slack 工作区；逐驱动验证实际投递、回执、TLS、拒绝与重试，确认最终收件人收到。
- [ ] **AWS 与对象存储**：提供隔离的 S3 bucket、SQS 标准／FIFO 队列、DynamoDB 表及实际 IAM／STS／KMS 权限；验证真实签名、角色授权、对象操作、队列顺序／去重／重试及持久化行为。
- [ ] **托管广播与商业服务**：提供 Pusher 应用／集群及实际商业 embedding 等已承诺驱动的测试端点；验证 HTTPS／WSS 发布订阅、鉴权、重连、错误传播和供应商实际响应。
- [ ] **文本机器翻译发布验收**：使用授权测试账号和端点逐家验证 DeepL、腾讯云、阿里云、百度、Azure、Google Cloud 的实际译文、账号／地区／模型权限、源语种识别、额度／计费和超时。LibreTranslate 已验 1.9.6 en→es 本机模型，部署所用模型、远端认证和 TLS 仍随实际环境复核。见[真实环境验收](audits/localization-and-text-translation/README.md#真实环境验收)。
- [ ] **正式 HTTPS 与文档地址**：提供授权测试域名、受信任证书、反向代理及最终托管地址；核对部署版本、转发、HTTP／Artisan／Worker、正常关停、三语与 API 链接。生产部署、推送、版本发布仍需用户单独授权。

执行范围和历史本地结果见[发布验收账本](audits/core-closure-ledger.md#v010-acceptance)。明确不支持的能力不列作待真实服务验收，按开发者文档的支持边界处理。

## 平台与发布条件

- [ ] **REL-V01 最终交付快照与项目生产回归（待选定交付组合后验收）**：先锁定具体提交、依赖、目标平台和实际业务依赖，再对该组合执行相关全量回归及业务端到端验收。[2026-11-23全量记录](audits/project-validation-2026-11-23.md)对应固定提交的21包403目标，不能外推为随后所有修改或当前未提交工作区都通过。按项目实际数据库／缓存／Worker组合验证HTTP与独立Artisan、迁移与缓存、服务重启／正常关停及失败恢复，记录实测版本和未验边界。容量压测按[暂缓范围](#待认领的缺口)处理。与真实供应商／Windows／最终分发批准配合，不把模拟阶段闭合、功能测试数量或开发版本号当成生产发行结论，也不要求为不使用的可选能力重复造实现。
- [ ] **最终 Windows 矩阵**。Windows 11 ARM64 的 Parallels 共享文件系统曾在生成 Rust archive 时返回 `os error 87`。使用镜像放在项目临时目录的原生 NTFS 验收盘及 ARM64 Clang、CMake 后，Rust 1.94.0 已完成三个观测扩展的定向构建与运行验收。最终交付快照仍需核对 Windows ARM64 和 x64、真实终端、服务启动与关停、实际 Docker Desktop／Compose／Buildx 后端。 [扩展平台记录](audits/extension-parity.md)
- [ ] **最终分发批准**：确定发布版本与目标硬件、复核实际交付物的依赖许可和升级说明，完成用户验收后，再按明确授权改版本、打 tag、推送、创建 Release 或部署。当前 `0.0.0` 仍为开发版本，本地验收不代表已发布。

<a id="webhook-recovery-handoff"></a>

## 顺序 2–10 交接（Webhook 恢复事件与组合制品验收）

- [x] **Webhook 恢复事件实现与 SQLite 验收**。本地提交 `cb8151df1` 已包含 `WebhookCallUnknownEvent`、验收夹具、三语文档和[执行记录](audits/webhooks/dispatch-events.md#恢复尝试的-unknown-事件)。真实 HTTP、Artisan、独立 Worker、SQLite Scheduler 都通过，确认监听器能读取已提交状态，双 Scheduler 只通知一次。扩展包 11 个测试、夹具和扩展包的严格 Clippy、格式检查及三语站点构建均通过。
- [ ] **Webhook 恢复事件跨后端复验**。Docker 引擎恢复后，用项目隔离包装器复跑 PostgreSQL／MySQL 和 Redis 队列组合。夹具可按隔离快照将 `tests/support/webhooks/app.rs` 登记为 `[[example]]`，再执行 `cargo +1.95.0 build --locked --offline --features webhook-client,webhook-server --example webhook-unknown-fixture`，运行 `FIXTURE_SEND_SCHEDULE=1 python3 -B tests/webhooks_send_acceptance.py <夹具>` 和无该变量的同一命令。已有 SQLite 日志与隔离快照位于 `target/tmp/goal-webhook-unknown-observation/`。
- [ ] **较新 Linux 生产 Dockerfile 验收**。冻结源码 `6a7874a3cb04a5319a9fdfa98d28a82d328a3f26` 的首次 ARM64 构建触及 32 GB 停线，续跑因 Docker 引擎持续超时安全结束，尚无该快照的运行镜像。引擎恢复且有足够空间后，经构建守卫续接 `target/tmp/goal-extension-linux-current-release/run.sh`，运行出包、11 份许可逐字节核对、58 项 SQLite 迁移、缓存前后 HTTP、Reverb、独立 Artisan Worker 和 OTLP。两种较早冻结 Linux 镜像、两份 Windows ZIP 和 macOS ARM64 新归档的已验结果在[发行验收记录](audits/extension-release-licenses.md)，不得外推为该 Linux 构建已验。
- [ ] **实机与外部条件**。原生 Linux x86_64 需已核实的隔离主机登录及非交互 Docker 权限，原生 Windows x64／Docker Desktop、macOS Developer ID 与公证、生产 TLS、托管 SQS 和真实审核／翻译／短信／支付／订阅／New Relic 环境按[顺序 2–10 验收账本](audits/extension-goal-gates.md)分项推进。先确认测试身份、范围、回调和费用授权，再核验真实服务。协议模拟和虚机证据按各自范围保留。
- **已提交及交接状态**。版权打包 `6db5a9be0`，Windows ZIP 实测 `146f7e907`，两种 Linux 冻结候选复验 `fae3a31e1`，macOS 冷构建 `6a7874a3c`，环境就绪复查 `f02caf1b6`，较新 Linux 停点 `fb9ced397`。本工作流的构建与验收均已退出。未推送。日志和构建产物留给汇总工作流，顺序 2–10 未闭合。

<a id="observability-handoff"></a>

## 2026-11-26 观测扩展对齐暂停交接

- **已本地提交。** Telescope 的 Artisan 仓储恢复 `5a51f8358`、历史排除键 `796f6b595`、调度用户和诊断载荷 `3097087b1`。Pulse 的卡片筛选链接 `071ea533b` 经固定 Livewire 3.8.12／4.4.10 和原生桌面／移动验收。Horizon 的集群实现 `12c1459b0`、Linux 自动选主证据 `352a6be6b`／`2cbc59c43`、Linux Supervisor 排空 `3227965d2` 已提交。三扩展同批默认回归 159 项通过。系统用户调度已有 macOS 真正切换到 `nobody`、Redis 锁释放、三库字段验证和 Windows 11 ARM64 上 AMD64 测试程序的三项证据。详细见[对齐账本](audits/extension-parity.md)。
- **调度作业诊断已本地提交。** `5f0b2a769` 修复默认描述及 `command`，沿用作业展示名、显式标题和原调度互斥键。SQLite 实际 Artisan 和 `telescope:show --json` 用例、相关 92 项回归及严格 Clippy 均已通过。Docker Engine 仍超时，恢复后按项目夹具在 PostgreSQL 18／MySQL 8.4 各执行 `cargo test --locked -p openrust-telescope --test provider scheduled_jobs_report_callback_and_display_description -- --exact`。证据位于 `target/tmp/observability-parity/horizon-cluster-reference/telescope-scheduled-jobs-{baseline,fixed,handoff-commit}.log`。
- **Linux 与发布验收。** Docker Engine 在 Rust 调度身份测试期间连续超时。隔离 Debian ARM64 Lima 实例已启动至可执行 `sudo -n -u nobody -- /usr/bin/id -un`，本批 Rust Linux 进程矩阵尚未执行，停工时已关闭实例。恢复后任选已恢复的 Docker 或项目内 `target/tmp/ov-vz` 的 Lima 实例 `ovs`，待依赖就绪后用 Rust 1.95.0 运行 `/acceptance-input/run-scheduler-vz.sh`，验证命令与文件身份、后台钩子、Redis 8.10.3 互斥锁、命令环境及实际 Telescope 持久化。保留项目内镜像和启动日志。Docker 旧构建的宿主进程已退出，Engine 恢复后以全名核对本批旧容器 `openrust-telescope-scheduled-user-8018b2443e4bbd8d-build` 的残留状态。Horizon 集群的目标 Release、实际 Redis 端点、物理 x86_64 及最终业务部署矩阵继续按 REL-V01 验收。Pulse 剩余 Recorder／Livewire、Telescope Watcher 剩余契约仍在各自审计。当前证据不足以标记三个扩展完整对等或生产就绪。
- **交接状态。** 本批宿主构建、定向验收和 Lima 已停止。保留源码、隔离快照、日志和独立制品；后续推送和编译产物清理由汇总对话统一执行。

<a id="artisan-controllers-handoff"></a>

## 固定教程逐章对齐交接（2026-11-26，Artisan REPL 与 Controllers）

- **已收口并本地提交**。Artisan 核心 43 节见[逐节对账](audits/artisan-tutorial/section-map.json)，`212a55c2c` 记录可选 REPL 单列。Controllers 已验 17/30 节，有 1 节部分通过、12 节待验。最近的独立提交为 `9dff257d9`、`80ab58e7e`、`8f0768cf8`、`53502b042`、`6ed6e97e7`、`f94a29999`、`6d80dbb23`、`02dad56ed`。以[控制器账本](audits/controllers-tutorial/section-map.json)确认边界，不重复开发已闭合入口。
- **恢复时优先完成可选 Tinker**。保留已随 `64c37b98f` 提交的[接续计划](plans/tinker-native-repl.md)和 `target/tmp/artisan-tutorial-20261125/tinker-integration/` 源码快照。该快照的 `Cargo.toml`、`Cargo.lock`、`vendor/openrust/tinker/`、三语 Artisan 正文及 `docs/tests/check_artisan_tutorial.mjs` 属于此单元。它包含最后调整的 IDE 求值退出码和本地 CLI 限制。另保留同级 `tinker-artisan-acceptance.py`、证据日志和 `tinker-integration-base.txt`，以及 `target/tmp/logging-tutorial-20261124/source/` 已验证交互消费者的隔离副本。两处快照切勿全量覆盖活跃工作区。
- **Tinker 已有证据与余项**。固定 Evcxr 0.21.1、Rust 1.94.0、macOS ARM64 的真实 `./artisan tinker` 通过了预载、交互持久状态、三语使用示例、容器与 Facade 同一实例、命令允许清单、别名排除后恢复及正常退出清理。SQLite 独立审计恰好一次 INSERT 和一次 UPDATE。启用扩展的实际独立 Artisan、`/up`、首页和 SIGTERM 通过。三语站点及 12 组浏览器检查通过。相关日志为 `tinker-artisan-live.log`、`tinker-http-smoke-enabled.log`、`tinker-artisan-docs-exact.log`。固定引擎的小型 PTY 与 IDE 状态反例通过，不能代替完整应用验收。完整应用 PTY 和最后版单次求值按停止指令取消，默认关闭与独立部署调用仍待验。恢复时用 `tests/build_guard.py` 包装 `tinker-artisan-acceptance.py --source <项目内隔离副本> --engine <项目内 Evcxr 0.21.1> --phase terminal|execute|disabled`，核对真实 Ctrl+C、panic 后可用、临时目录回收、单次求值成功或失败的退出码，以及格式化 panic 前的数据库写入仅一次。再跑最新候选 `cargo fmt --all --check`、`cargo clippy --workspace --all-targets --features tinker --locked -- -D warnings` 和默认部署烟测。通过后更新三语、Artisan 五节账本并单独本地提交。Linux／Windows 动态装载和真机终端随发布核对。既有 `tinker-integration-gates.log` 早于最后两处命令调整。
- **随后继续 Controllers**。保留 `target/tmp/artisan-tutorial-20261125/controller-singleton-source/` 中的 `routing/{router,resource}.rs`、`support/facades.rs`、`controller_singleton_routes.rs`、`singleton-routes-oracle.json`、三语 Controllers 正文及浏览器检查。这批 Router／Facade 批量单例与资源 URI 动词为未验候选。固定框架的真实 PHP Router 已生成 41 条注册记录，日志旁的 `controllers-singletons-native-routes.json` 可复查。先在最新提交基线运行 `cargo test -p openrust-framework --locked --test controller_singleton_routes`，核对 Router、Facade、真实 TCP 和 Artisan `route:list`，再完成三语正式示例、站点及既有资源路由回归。此后核对普通控制器、依赖注入、中间件和授权属性，以及默认嵌套关系推断。依赖注入的固定上游 POST／PUT 返回向量已保存在 `controller-dependency-injection-reference.json`。
- **安全接手**。本次停手只追加此条账本。两批候选均未暂存、未提交。候选源码、独立测试脚本、日志和引擎证据须在清理时保留，可清理归属已确认的编译产物。本对话的终端、求值、Cargo 守卫和计时已停止。推送及编译产物清理由汇总对话统一接手。

## Vendor 目录统一

- [ ] 当前小阶段开发完成并通过验收后，立即按[项目规范](conventions.md#代码与测试目录)将 `vendor/` 目录统一迁移到 `vendor/<Cargo 包名>/`。例如将 `vendor/openrust/ai/` 迁至 `vendor/openrust-ai/`。按各包 `Cargo.toml` 的 `package.name` 确定目录，同步更新工作区成员、路径依赖、构建与发布脚本、测试、项目指引和三语文档中的现行路径。迁移后核对 Cargo 工作区与依赖解析、编译和发布入口。
