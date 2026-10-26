# 邮件

`MailConfig` 定义于 `openrust::config::mailer`，并从 `openrust::mail` 重新导出。运行时 TOML 配置放在应用根目录的 `config/`；本地 SMTP 凭据可通过 `.env` 提供。生产环境从进程环境变量读取秘密，并忽略 `.env`。

OpenRust 框架的邮件能力。实现 `Email::message` 来构造 `lettre::Message`，然后通过 `Mailer` 服务发送。`MailerPlugin` 默认注册 log driver。启用 `mail-smtp`，并设置 `[mail] driver = "smtp"`、`smtp_host`，还可按需设置成对的 `smtp_username` / `smtp_password` 字段，以使用强制 STARTTLS 的 SMTP 发送邮件。SMTP 凭据在 `Debug` 输出中会被隐藏。默认要求 STARTTLS；只有 Mailpit 等未认证的 localhost/loopback 开发接收端可以显式设置 `smtp_starttls = false`，生产环境会拒绝该设置。使用 `docker compose -f compose.ci.yaml up -d mailpit` 启动本地 Mailpit 服务，然后设置 `APP_MAIL__DRIVER=smtp`、`APP_MAIL__SMTP_HOST=127.0.0.1`、`APP_MAIL__SMTP_PORT=1025` 和 `APP_MAIL__SMTP_STARTTLS=false`。生产环境拒绝默认的 `log` driver，因此必须显式配置实际邮件供应商。启用 `test-util` 后，可在测试中使用 `FakeMailer` 的 `sent_count` 和 `assert_sent_to` 断言。SMS 是 `openrust::sms` 中的同级服务；两者都使用公开 driver trait、显式 locale 选择，并且单次提交不会自动重试。启用 `sms-twilio`（同时启用 `http-client`）即可使用 Twilio；配置时必须设置 `sms.account_sid`、`sms.api_key_sid`、`sms.api_key_secret`，并在 `sms.from` 与 `sms.messaging_service_sid` 中二选一。凭据和消息数据在 debug 输出中会被隐藏；生产环境拒绝 log driver。

`Mailer::send` 会立即发送。队列能力在 M4 中集成。供应商接受提交不代表邮件已送达。

要本地化邮件，请从请求的 `RequestLocale` 中选择 `Locale`，或为后台任务显式指定 locale。`LocalizedEmail::new` 会从共享 `I18n` catalog 中读取独立的主题和正文 key，使用命名参数渲染，并构造纯文本 `lettre::Message`；应用仍可自行实现 `Email`，以使用自定义 multipart 或 HTML 格式。邮件传输层不会从全局状态推断 locale；调用方须在构造消息时选择。

启用可选 feature `mail-templates` 后，可使用 `EmailTemplateCatalog` 注册应用编译期嵌入的模板，并以 `MiniJinja` 渲染纯文本及 HTML。模板源由 `include_str!` 引入；框架不提供任意文件或网络 loader。注册名必须是受限相对标识符；HTML 文件自动转义，缺少变量、语法错误、执行预算耗尽会返回错误。单个正文最多输出 256 KiB，主题拒绝换行并限制为 998 字节。locale 依次尝试精确标签、同语言资源和配置的模板回退 locale；主题仍由共享 `I18n` catalog 翻译。`RenderedEmail::multipart` 可直接组合附件，之后仍通过现有 `Mailer` 发送。

运行 `openrust generate mailer <name>` 会启用 `mail-templates`，生成 Rust 注册封装与 `templates/mail/<name>/{en,zh-CN}/` 下的纯文本/HTML 示例。生成后须在应用 `I18n` catalog 中增加 `mailers.<name>.subject`，并按应用需要编辑模板。该命令不会覆盖已存在邮件或模板目录。

```rust,no_run
use openrust::mail::{Email, Mailer, lettre::Message};
use openrust::core::{Error, Result};

struct WelcomeEmail;

impl Email for WelcomeEmail {
    fn message(&self) -> Result<Message> {
        Ok(Message::builder()
            .from("OpenRust <noreply@example.com>".parse().map_err(Error::internal)?)
            .to("person@example.com".parse().map_err(Error::internal)?)
            .subject("Welcome")
            .body(String::from("Welcome to OpenRust"))
            .map_err(Error::internal)?)
    }
}

async fn send(mailer: &Mailer) -> Result<()> {
    mailer.send(&WelcomeEmail).await
}
```

使用 SMTP 时，在 `openrust` 依赖中添加 `features = ["mail-smtp"]`，并进行如下配置：

```toml
[mail]
driver = "smtp"
smtp_host = "smtp.example.com"
smtp_port = 587
smtp_username = "mailer"
smtp_password = "replace-with-a-secret"
```
