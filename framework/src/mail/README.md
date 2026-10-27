# 邮件

`MailConfig` 定义于 `openrust::config::mailer`，并从 `openrust::mail` 重新导出。运行时 TOML 配置放在应用根目录的 `config/`；SMTP 凭据可通过受保护的 `.env` 或部署环境变量提供，进程环境变量优先。生产使用 `.env` 时须限制文件权限，并确保应用根目录不作为静态公开目录。

OpenRust 框架的邮件能力。实现 `Email::message` 来构造 `lettre::Message`，然后通过 `Mailer` 服务发送。`MailerPlugin` 默认注册 log driver。启用 `mail-smtp`，并设置 `[mail] driver = "smtp"`、`smtp_host`，还可按需设置成对的 `smtp_username` / `smtp_password` 字段，以使用强制 STARTTLS 的 SMTP 发送邮件。SMTP 凭据在 `Debug` 输出中会被隐藏。默认要求 STARTTLS；只有 Mailpit 等未认证的 localhost/loopback 开发接收端可以显式设置 `smtp_starttls = false`，生产环境会拒绝该设置。使用 `docker compose -f compose.ci.yaml up -d mailpit` 启动本地 Mailpit 服务，然后设置 `MAIL_DRIVER=smtp`、`MAIL_SMTP_HOST=127.0.0.1`、`MAIL_SMTP_PORT=1025` 和 `MAIL_SMTP_STARTTLS=false`。生产环境拒绝默认的 `log` driver，因此必须显式配置实际邮件供应商。启用 `test-util` 后，可在测试中使用 `FakeMailer` 的 `sent_count` 和 `assert_sent_to` 断言。SMS 是 `openrust::sms` 中的同级服务；两者都使用公开 driver trait、显式 locale 选择，并且单次提交不会自动重试。启用 `sms-twilio`（同时启用 `http-client`）即可使用 Twilio；配置时必须设置 `SMS_ACCOUNT_SID`、`SMS_API_KEY_SID`、`SMS_API_KEY_SECRET`，并在 `SMS_FROM` 与 `SMS_MESSAGING_SERVICE_SID` 中二选一。凭据和消息数据在 debug 输出中会被隐藏；生产环境拒绝 log driver。

`Mailer::send` 会立即发送。启用 `queue` 后，`openrust::notifications::Notifier` 可把固定邮件/SMS 内容排入数据库队列；worker 重试复用已渲染内容，不会重新读取 locale 或模板。先注册 `DatabasePlugin`、`QueuePlugin` 与 `NotificationsPlugin`，并把 `QueueMigration` 加入应用迁移器；标准 worker 用 `queue work notifications` 处理默认通知队列。邮件与短信分别作为独立任务，渠道之间没有顺序保证。

应用可实现公开的 `MailDriver` / `SmsDriver`，再由应用自己的 `Plugin::build` 读取供应商配置并通过 `app.try_insert(Mailer::new(driver))`、`app.try_insert(SmsSender::new(driver))` 注册服务。使用自定义供应商时，不注册对应的内建 `MailerPlugin` 或 `SmsPlugin`；内建 driver 名称只由框架维护。独立 consumer 展示从配置到发送的完整路径：[自定义邮件与短信 driver](../../../tests/consumers/communication-drivers/README.md)。应用自定义供应商时也要自行定义其重试、幂等、凭据轮换和送达状态处理；短信 `SmsReceipt` 仅代表提交被接受。

通知消息会将收件人和正文写入队列数据库，调用方应按业务数据保护数据库、备份和保留期；不要将密码、access/refresh token、验证码或不必要的个人信息放入任务。直接 `dispatch` 对各渠道分别提交，若后续渠道入队失败，已提交的前序任务不会自动撤销；需要与业务变更或多个渠道一起原子提交时，使用 `dispatch_in_transaction`。活动唯一键按渠道分别生效，任务完成或失败归档后释放，不是永久请求幂等记录。

在业务事务中派发多渠道通知：

```rust,no_run
#[cfg(feature = "queue")]
use std::sync::Arc;
#[cfg(feature = "queue")]
use openrust::{
    core::{App, Result},
    db::Db,
    notifications::{EmailDelivery, Notification, Notifier, SmsDelivery},
};

#[cfg(feature = "queue")]
async fn queue_account_notice(app: &App, db: &Db) -> Result<()> {
    let notifier = app.state().try_get::<Notifier>()?;
    let message = Notification::new()
        .email(EmailDelivery::new(
            "noreply@example.com",
            "person@example.com",
            "账户已创建",
            "欢迎使用。",
            None,
        )?)
        .sms(SmsDelivery::text("+15555550123", "账户已创建")?);
    db.transaction(|transaction| {
        let notifier = Arc::clone(&notifier);
        let message = message.clone();
        Box::pin(async move {
            notifier
                .dispatch_in_transaction(transaction, &message)
                .await?;
            Ok(())
        })
    })
    .await
}
```

队列执行至少一次。SMTP/Twilio 已受理但 worker 在确认任务前故障时，重试可能再次发送；框架不承诺外部发送 exactly-once。供应商接受提交也不代表最终送达。

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
