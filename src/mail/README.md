# 邮件

`MailerServiceProvider` 提供配置驱动的 Provider 装配入口。装配只构造 driver，不发送邮件。

`MailConfig` 定义于 `openrust::config::mailer`，并从 `openrust::mail` 重新导出。默认配置在可编辑的 `config/mail.rs`，经同一配置仓储加载；TOML 仅作显式可选来源；SMTP 凭据可通过受保护的 `.env` 或部署环境变量提供，进程环境变量优先。生产使用 `.env` 时须限制文件权限，并确保应用根目录不作为静态公开目录。

实现 `Email::message` 构造 `lettre::Message`，通过同一容器的 `Mailer` 发送。可编辑 `config/mail.rs` 默认选择 `log`，在 production 和其他环境均可启动；它只记录接受摘要，不记录邮件正文、收件人或凭据，也不表示远端送达。没有真实投递需求时无需配置SMTP或修改应用环境。

实际邮件投递需启用 `mail-smtp`，在同一配置仓储选择 `smtp` 并提供 `smtp_host`；部署值使用 `MAIL_DRIVER`、`MAIL_SMTP_HOST`、`MAIL_SMTP_PORT`、`MAIL_SMTP_STARTTLS`，用户名和密码必须成对设置。远程主机要求STARTTLS；仅未认证的localhost/loopback开发sink可显式关闭。未知driver、缺少feature、缺失主机、凭据配对或TLS错误均拒绝初始化，不回退log。错误诊断显示服务、固定配置键和原因码，原始值保留于私有错误链，不直接输出到Display/Debug或HTTP 5xx响应。

测试可用 `test-util` 的 `FakeMailer`、`sent_count` 和 `assert_sent_to`。本机Mailpit协议验收需自行准备已有服务并显式配置；它不是外部供应商送达证明。SMS沿同一Provider/公开 `SmsDriver` 接口，默认log同样可在production启动且不记录PII；选择Twilio需启用 `sms-twilio` 并配置部署凭据和发送身份，真实发送仍需授权。

`Mailer::send` 会立即发送。启用 `queue` 后，`openrust::notifications::Notifier` 可把固定邮件/SMS 内容排入数据库队列；worker 重试复用已渲染内容，不会重新读取 locale 或模板。先注册 `DatabaseServiceProvider`、`QueueServiceProvider` 与 `NotificationsServiceProvider`，并把 `QueueMigration` 加入应用迁移器；标准 worker 用 `queue work --queue notifications` 处理通知队列。邮件与短信分别作为独立任务，渠道之间没有顺序保证。

应用可实现公开的 `MailDriver` / `SmsDriver`，再由应用自己的 `ServiceProvider::register` 注册 singleton 工厂，工厂通过 `Resolver::section` 读取当前配置并返回 `Arc<Mailer>` / `Arc<SmsSender>`。使用自定义供应商时，不注册对应的内建 `MailerServiceProvider` 或 `SmsServiceProvider`；内建 driver 名称只由框架维护。独立 consumer 展示从配置到发送的完整路径：[自定义邮件与短信 driver](../../tests/consumers/communication-drivers/README.md)。应用自定义供应商时也要自行定义其重试、幂等、凭据轮换和送达状态处理；短信 `SmsReceipt` 仅代表提交被接受。

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
    let notifier = app.state().resolve::<Notifier>().await?;
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
