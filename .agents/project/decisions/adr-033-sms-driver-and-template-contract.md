# ADR-033：短信供应商与通信模板 contract

- 状态：Accepted（邮件/SMS 基础与 F3 安全邮件模板已实现；通知队列组合待交付）
- 日期：2026-10-26
- 供应商参考：[Twilio Messages resource](https://www.twilio.com/docs/messaging/api/message-resource) 与 [Twilio REST authentication](https://www.twilio.com/docs/usage/requests-to-twilio)。
- 依据：[ADR-009](adr-009-extensible-capability-drivers.md)、[ADR-011](adr-011-sms-and-communication-baseline.md)、[ADR-031](adr-031-outbound-http-client.md)、[ADR-032](adr-032-request-internationalization.md)

## 背景

本 ADR 制定时，`openrust::mail` 已提供邮件、SMTP、log 与 test-util fake，短信尚待接入；当前实现以对应 `core/` 事实为准。认证验证码由 `VerificationCodes` 生成和验证，发送由调用者负责。通信需可在本地无供应商账号运行、可在测试中捕获、可由外部 crate 替换；生产错误配置不能悄悄变成一次成功的 log 发送。

## 决策

1. 邮件与短信是两个公开领域入口，分别由 `Mailer`/`MailDriver` 与 `SmsSender`/`SmsDriver` 表达；不建立万能消息 driver。`Email::message` 与 `SmsMessage` 只描述一次发送，不隐式排队或重试。
2. `SmsMessage` 支持 E.164 收件号码、纯文本或供应商模板 SID 加 JSON 参数；其 `Debug` 对号码、正文和模板参数脱敏。`SmsSender` 返回供应商接受/拒绝结果，不承诺最终送达；不提供自动重试。
3. 两种 driver 都可由公开构造器创建并以 `Mailer::new` / `SmsSender::new` 注册，第三方适配不需改框架。内置 log driver 只记录渠道及“已接收”事件，不记录地址、号码、正文、验证码或模板参数；test-util fake 可捕获消息供测试断言。
4. 新项目默认为 `log`，但 `log` 只允许在非 production profile 使用；production 配置选择 log 时启动失败。SMTP 保持 `mail-smtp` feature 和默认强制 STARTTLS；仅允许非生产、无认证、loopback 主机显式关闭 STARTTLS，以连接本地 Mailpit。生产环境始终要求 STARTTLS。短信供应商 feature `sms-twilio` 显式启用，并依赖已有 `http-client` feature，不增加依赖。
5. Twilio 使用 HTTPS Messages REST form POST，通过 HTTP Basic auth 的 API key SID/secret，路径携带 Account SID；内容只发送 Body 或 MessagingServiceSid + ContentSid/ContentVariables。请求使用统一 `HttpClient` 默认超时/响应大小上限；不自动重试，因为超时无法确认远端是否已受理。仅 2xx 表示已受理，供应商拒绝映射为稳定的脱敏发送错误。凭据 Debug 脱敏。
6. `Email` 和 `SmsMessage` 的内容由调用者明确准备；`LocalizedEmail` 使用 ADR-032 `I18n` catalog 与显式 `Locale` 渲染 subject/body keys 和命名参数，短信文本同样显式翻译，provider templates 参数独立传入。后台任务必须显式给 locale，不读取 task-local 或全局隐式状态。不为通信再造一套 i18n 消息语言或 catalog；邮件布局渲染按下述补强与同一国际化服务整合。
7. 真实供应商 HTTP 协议用本机 TCP/注入 transport 验证请求格式、Basic auth、成功及错误/超时映射，不等同于真实账户发送或最终运营商送达。实发仅在开发者提供已授权目标和凭据后执行；否则记录缺失证据。
8. M4 队列接入后由队列层定义异步、持久失败与重试；非幂等供应商调用的超时可能造成重复发送，不承诺 exactly-once。
9. F3 采用可选 `mail-templates` feature 与 MiniJinja 2.24；只接受 `include_str!` 嵌入的应用模板，不开任意文件/网络 loader。locale 精确项、同语言项、配置回退项按优先级组合，允许继承布局；HTML 自动转义、缺变量严格失败，执行燃料与输出有界。主题仍由共享 `I18n` 翻译，纯文本与可选 HTML 组成 MIME alternative；附件、`Email` 和 driver 复用 lettre/Mailer。生成器创建中英模板与注册包装，现有目录不覆盖。队列重试与通知固定内容/模板版本在后续通信队列簇实现。

## 验收边界

- MailerPlugin / SmsPlugin 默认本地可构建；production log 配置启动失败；缺少 feature 或必要凭据时错误包含配置键但不含值。
- SMTP 真实协议仍由已配置 SMTP 服务验收；Mailpit 本地路径由 Compose 可复跑验收；仅 `localhost`/loopback IP 且无 SMTP 认证时可显式关闭 STARTTLS，production 拒绝此配置。
- Twilio adapter 由 fake/local HTTP server 覆盖 URL、form 字段、认证、2xx/4xx/5xx、超时与秘密脱敏；实际投递凭据和目标未提供前不得记录为已实发。
- 认证调用端继续只负责验证码业务；本簇不改验证码消费、账户创建或 AuthFlows 流程。
- 新增邮件 renderer 须验证 HTML 注入、路径/缺参错误、语言回退、纯文本/HTML multipart、现有附件、fake 断言、本机 Mailpit 实收及生成项目 check/test；队列集成再做双库验证。本条是待执行验收，不代表本轮已投递或已开发。
