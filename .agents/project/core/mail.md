# `openrust::mail` 与 `openrust::sms` 当前事实

> 受众：AI Agent
> 更新：2026-10-26
> 摘要：SMTP/log/fake 与 Twilio adapter 已实现；本机 Mailpit 邮件实收有记录，真实短信投递仍待授权。

## 邮件

- `MailConfig` 定义在 `openrust::config::mailer`，通过 `openrust::mail` 兼容 re-export；SMTP 本地凭据可放 `.env`，production 忽略 `.env`。
- `Email::message` 产出 `lettre::Message`；`Mailer::send` 通过公开 `MailDriver` 提交。
- `LocalizedEmail` 使用 `I18n` catalog、显式 `Locale` 与命名参数渲染纯文本主题/正文；后台任务不依赖请求 task-local locale。
- 可选 `mail-templates` 使用 MiniJinja 2.24；`EmailTemplateCatalog` 只注册 `include_str!` 嵌入式应用资源，不开文件/网络 loader。按 locale 分层覆盖并支持继承布局；HTML 自动转义、严格缺参失败，模板燃料和输出分别限制为 100,000 与 256 KiB；主题拒绝换行且最多 998 字节。
- `RenderedEmail` 生成纯文本/HTML alternative，保留 lettre multipart 附件组合能力；新消息继续经现有 `Email`/`Mailer` driver。`openrust generate mailer` 生成中英模板与注册封装，并为应用启用 feature，不覆盖已有文件。
- `MailerPlugin` 默认注册不记录内容的 `LogMailer`；production 选择 `log` 会启动失败。
- `mail-smtp` 使用 Tokio / Rustls，默认要求 STARTTLS；只有无认证 loopback 开发服务可显式关闭 TLS。SMTP 密码 Debug 脱敏，`test-util` 暴露 `FakeMailer`。

## 短信

- `SmsConfig` 在 `openrust::config::sms`；应用可安装 `SmsPlugin` 获得 `SmsSender`。`SmsMessage` 支持 E.164 文本或供应商 template SID 与参数，号码与内容 Debug 脱敏。
- 公开 `SmsDriver`/`SmsSender::new` 可由第三方驱动扩展；默认 `LogSmsDriver` 不记录 PII，production 选择 log 启动失败；`test-util` 的 `FakeSmsDriver` 仅用于测试。
- `sms-twilio` feature 启用 `http-client` 与 Twilio Messages REST adapter，认证使用 API key SID/secret，凭据配合 account SID 与发送方配置；使用有界超时/响应、不会自动重试。成功表示 Twilio 接受请求，不表示最终送达。
- 邮件与短信文本由调用方显式选择语言；邮件可直接用 `LocalizedEmail`，短信通过 `I18n::translate(locale, ...)` 准备文本；短信 provider templates 使用供应商管理的模板，不被框架本地插值。

## 边界

- 本机 Mailpit v1.32.0 SMTP 投递及收件 API 确认已记录于 [测试基线](../memory/testing.md)；这不代表外部邮箱供应商的真实送达。Twilio 协议测试使用注入 transport，真实账户实发仍需授权凭据与目标。
- 启用 `queue` 后 `openrust::notifications::Notifier` 会将固定邮件/SMS payload 分渠道持久化，复用 `Mailer` / `SmsSender` 和 queue worker retry；locale/template 仅在派发前准备，不因重试重渲染。事务内派发可保证业务变更与多渠道任务原子提交。普通多渠道派发逐项提交，后续插入失败可能留下前序任务。
- 通知 payload 包含收件人和正文但不输出日志，调用方需保护数据库与备份，不放入认证凭据、token 或验证码。队列是至少一次执行，provider 受理与任务确认之间崩溃会重复发送；不提供 exactly-once。Twilio 实际账号投递仍待授权。
- Mail/SMS 发送前后通用业务扩展仍待跨能力功能簇接入；本通知编排不引入 Hooks 系统。
- 认证验证码由 `VerificationCodes` 生成消费，发送仍由调用方选择渠道；本簇不启用短信登录或更改账号流程。

F3 已交付安全模板与生成体验，M4 已接入固定内容的队列通知。SMTP 本机实际投递、Mailpit API 内容及通知队列双库证据见 [测试基线](../memory/testing.md)。
