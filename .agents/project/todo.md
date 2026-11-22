# 项目待办

## 语言候选自动发现（2026-11-23）

默认HTTP启动后从FileLoader应用资源发现候选，显式setLocale（含空列表）优先；控制器共用LocaleSelection，欢迎页两个菜单分支已移除三语硬编码。母语名称精确读取、缺少显示代码，长菜单滚动及RTL元信息可用。新增规范标签语言资源后重启，无需修改控制器或菜单；纯数据库语言显式配置。没有增加18种正式译文或其文档。

179次定向Rust执行（93框架＋76应用＋10有状态API）、24组真实HTTP Chromium、fmt、workspace/all-targets严格Clippy及三语内容／构建／双前缀浏览器／全导航通过。17个技术交付文件与验收快照一致，详见[验收与边界](audits/03-07/welcome-language-2026-11-22.md#语言候选自动发现2026-11-23)。无本轮剩余实现阻塞；其他引擎／实机、Windows／Docker／性能及完整workspace测试未验。用户随后授权本轮本地提交，不含推送；保留并行改动。

## 默认语言名称统一为 locale（2026-11-23）

应用与框架缺省query／偏好Cookie统一locale，保留Accept-Language、既有环境覆盖、自定义lang及null语义；复用POST /locale和store_locale。旧lang不默认读取，升级重新选择、更新书签并重建配置缓存。覆盖此前默认lang约定，历史记录保留当时边界。

Rust1.94.0定向215次执行（131框架＋74应用＋10有状态API，0失败／忽略）、fmt、workspace/all-targets严格Clippy、25组真实HTTP Chromium及三语内容／构建／浏览器／导航通过。31个语言相关交付文件与快照一致；日志模块后续协作修改不计本轮通过。详见[验收与迁移](audits/03-07/welcome-language-2026-11-22.md#默认语言名称统一为-locale2026-11-23)。无本范围剩余实现阻塞；完整workspace测试、其他引擎／实机、Windows／Docker／性能未验。用户随后授权本轮本地提交，不含推送或发布；无关协作改动继续保留。


## Blade 原生 Rust 代码块（2026-11-23）

[计划](plans/blade-native-rust-blocks.md)范围已实现并按[记录](audits/03-08/native-rust-blocks-2026-11-23.md)验收；无剩余实现阻塞。Windows／release／Docker／性能／持久化异步队列仍为未验范围，不计入完成结论。用户随后授权本次本地提交，不含推送或发布。

## 请求语言选择与欢迎页切换

- [x] **自动跟随／固定语言（2026-11-23）**：POST／CSRF加密Cookie保存，恢复自动删除；URL仅临时覆盖，偏好与实际语言分离；默认管线单次选择、无状态／有状态API、配置缓存、私有缓存与三语说明闭环。208项Rust回归、10项guard、23组最终HTTP浏览器、fmt／严格Clippy与三语文档检查通过，见[验收边界](audits/03-07/welcome-language-2026-11-22.md#自动跟随与持久手动偏好2026-11-23)。实机／其他引擎／Windows／Docker／压测未验；账号和独立客户端持久化属于应用接入范围。


- [x] **请求偏好与当前应用语言职责复核（2026-11-23）**：保留APP_LOCALE_HEADER环境支持，明确Request标准偏好／SetLocale配置协商／App当前语言，修正三语正文与计划。125项定向回归、44组HTTP浏览器、fmt／严格Clippy及三语检查通过，见[证据及未验边界](audits/03-07/welcome-language-2026-11-22.md#请求偏好与应用语言职责边界2026-11-23)。无本次提交／推送授权；无关数据库协作WIP未计通过。

- [x] **可配置语言请求头**：app.locale_header默认Accept-Language／缺键同默认／合法自定义头／null仅关闭头识别；callback→query→头→兜底，标准Request API与缓存不混用，Vary使用实际头名。最终123项定向测试、44组真实HTTP浏览器、fmt／workspace严格Clippy及三语文档全部检查通过，边界见[验收记录](audits/03-07/welcome-language-2026-11-22.md#可配置语言请求头2026-11-23)。本轮未授权暂存、提交或推送。

- [x] **URL语言参数与欢迎页手动切换**：默认lang、可配置名称／null关闭、回调→query→浏览器、非法输入400、精确候选与请求隔离已实现；三语普通GET／自动控件、参数保留和候选过滤已实现。范围131项回归、36组真实浏览器、fmt／严格Clippy与三语文档检查通过，组合在途源码复验和边界见[实际记录](audits/03-07/welcome-language-2026-11-22.md#url-语言参数与欢迎页切换2026-11-22)。不代表全仓其他在途任务完成；用户已授权本轮本地提交，未推送／发布。


## 语言标识统一迁移

- [x] **BCP 47 全项目迁移的本地实施与验收**：默认三语统一为 `en`／`zh-Hans`／`zh-Hant`，配置／API／资源／文档目录及 URL、Faker／Number／邮件通知／Worker／生成应用一起迁移；三库升级脚本、三语文档、浏览器、独立部署及 Docker 实际验收通过。支持边界和失败记录见[开发方案](plans/locale-bcp47.md#11-实际实施与验收2026-11-22)及[验收记录](audits/03-07/locale-bcp47-2026-11-22.md)，不将外部目标或跳过计为通过。
- [x] **本地配置和 Seeder 收尾**：本地 `.env` 三个语言值已迁至 zh-Hans，其他内容及权限保留；隔离副本通过 Artisan、冷／缓存HTTP8场景。复用已有隔离数据库机制，完整71项Seeder连续三轮正常并发通过，并核对测试数据库回收；fmt、严格Clippy及42项应用回归通过。见[收尾证据](audits/03-07/locale-bcp47-2026-11-22.md#后续收尾2026-11-22)。
- [ ] **BCP 47 实际环境升级（部署者执行）**：本仓本地 `.env` 已升级；其他部署环境配置、应用自有语言资源／设置／数据库／旧邮件通知载荷仍须先备份、盘点和冲突审阅。只读 SQL 与默认 ROLLBACK 的三库事务已交付并在隔离实例验证，未自动操作开发／生产数据。按[三语升级说明](../../docs/zh-Hans/digging-deeper/localization.md#bcp47-upgrade)排空旧任务、清旧缓存、同步部署、重建缓存并重启。需实际环境授权和应用专用映射后执行；Windows／正式域名及真实供应商仍按下方条件验收。

## Laravel 对齐后续边界

- [ ] **全仓对齐实施计划（2026-11-22）**：直接接续[开发计划](plans/framework-parity-remediation-2026-11-22.md)和[统一数据](plans/framework-parity-data-2026-11-22.json)。已失效的Agent提示词按用户要求删除；任务及验收全部完成后统一归档本批材料。

L01–L22本批实现和实际验收见[审计修复节](audits/laravel-code-audit-2026-11-21.md#2026-11-21-修复与重新验收)。以下不混入已完成结论，不重开历史扩展R1–R6。

- [ ] **完整API候选核对**：初审`api-candidates.json`尚未逐方法判定的候选，继续按固定Framework v13.40.0核对已有原名、宏、trait和Rust适配；Validation的PHP规则／全部PCRE选项与日期语法、完整TestResponse、其余Query／ORM接口单独界定范围，不能以关键词扫描认定缺陷或全部支持。
- [ ] **Reverb完整用户登录协议**：已有`userRoutes`／`authenticateUser`与真实HTTP签名回归，但服务端完整`pusher:signin`客户端处理尚未实现。这是实现缺口，不是仅缺外部凭据；需协议实现和真实客户端握手／拒绝／重连验收，不能由HTTP签名端点代替。
- [ ] **Fortify偶发Session失败复现**：本批隔离MySQL的accounts_http曾一次返回Storage失败，同二进制连续12轮、四扩展后续整批、PG／MySQL完整生成应用均通过。尚无可靠根因，保留`fortify-stability.log`及原失败批；重现时保留SQL／连接和Session日志，不把未定位事件写成已修复生产缺陷。
- [ ] **PCRE2补丁上游跟进**：现有0.2.11本地补丁只开放原生DOLLAR_ENDONLY编译选项，上游提供等价builder方法后移除本地补丁，并复验D／mD与三语支持矩阵。实际分发继续复核MIT／Unlicense选择和notice。

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

## 已完成

- [x] **数据库动态设置扩展**。提供通用后台设置底座，网站／SEO、地区显示、邮件、短信、存储、上传、账号安全、通知及第三方服务等按组接入，允许业务模块扩展。优先借鉴 Spatie Laravel Settings，沿用 Laravel 配置命名，采用类型化设置、单表仓储、敏感值加密和明确的请求／任务生效边界；后台表单与权限属于应用层。S1 通用底座已实现并通过三库、实际入口及三语文档验收；后台产品、权限策略及具体服务适配属于应用层按需接入，不作为本扩展完成条件。见[开发记录](plans/database-settings.md)及[验收记录](audits/settings/README.md)。

- [x] **数据库翻译扩展**。沿用 Spatie `language_lines` 多语言 JSON 结构，实现显式 DatabaseLoader、文件覆盖和 Lang 原子发布；Rust1.94＋PostgreSQL18 的 8 项行为／HTTP 测试、4 示例、严格全feature Clippy／fmt与三语12＋6浏览器通过。精确语言回退和缺表错误适配见 [验收与调研](audits/translation-database/README.md)。

- [x] **06-08 Artisan 交互本地化**。命令列表、帮助、参数错误与生产确认复用应用翻译器。默认使用 `APP_LOCALE`，应用翻译缺失时按 `APP_FALLBACK_LOCALE` 回退。命令名、选项、退出码与机器可读输出结构沿用现有契约。 [基础验收](audits/06-08/README.md) · [生产确认验收](audits/06-08/production-confirmation.md)

- [x] **欢迎页页头与Blade语言链接（2026-11-23）**：移除language_links控制器函数，候选／编码链接／母语名称及状态全部由模板处理；语言名称独立JSON，页头主题默认跟随系统。96项定向回归、fmt、严格Clippy、36组真实HTTP浏览器及三语文档构建／浏览器／导航通过，交付文件与快照一致；边界及证据见[主题记录](audits/03-07/welcome-language-2026-11-22.md#欢迎页页头母语名称与-blade-链接生成2026-11-23)。用户已授权本轮本地提交，不包含推送或发布。


- [x] **语言候选配置（2026-11-23）**：通过app.supported_locales／APP_SUPPORTED_LOCALES控制候选，缺省／null自动发现，空列表停用，显式代码优先；复用启动快照与配置缓存，三语文档同步。验收与范围见[主题记录](audits/03-07/welcome-language-2026-11-22.md#语言候选配置2026-11-23)。用户已授权本轮本地提交，不包含推送。
