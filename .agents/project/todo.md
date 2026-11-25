# 项目待办

集中列出已发现的未完成事项及条件性后续；已完成范围和验收边界见[当前交接](current.md)及对应主题记录。本页不是全框架审计已经结束的声明，接手时仍须按最新源码和逐项账本核对。

## 用户认证入门套件

- [ ] **API-A01 完整用户套件（已确认／开发中）**：用户已确认按[正式决策及执行批次](plans/user-auth-starter-kit.md)交付完整网页、用户API、账户中心及可选认证接线。已验双预设安装及基础认证页面；继续邮件／凭据生命周期、Token协议、完整账户中心、可选因素和最终矩阵。各阶段按全部契约闭合，基础单元通过及底层已有模块均不能算套件完成，不推送或发布。

## 语言标识统一迁移

- [ ] **BCP 47 实际环境升级（部署者执行）**：本仓本地 `.env` 已升级；其他部署环境配置、应用自有语言资源／设置／数据库／旧邮件通知载荷仍须先备份、盘点和冲突审阅。只读 SQL 与默认 ROLLBACK 的三库事务已交付并在隔离实例验证，未自动操作开发／生产数据。按[三语升级说明](../../docs/zh-Hans/digging-deeper/localization.md#bcp47-upgrade)排空旧任务、清旧缓存、同步部署、重建缓存并重启。需实际环境授权和应用专用映射后执行；Windows／正式域名及真实供应商仍按下方条件验收。

## Laravel 对齐后续边界

- [ ] **全仓对齐实施计划（2026-11-22，按完整对话重做）**：接续入口为[开发计划](plans/framework-parity-remediation-2026-11-22.md)（含26条需求对照、调研、核对记录及历史审计全文）和[统一数据](plans/framework-parity-data-2026-11-22.json)（193条任务、14张原始审计表及快照）；原188行方法映射保留，覆盖完整API裁决、38模块、19包与R／X／E差异。保留用户扩展；BCP 47本地迁移已实施，接续受影响回归与实际环境升级边界。用户已授权接手全部差异，正在实施；已补基础类型、部分manager/锁/进程/URL/译器接口并分批验收。实际结果与未完成项见开发计划“当前实施与验收”和统一数据，不能以本批部分通过代替全量关闭。已失效的Agent提示词按用户要求删除；任务及验收全部完成后统一归档本批材料。

L01–L22本批实现和实际验收见[审计修复节](audits/laravel-code-audit-2026-11-21.md#2026-11-21-修复与重新验收)。以下不混入已完成结论，不重开历史扩展R1–R6。

- [ ] **完整API候选核对**：以[全仓声明裁决队列（audit_tables.api_review_queue）](plans/framework-parity-data-2026-11-22.json)及[原名候选（audit_tables.api_candidates）](plans/framework-parity-data-2026-11-22.json)接续初审，按固定Framework v13.40.0核对所属类型、签名、生命周期、已有原名、宏、trait和Rust适配；Validation的PHP规则／全部PCRE选项与日期语法、完整TestResponse、其余Query／ORM接口逐项裁决，不能以关键词扫描认定缺陷或全部支持。原始计数属于旧快照，当前结论须重新核对。
- [ ] **ART-01 Artisan固定教程剩余入口（实施中／整章未闭合）**：接续[48节清单](audits/artisan-tutorial/section-map.json)和[逐簇验收](audits/artisan-tutorial/README.md)。签名／声明式依赖、CommandInput、Isolatable ID／期限／取消与六存储验证已有已验批次，不重复实现。继续核对缺失输入提示、问答／表格／进度、生成／注册、信号、事件／Dev等未闭合入口，区分未实现与已有但未验；每项从正式命令声明和实际消费者实测HTTP、独立Artisan、三语直接示例及严格门禁。Tinker仍Planned，不能计作已有REPL；Sail／PHP生态及Rust交互环境须单独核定，不暗中引入PHP运行时。保留当前其他对话的Console在途改动，不用早期簇的未完成列表覆盖后续已验结果。
- [ ] **Reverb完整用户登录协议**：已有`userRoutes`／`authenticateUser`与真实HTTP签名回归，但服务端完整`pusher:signin`客户端处理尚未实现。这是实现缺口，不是仅缺外部凭据；需协议实现和真实客户端握手／拒绝／重连验收，不能由HTTP签名端点代替。
- [ ] **Fortify偶发Session失败复现**：本批隔离MySQL的accounts_http曾一次返回Storage失败，同二进制连续12轮、四扩展后续整批、PG／MySQL完整生成应用均通过。尚无可靠根因，保留`fortify-stability.log`及原失败批；重现时保留SQL／连接和Session日志，不把未定位事件写成已修复生产缺陷。
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

- [ ] **DRV-04 Cashier扩展范围核定（单列跟踪，不等同Pay实现）**
  - **当前证据：** [CashierManager／Client](../../vendor/openrust/cashier/src/client.rs)使用固定Stripe／Paddle Provider，没有任意第三方后端的通用extend；不能把其具名账户配置说成开放式订阅驱动。
  - **处理范围：** 先核对固定Cashier上游的产品专用契约，明确本轮开放的是后端适配、传输替换还是新增订阅供应商；形成可审阅的范围与兼容方案后再实现已核定部分。不因文件存储可扩展就合并Pay与Cashier、替换既有Provider枚举公开语义或重写计费引擎。任何新增供应商能力须另列真实账单／回调／状态机验收，不借用Stripe／Paddle旧记录计通过。

- [x] **DRV-05 短信既有扩展消费者与三语示例补齐（2026-11-25 三语、HTTP／Artisan及三库双队列闭合）**
  - **已有能力：** [SmsDriver／SmsManager::extend](../../vendor/openrust/framework/src/notifications/sms/sms_manager.rs)及[自定义驱动测试](../../vendor/openrust/framework/tests/sms_manager.rs)已存在，覆盖实例复用、显式配置刷新、失败重试、工厂替换及应用隔离。此项不是新增短信驱动引擎，也不以Sms门面没有同名extend就判为无法扩展。
  - **处理与验收：** 在[三语短信指南](../../docs/zh-Hans/digging-deeper/sms.md#custom-sms-providers)补外部SmsDriver实现、Provider解析Manager注册工厂、具名配置及Sms::driver调用的完整可编译示例，并实测HTTP与独立Artisan／队列Worker的装配。替换工厂／修改配置后，沿用forget_drivers与旧SmsSender保留快照的既有规则；常驻进程须获得自己的注册，不序列化闭包或依赖父进程内存。

- [x] **DRV-06 语言资源默认 Loader 可替换性**。L10N-01／L10N-02 已交付。按应用 Provider 注册 `lang.loader` 并配置 `lang.resources`，普通 Lang 查询使用启动时发布的来源。显式刷新保留运行时覆盖，支持失败回退。数据库包已迁名，三库 HTTP／Artisan／Worker 与配置缓存通过。见[拆分验收](audits/localization-and-text-translation/README.md#l10n-02-默认-loader-与数据库词条)。

**共同交付标准：** 每项保持既有公开契约和应用隔离；注册、解析、执行和错误路径必须由实际公开消费者验证。优先复用现有契约／Manager／Provider，不新增全局注册表，不把SDK、存储适配与业务流程重新混入模型。支持范围及Rust适配写入相关三语正文和逐项验收记录，通过相应格式／严格Clippy与相关回归后逐批本地提交、清理本批临时产物，不推送。DRV-01至DRV-03可按文件归属独立推进，复用统一的生命周期验收要求；DRV-04先核定边界，DRV-05复用现有实现，DRV-06单独处理语言资源装配。真实供应商资金、凭据、投递与正式环境验证继续归下节，不能用模拟驱动通过消除这些未验边界。

## Webhook独立扩展（持续实施中／完整收发未闭合）

- [ ] **WH-00 Webhook接收／发送独立扩展与成熟完整能力（实施中）**：接续[Webhook开发计划](plans/webhooks.md)。用户要求独立实现Spatie Webhook能力并完整补齐计划；以固定Spatie Client／Server全部公开类型／配置为对照，分别规划openrust-webhook-client接收与openrust-webhook-server发送。WC-01至WC-13、WS-01至WS-12及其他公开入口均须逐项裁决／验收，含原始载荷／附件、多配置路由、自定义验证器／Signer／Profile／Response／Job／Backoff、同步／异步／条件派发、TLS／mTLS／代理、事件、可靠持久化／去重／重试／重放、端点／订阅、限流／清理／可观察性。按WH-00至WH-07实施，WH-V01至WH-V27验收；Svix等外部后端／FIFO／独立UI另列WH-08条件性后续，不冒充原生已完成。内容审核复用接收包而非再建inbox，Pay／Cashier事务与公开契约保留。七项Webhook审计欠缺已按WD-01至WD-07补齐决定：信任分类／Deferred、当前端点许可、统一领取令牌、有界原子扇出、人工重放代次／预算及首次发送安全门禁，详见计划第13节。用户另明确客户端与服务端均须同一套实现支持单机和集群，计划第14节已明确共享数据库／Queue／锁／限流／Storage、双拓扑与故障／扩缩容验收，集群不是WH-08后续项；完整收发与双拓扑验收仍未完成。用户现已启用持续目标并授权实现；两包配置、签名、退避、扩展契约、四种启用组合及HTTP／Artisan／三库Worker基础已验，详见[实施记录](audits/webhooks/README.md)及[逐类型清单](audits/webhooks/contracts.json)。手动Processor、按范围隔离的原文接收记录与独立Worker读取已验，见[接收记录批次](audits/webhooks/receiver.md)；[公开路由与具名工厂](audits/webhooks/routes.md)已通过三库HTTP／Artisan／Worker及缓存门禁；[解析载荷和数据库附件读取](audits/webhooks/payload.md)已接入；[原生初始化](audits/webhooks/installation.md)及[迁移发布](audits/webhooks/migration-publication.md)已接入；[已安装应用双协议收发](audits/webhooks/installation.md#installed-roundtrip)通过原生 HTTP、Artisan 和独立 Worker；附件跨主机共享磁盘及远端驱动／[原生处理 Job](audits/webhooks/jobs.md)剩余生命周期门禁与跨后端恢复。已接入[同库原子入队](audits/webhooks/atomic-enqueue.md)和[接收 Worker 租约](audits/webhooks/processing-lease.md)，并经三库并发进程与崩溃重领验收。已接入[已验签事件去重](audits/webhooks/verified-event-deduplication.md)。[同库持久 Deferred](audits/webhooks/deferred-processing.md)已接入。[逐次领取审计和旧 Worker 写回隔离](audits/webhooks/processing-attempts.md)已验，已接入[关联就绪主动唤醒](audits/webhooks/deferred-wakeup.md)。已接入[无稳定 ID 的摘要窗口去重](audits/webhooks/body-deduplication.md)和[接收记录保留期清理](audits/webhooks/retention.md)和[通用模型清理入口](audits/webhooks/retention.md#model-prune)均已接入，另补[逐实例清理钩子和失败报告](audits/webhooks/retention.md#prunable)与[批量清理及 PostgreSQL 分区边界](audits/webhooks/retention.md#mass-prunable)。已接入[无签名查单提示](audits/webhooks/unverified-hints.md)，正式领域消费者与多主机仍待验收。已接入[验签判定审计与清理](audits/webhooks/verification-evidence.md)。已接入[私有 Storage 附件引用](audits/webhooks/attachment-storage.md)及[过期对象清理与暂存补偿](audits/webhooks/attachment-cleanup.md)。[接收提交后的持久派发恢复](audits/webhooks/dispatch-recovery.md)已接入。[入列后丢失任务的有界补派](audits/webhooks/processing-recovery.md)已接入。[跨队列后端的持久业务延期](audits/webhooks/durable-deferred-recovery.md)已接入。[耗尽诊断与受控接收重放](audits/webhooks/manual-replay.md)已接入。[发送 Builder 快照与 HTTP 固定目的地](audits/webhooks/send-builder.md)已接入。[发送持久领取与首次安全投递](audits/webhooks/first-delivery.md)已接入；[Builder 派发和发送事件](audits/webhooks/dispatch-events.md)已接入；[出站定时恢复](audits/webhooks/outbound-recovery.md)已接入；[具名发送 Job 扩展](audits/webhooks/custom-job.md)已接入；[错误契约与持久序列化](audits/webhooks/errors.md)已核对，并补发送 Job 未注册时的拒绝门禁；[出站人工重放](audits/webhooks/outbound-replay.md)已接入；[端点当前版本许可](audits/webhooks/endpoints.md)已接入；[出站共享限额](audits/webhooks/delivery-limits.md)已接入；[订阅有界扇出](audits/webhooks/subscription-fanout.md)已接入；[出站 TLS 与客户端身份](audits/webhooks/outbound-tls.md)已接入；[具名 SOCKS5 出站代理](audits/webhooks/outbound-proxy.md)已接入；[出站诊断](audits/webhooks/outbound-diagnostics.md)已接入；[出站快照与尝试留存](audits/webhooks/outbound-retention.md)已接入；[共享发布正文回收](audits/webhooks/publication-retention.md)已接入；[出站业务保留](audits/webhooks/retention-hold.md)已接入；[幂等键与持久 ID](audits/webhooks/idempotency-identity.md)已分离；[幂等键到期释放](audits/webhooks/idempotency-expiry.md)已接入；[投递与发布历史回收](audits/webhooks/history-retention.md)已接入；[重放审计保留期](audits/webhooks/replay-retention.md)已接入；[固定地址 HTTP CONNECT](audits/webhooks/http-connect.md)已接入；[HTTPS CONNECT 独立信任](audits/webhooks/https-connect.md)已接入；[单机与双节点拓扑](audits/webhooks/topology.md)已验五个本地组合；[共享后端断连](audits/webhooks/backend-interruption.md)已在双节点覆盖 PostgreSQL、MySQL 和 Redis；[共享 S3 附件](audits/webhooks/shared-object-storage.md)已通过同主机双节点验收；[双节点调度与授权故障](audits/webhooks/cluster-ownership.md)已验 Scheduler 退出、旧领取回归及端点和凭据失效；[两个实际版本滚动升级](audits/webhooks/rolling-upgrade.md)已在同主机补验；继续核对真实跨主机、托管对象存储、其余集群故障门禁和代理映射；不擅自部署／付费调用，每批实测HTTP／Artisan／Worker、三语及验收记录、本地提交，不推送。

## 内容安全与内容审核（实施中）

- [ ] **CM-00 文本／图片／视频内容审核（四家已确认，实施中）**：接续[内容安全开发计划](plans/content-moderation.md)。[工程核对和外部驱动验收](audits/content-moderation/README.md)已记录。用户已确认国内阿里云＋腾讯云、海外Sightengine＋Hive全部接入，并再次明确外部自定义驱动是必交付能力：独立crate实现公开契约，经Provider注册与具名配置走同一调用通道，无需修改框架枚举；验收包含部分能力、公开结果构造及异步任务跨进程恢复。其余采用计划建议：独立可选包、Manager／Provider／外部自定义驱动、强类型异步契约，复用HTTP Client／Storage／Queue，分离审核与发布，未完成／失败保持待审，Review交人工。按CM-00落实工程细节，再按CM-01至CM-05分批实施，CM-V01至CM-V19按共享／逐家能力分项验收；不能将Hive降为备选、只做文本就关闭全媒体范围或以共享Manager测试代替四家消费者。七项审计欠缺已按D-01至D-07补齐决策：配置／凭据版本绑定、可靠回调接收与提前通知补偿、父子任务终态、四家必需能力闭合、凭据引用与输出保护、有限限流／查询预算及批次顺序，详见计划第8节；用户随后要求独立Webhook扩展，接收基础按WH计划复用，CM-03依赖可靠接收接缝，联合消费者不设循环前置。持续目标已开始实施。外部驱动底座和阿里云／腾讯云文本及图片的本地协议验收已记录，Sightengine 与 Hive 的文本和视觉图片已通过本地协议验收。Hive 独立 OCR 已补验，阿里云、腾讯云及 Sightengine 点播已完成本地协议验收。四家驱动共用的[凭据挂载与修订](audits/content-moderation/README.md#credential-revisions)已通过缓存、HTTP、Artisan 和独立 Worker；[固定配置修订查单](audits/content-moderation/README.md#profile-revisions)已补验。[父审核与点播检查同库落单](audits/content-moderation/README.md#review-storage)已通过三库 HTTP、Artisan 和独立 Worker；[点播领取、送审与失联补偿](audits/content-moderation/README.md#submission-lease)已通过三库。[查单租约、持续预算和必检覆盖](audits/content-moderation/README.md#query-budget)已实测；[Scheduler 定时补派](audits/content-moderation/README.md#review-recovery)已通过数据库／Redis 队列。共享上限、回调及四家真实供应商验收继续推进。缺真实账号或合同的范围单列。机器翻译DRV-03、Lang Loader DRV-06与本计划独立；病毒扫描、PII、哈希、自托管、直播等不作为首版默认依赖。

## 供应商专用能力

- [x] **LOG-S01 New Relic APM 独立扩展及协议模拟验收**，按用户确认的范围闭合。[逐项验收记录](plans/log-supplier-integrations.md#合成业务数据与实际-otlp-模拟验收)
- [ ] **LOG-S01 发布实机验收**，使用授权租户核对 New Relic 摄取、NRQL、APM 与错误和日志页面；Windows 原生、托管 SQS 身份按发布环境复核。[验收边界](plans/log-supplier-integrations.md#合成业务数据与实际-otlp-模拟验收)

## 条件性后续与已知限制（不是新增功能缺陷）

以下是最近几轮已发现、此前仅留在专项记录的限制索引。当前原任务已验收的范围保持；这些条件项不授权自动扩展任务、启用原型或发布产品。

- **路由编译原型后续评估（仅在继续推进或评估生产接入时）**：原型优化本轮目标已通过，正式Router未替换；[同基线三版记录](audits/route-registration-allocations/compiled-prototype/refinement/README.md)仍保留退化：万条overlap靠后静态优先命中22.08→77.5ns、万条static首次编译573.79→851.88µs、128条static完整注册相对基线增加13.02%。后续若推进，先定位这些代价，再补真实业务命中权重、分散前缀、多种前缀长度／根动态密集、多方法／域名、并发、旧路由缓存冷热、跨平台与长期RSS测量；保持注册／重复覆盖、方法／Allow、约束、默认值、绑定、Unicode／解码、隔离及缓存格式。不得把旧固定负载的零预热分配当成整个HTTP零分配，或直接据此更换生产Router。
- **crates.io等注册表发布（仅在另行确定发布方式后）**：[统一版本管理](audits/workspace-versions/README.md)已完成，21包继承、业务版本独立、三语自动片段及叶子包归一化／独立构建已验；不是待重做的版本同步任务。当前publish=false是保留策略，现有path依赖没有全部声明注册表版本约束，也未验证20个官方包逐包独立发布。若以后选择注册表分发，另行核对依赖版本来源／发布顺序、归档包含文件与许可、工作区元数据归一化及独立消费者，再按当次授权处理；不能仅打开publish或搬出成员目录就称可发布，不要重复手填散落版本号。

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

- [ ] **REL-V01 最终交付快照与项目生产回归（待选定交付组合后验收）**：先锁定具体提交、依赖、目标平台和实际业务依赖，再对该组合执行相关全量回归及业务端到端验收。[2026-11-23全量记录](audits/project-validation-2026-11-23.md)对应固定提交的21包403目标，不能外推为随后所有修改或当前未提交工作区都通过。按项目实际数据库／缓存／Worker组合验证HTTP与独立Artisan、迁移与缓存、服务重启／正常关停、失败恢复，并按业务负载目标验证容量；记录实测版本和未验边界。与真实供应商／Windows／最终分发批准配合，不把模拟阶段闭合、功能测试数量或开发版本号当成生产发行结论，也不要求为不使用的可选能力重复造实现。
- [ ] **最终 Windows 矩阵**：2026-11-21 已实际启动 Windows 11 ARM64（10.0.26300.9974）、确认 Rust 1.94.0，并尝试 UNC 和映射盘两种项目路径；Parallels 共享文件系统在生成 Rust archive 时返回 `os error 87`，未完成本批构建。需支持 Rust 构建的项目内原生文件系统环境，再验 Windows ARM64／x64、真实终端及实际 Docker Desktop／Compose／Buildx 后端；不得改用项目外目录绕过产物规则。
- [ ] **最终分发批准**：确定发布版本与目标硬件、复核实际交付物的依赖许可和升级说明，完成用户验收后，再按明确授权改版本、打 tag、推送、创建 Release 或部署。当前 `0.0.0` 仍为开发版本，本地验收不代表已发布。
