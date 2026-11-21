# 项目待办

## Laravel 对齐后续边界

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
