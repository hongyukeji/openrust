# `openrust::mail` 与 `openrust::sms` 当前事实

## 邮件

- `MailConfig` 定义在 `openrust::config::mailer`，通过 `openrust::mail` 兼容 re-export；SMTP 本地凭据可放 `.env`，production 忽略 `.env`。
- `Email::message` 产出 `lettre::Message`；`Mailer::send` 通过公开 `MailDriver` 提交。
- `LocalizedEmail` 使用 `I18n` catalog、显式 `Locale` 与命名参数渲染纯文本主题/正文；后台任务不依赖请求 task-local locale。
- `MailerPlugin` 默认注册不记录内容的 `LogMailer`；production 选择 `log` 会启动失败。
- `mail-smtp` 启用 Tokio / Rustls 强制 STARTTLS；SMTP 密码 Debug 脱敏。`test-util` 暴露 `FakeMailer`。

## 短信

- `SmsConfig` 在 `openrust::config::sms`；应用可安装 `SmsPlugin` 获得 `SmsSender`。`SmsMessage` 支持 E.164 文本或供应商 template SID 与参数，号码与内容 Debug 脱敏。
- 公开 `SmsDriver`/`SmsSender::new` 可由第三方驱动扩展；默认 `LogSmsDriver` 不记录 PII，production 选择 log 启动失败；`test-util` 的 `FakeSmsDriver` 仅用于测试。
- `sms-twilio` feature 启用 `http-client` 与 Twilio Messages REST adapter，认证使用 API key SID/secret，凭据配合 account SID 与发送方配置；使用有界超时/响应、不会自动重试。成功表示 Twilio 接受请求，不表示最终送达。
- 邮件与短信文本由调用方显式选择语言；邮件可直接用 `LocalizedEmail`，短信通过 `I18n::translate(locale, ...)` 准备文本；短信 provider templates 使用供应商管理的模板，不被框架本地插值。

## 边界

- 本地 SMTP Mailpit 和真实供应商投递仍需真实服务及授权目标分别验收；当前 Twilio 协议测试使用注入 transport，不是账户实发。队列/失败任务在 M4 接入。
- 认证验证码由 `VerificationCodes` 生成消费，发送仍由调用方选择渠道；本簇不启用短信登录或更改账号流程。
