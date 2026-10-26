# `openrust::mail` 当前事实

## 已实现

- `MailConfig` 定义在 `openrust::config::mailer`，旧的 `openrust::mail::MailConfig` 导出保持可用；SMTP 本地凭据可放入 `.env`，production 忽略 `.env`。

- `Email::message` 产出 `lettre::Message`；`Mailer::send` 通过 `MailDriver` 扩展入口交付邮件。
- `MailerPlugin` 读取 `[mail]` 配置，默认注册不记录邮件内容的 `LogMailer`。
- `mail-smtp` 启用 Tokio / Rustls 强制 STARTTLS SMTP driver；SMTP 密码的 Debug 输出已脱敏。
- `test-util` 提供 `FakeMailer`，支持发送数量与收件人断言。
- `apps/server` 和 CLI 生成项目默认注册 `MailerPlugin`，默认配置为 `driver = "log"`。

## 边界

- `Mailer::send` 立即发送；队列化发送与短信能力在后续 M3/M4 功能簇接入。
- SMTP 使用 TLS relay，真实供应商发送需独立运行环境验收。
