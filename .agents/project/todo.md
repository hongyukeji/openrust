# 项目待办

仅列未完成事项；已完成范围和验收边界见[当前交接](current.md)及对应主题记录。

## 用户认证入门套件

- [ ] **API-A01 完整用户套件（已确认／待实施）**：用户已确认按[正式决策及执行批次](plans/user-auth-starter-kit.md)交付完整网页、用户API、账户中心及可选认证接线。先核对安装与既有公开契约，再按依赖实现和验收；计划落盘及底层已有模块均不能算套件完成，不推送或发布。

## 语言标识统一迁移

- [ ] **BCP 47 实际环境升级（部署者执行）**：本仓本地 `.env` 已升级；其他部署环境配置、应用自有语言资源／设置／数据库／旧邮件通知载荷仍须先备份、盘点和冲突审阅。只读 SQL 与默认 ROLLBACK 的三库事务已交付并在隔离实例验证，未自动操作开发／生产数据。按[三语升级说明](../../docs/zh-Hans/digging-deeper/localization.md#bcp47-upgrade)排空旧任务、清旧缓存、同步部署、重建缓存并重启。需实际环境授权和应用专用映射后执行；Windows／正式域名及真实供应商仍按下方条件验收。

## Laravel 对齐后续边界

- [ ] **全仓对齐实施计划（2026-11-22，按完整对话重做）**：接续入口为[开发计划](plans/framework-parity-remediation-2026-11-22.md)（含26条需求对照、调研、核对记录及历史审计全文）和[统一数据](plans/framework-parity-data-2026-11-22.json)（193条任务、14张原始审计表及快照）；原188行方法映射保留，覆盖完整API裁决、38模块、19包与R／X／E差异。保留用户扩展；BCP 47本地迁移已实施，接续受影响回归与实际环境升级边界。用户已授权接手全部差异，正在实施；已补基础类型、部分manager/锁/进程/URL/译器接口并分批验收。实际结果与未完成项见开发计划“当前实施与验收”和统一数据，不能以本批部分通过代替全量关闭。已失效的Agent提示词按用户要求删除；任务及验收全部完成后统一归档本批材料。

L01–L22本批实现和实际验收见[审计修复节](audits/laravel-code-audit-2026-11-21.md#2026-11-21-修复与重新验收)。以下不混入已完成结论，不重开历史扩展R1–R6。

- [ ] **完整API候选核对**：以[全仓声明裁决队列（audit_tables.api_review_queue）](plans/framework-parity-data-2026-11-22.json)及[原名候选（audit_tables.api_candidates）](plans/framework-parity-data-2026-11-22.json)接续初审，按固定Framework v13.40.0核对所属类型、签名、生命周期、已有原名、宏、trait和Rust适配；Validation的PHP规则／全部PCRE选项与日期语法、完整TestResponse、其余Query／ORM接口逐项裁决，不能以关键词扫描认定缺陷或全部支持。原始计数属于旧快照，当前结论须重新核对。
- [ ] **Reverb完整用户登录协议**：已有`userRoutes`／`authenticateUser`与真实HTTP签名回归，但服务端完整`pusher:signin`客户端处理尚未实现。这是实现缺口，不是仅缺外部凭据；需协议实现和真实客户端握手／拒绝／重连验收，不能由HTTP签名端点代替。
- [ ] **Fortify偶发Session失败复现**：本批隔离MySQL的accounts_http曾一次返回Storage失败，同二进制连续12轮、四扩展后续整批、PG／MySQL完整生成应用均通过。尚无可靠根因，保留`fortify-stability.log`及原失败批；重现时保留SQL／连接和Session日志，不把未定位事件写成已修复生产缺陷。
- [ ] **PCRE2补丁上游跟进**：现有0.2.11本地补丁只开放原生DOLLAR_ENDONLY编译选项，上游提供等价builder方法后移除本地补丁，并复验D／mD与三语支持矩阵。实际分发继续复核MIT／Unlicense选择和notice。

## 自定义驱动与翻译来源扩展（2026-11-25）

本节来自用户对“支付／短信／翻译能否像 Laravel 文件存储一样扩展驱动”的复核，属于代码与公开契约补齐，不是等待供应商账号的验收事项。本次只读核对源码和既有测试并登记待办，未实现或重跑消费者；不得据此标记已完成。接手时先复查最新源码及固定上游，按项目原生命名和既有模块职责设计；“可配置多个名称”不等于“可注册外部实现”。

- [ ] **DRV-01 文件系统自定义驱动（待实现，优先基础接缝）**
  - **当前证据：** [FilesystemManager](../../vendor/openrust/framework/src/filesystem/filesystem_manager.rs)的具名磁盘解析仅分支处理local／s3；[FilesystemAdapter](../../vendor/openrust/framework/src/filesystem/filesystem_adapter.rs)内部Driver也是封闭枚举。已有Filesystem／Cloud／Factory契约及临时URL回调，尚不能据此认定Storage已支持通用自定义后端注册。
  - **处理范围：** 对照固定Laravel FilesystemManager的自定义工厂和适配层职责，补外部后端可承载的适配接缝及注册／解析入口。保留Storage、Factory、FilesystemAdapter的既有公开返回类型和行为；不能只加extend注册表，最终仍要求调用方修改内部枚举。保持local／S3的路径保护、权限、流式操作、错误策略和临时URL行为。
  - **验收：** 从独立消费crate实现测试后端，在应用Provider注册，经真实Storage具名磁盘入口完成读写、删除及错误传播；覆盖未知驱动、配置错误、同驱动多配置、实例复用、替换后缓存失效、应用隔离及配置缓存前后。云／临时URL等可选能力不支持时明确返回错误，不伪装成功。

- [ ] **DRV-02 Pay自定义支付驱动（待实现，可与DRV-01独立推进）**
  - **当前证据：** [PayManager／Gateway](../../vendor/openrust/pay/src/manager.rs)固定匹配Wechat／Alipay，没有公开自定义工厂注册；多商户profile目前仅复用这两个实现。此项归独立包openrust-pay，不放进应用安装残留或核心框架的平行支付模块。
  - **处理范围：** 提供可由外部crate实现的支付契约与应用实例内工厂注册，打通Pay::driver到Gateway的实际消费者；核对[公共输入／结果与VerifiedNotification](../../vendor/openrust/pay/src/types.rs)对外部实现的可构造性和信任边界，不通过直接开放全部私有验签字段绕过既有校验。保留内置微信／支付宝、订单／金额／场景、密钥快照、错误及回调语义；不顺带承诺新币种、服务商或新支付产品。
  - **验收：** 外部测试驱动经Provider注册，使用原Pay入口执行支付、查询、关闭、退款及回调，明确不支持操作；内置协议正反例继续通过。保持DatabaseNotifications的去重、行锁、业务回调及完成标记在同一事务边界；涉及共享存储的改动按项目三库规则实测。未知驱动、工厂失败重试、具名隔离、替换／旧句柄、配置缓存、HTTP与独立Artisan均需覆盖。

- [ ] **DRV-03 翻译默认Loader可替换性（待核对并补实现）**
  - **当前证据：** [Loader契约](../../vendor/openrust/framework/src/contracts/translation/loader.rs)、Lang::load_from及DatabaseLoader已有自定义来源导入能力；[Translator](../../vendor/openrust/framework/src/translation/translator.rs)及[TranslationServiceProvider](../../vendor/openrust/framework/src/translation/translation_service_provider.rs)仍具体依赖FileLoader。只替换容器中的dyn Loader绑定，不能保证默认Lang查询使用该实现；不应把显式预加载记为默认加载器可替换。
  - **处理范围：** 按固定Laravel Loader／Translator／Provider契约核定默认来源替换、文件与数据库组合、缺失／回退及缓存的接线。Rust同步查询与异步Loader的边界须明确，保留既有Lang::load_from批量原子发布、FileLoader句柄及命名空间／JSON／复数行为，不偷偷阻塞异步运行时或每次翻译都发网络请求。不为表面统一强造Lang::driver；本节指语言词条加载，不包含机器翻译供应商API。
  - **验收：** 独立crate实现Loader并经正式装配后，让真实HTTP、Artisan及Worker中的Lang查询实际消费它；覆盖locale／fallback、命名空间与JSON、文件覆盖、加载失败和并发更新冲突、缓存与应用隔离。复验已有十八语默认资源和数据库加载三库行为，不能只测直接调用自定义Loader。

- [ ] **DRV-04 Cashier扩展范围核定（单列跟踪，不等同Pay实现）**
  - **当前证据：** [CashierManager／Client](../../vendor/openrust/cashier/src/client.rs)使用固定Stripe／Paddle Provider，没有任意第三方后端的通用extend；不能把其具名账户配置说成开放式订阅驱动。
  - **处理范围：** 先核对固定Cashier上游的产品专用契约，明确本轮开放的是后端适配、传输替换还是新增订阅供应商；形成可审阅的范围与兼容方案后再实现已核定部分。不因文件存储可扩展就合并Pay与Cashier、替换既有Provider枚举公开语义或重写计费引擎。任何新增供应商能力须另列真实账单／回调／状态机验收，不借用Stripe／Paddle旧记录计通过。

- [ ] **DRV-05 短信既有扩展消费者与三语示例补齐（复用已实现能力）**
  - **已有能力：** [SmsDriver／SmsManager::extend](../../vendor/openrust/framework/src/notifications/sms/sms_manager.rs)及[自定义驱动测试](../../vendor/openrust/framework/tests/sms_manager.rs)已存在，覆盖实例复用、显式配置刷新、失败重试、工厂替换及应用隔离。此项不是新增短信驱动引擎，也不以Sms门面没有同名extend就判为无法扩展。
  - **处理与验收：** 在[三语短信指南](../../docs/zh-Hans/digging-deeper/sms.md#custom-sms-providers)补外部SmsDriver实现、Provider解析Manager注册工厂、具名配置及Sms::driver调用的完整可编译示例，并实测HTTP与独立Artisan／队列Worker的装配。替换工厂／修改配置后，沿用forget_drivers与旧SmsSender保留快照的既有规则；常驻进程须获得自己的注册，不序列化闭包或依赖父进程内存。

**共同交付标准：** 每项保持既有公开契约和应用隔离；注册、解析、执行和错误路径必须由实际公开消费者验证。优先复用现有契约／Manager／Provider，不新增全局注册表，不把SDK、存储适配与业务流程重新混入模型。支持范围及Rust适配写入相关三语正文和逐项验收记录，通过相应格式／严格Clippy与相关回归后逐批本地提交、清理本批临时产物，不推送。DRV-01至DRV-03可按文件归属独立推进，复用统一的生命周期验收要求；DRV-04先核定边界，DRV-05复用现有实现。真实供应商资金、凭据、投递与正式环境验证继续归下节，不能用模拟驱动通过消除这些未验边界。

## 真实服务验收（等待条件，不纳入本轮本地收尾）

2026-11-21 用户确认真实服务验收目前无法执行，待条件具备后继续。以下项目均未验收；本地协议模拟、测试容器或浏览器虚拟设备不能替代实际目标结果。每项使用授权的隔离账号和测试数据，记录运行版本、失败恢复及资源收尾。

- [ ] **支付与订阅**：提供微信支付、支付宝、Stripe、Paddle 的授权测试商户、回调地址和适用客户端；验证下单、支付／SCA、订阅变更、退款、签名与重复／乱序通知、最终账单和资金结果。涉及真实资金的操作单独取得授权。
- [ ] **身份登录与生产 SSO**：提供已登记回调的 OAuth／OIDC、微信／QQ／小程序测试应用和受控账号；验证真实授权、撤销、账户归属、会话及受支持的退出边界。Passkeys 使用实际设备／平台验证器，不以虚拟认证器代替真机。
- [ ] **邮件、短信与通知**：提供已验证的发件域名、SMTP／供应商凭据及受控邮箱、号码或 Slack 工作区；逐驱动验证实际投递、回执、TLS、拒绝与重试，确认最终收件人收到。
- [ ] **AWS 与对象存储**：提供隔离的 S3 bucket、SQS 标准／FIFO 队列、DynamoDB 表及实际 IAM／STS／KMS 权限；验证真实签名、角色授权、对象操作、队列顺序／去重／重试及持久化行为。
- [ ] **托管广播与商业服务**：提供 Pusher 应用／集群及实际商业 embedding 等已承诺驱动的测试端点；验证 HTTPS／WSS 发布订阅、鉴权、重连、错误传播和供应商实际响应。
- [ ] **正式 HTTPS 与文档地址**：提供授权测试域名、受信任证书、反向代理及最终托管地址；核对部署版本、转发、HTTP／Artisan／Worker、正常关停、三语与 API 链接。生产部署、推送、版本发布仍需用户单独授权。

执行范围和历史本地结果见[发布验收账本](audits/core-closure-ledger.md#v010-acceptance)。明确不支持的能力不列作待真实服务验收，按开发者文档的支持边界处理。

## 平台与发布条件

- [ ] **最终 Windows 矩阵**：2026-11-21 已实际启动 Windows 11 ARM64（10.0.26300.9974）、确认 Rust 1.94.0，并尝试 UNC 和映射盘两种项目路径；Parallels 共享文件系统在生成 Rust archive 时返回 `os error 87`，未完成本批构建。需支持 Rust 构建的项目内原生文件系统环境，再验 Windows ARM64／x64、真实终端及实际 Docker Desktop／Compose／Buildx 后端；不得改用项目外目录绕过产物规则。
- [ ] **最终分发批准**：确定发布版本与目标硬件、复核实际交付物的依赖许可和升级说明，完成用户验收后，再按明确授权改版本、打 tag、推送、创建 Release 或部署。当前 `0.0.0` 仍为开发版本，本地验收不代表已发布。
