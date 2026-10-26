# `openrust::mail` 与 `openrust::sms` 当前事实

> 受众：AI Agent
> 更新：2026-10-26
> 摘要：SMTP/log/fake 与 Twilio adapter 已实现；本机 Mailpit 邮件实收有记录，真实短信投递仍待授权。

## 邮件

- `MailConfig` 定义在 `openrust::config::mailer`，通过 `openrust::mail` 兼容 re-export；SMTP 本地凭据可放 `.env`，production 忽略 `.env`。
- `Email::message` 产出 `lettre::Message`；`Mailer::send` 通过公开 `MailDriver` 提交。
- `LocalizedEmail` 使用 `I18n` catalog、显式 `Locale` 与命名参数渲染纯文本主题/正文；后台任务不依赖请求 task-local locale。
- 当前没有 MiniJinja 或其他邮件模板引擎；纯文本消息插值不等于完整 HTML/模板渲染能力。
- `MailerPlugin` 默认注册不记录内容的 `LogMailer`；production 选择 `log` 会启动失败。
- `mail-smtp` 使用 Tokio / Rustls，默认要求 STARTTLS；只有无认证 loopback 开发服务可显式关闭 TLS。SMTP 密码 Debug 脱敏，`test-util` 暴露 `FakeMailer`。

## 短信

- `SmsConfig` 在 `openrust::config::sms`；应用可安装 `SmsPlugin` 获得 `SmsSender`。`SmsMessage` 支持 E.164 文本或供应商 template SID 与参数，号码与内容 Debug 脱敏。
- 公开 `SmsDriver`/`SmsSender::new` 可由第三方驱动扩展；默认 `LogSmsDriver` 不记录 PII，production 选择 log 启动失败；`test-util` 的 `FakeSmsDriver` 仅用于测试。
- `sms-twilio` feature 启用 `http-client` 与 Twilio Messages REST adapter，认证使用 API key SID/secret，凭据配合 account SID 与发送方配置；使用有界超时/响应、不会自动重试。成功表示 Twilio 接受请求，不表示最终送达。
- 邮件与短信文本由调用方显式选择语言；邮件可直接用 `LocalizedEmail`，短信通过 `I18n::translate(locale, ...)` 准备文本；短信 provider templates 使用供应商管理的模板，不被框架本地插值。

## 边界

- 本机 Mailpit v1.32.0 SMTP 投递及收件 API 确认已记录于 [测试基线](../memory/testing.md)；这不代表外部邮箱供应商的真实送达。Twilio 协议测试使用注入 transport，真实账户实发仍需授权凭据与目标。
- 队列化 Mail/SMS、失败任务联动、薄多渠道通知和发送前后公共扩展仍待按 M4 及跨能力功能簇接入；已有 queue 模块不代表通信自动入队。
- 认证验证码由 `VerificationCodes` 生成消费，发送仍由调用方选择渠道；本簇不启用短信登录或更改账号流程。

V1 的安全模板/布局与生成体验按 [F3 邮件模板](../plans/v1-foundation-completion.md#f3-邮件模板) 补齐，复用现有 lettre/Email/Mailer；本轮仅更新目标，当前纯文本本地化边界不变。
