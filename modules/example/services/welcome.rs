//! 业务主动发起的欢迎消息；启动和 Provider 装配均不触发发送。

use std::sync::Arc;

use openrust::core::{RegisterContext, ServiceProvider};
#[cfg(all(feature = "auth", any(feature = "db-postgres", feature = "db-mysql")))]
use openrust::db::sea_orm::{ConnectionTrait, DbBackend, Statement};
use openrust::mail::{Email, Mailer, lettre::Message};
#[cfg(test)]
use openrust::sms::{SmsMessage, SmsSender};
use openrust::{Error, Result};
#[cfg(all(feature = "auth", any(feature = "db-postgres", feature = "db-mysql")))]
use openrust::{
    i18n::{I18n, Locale, MessageArgs},
    mail::LocalizedEmail,
};

/// 注册欢迎业务服务，复用当前应用最终绑定的通信服务。
pub(crate) struct WelcomeServiceProvider;
impl ServiceProvider for WelcomeServiceProvider {
    fn register<'a>(
        &'a self,
        app: &'a mut RegisterContext<'_>,
    ) -> openrust::core::BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.singleton::<WelcomeService, _>(|resolver| {
                Box::pin(async move {
                    Ok(Arc::new(WelcomeService {
                        mailer: resolver.resolve::<Mailer>().await?,
                        #[cfg(test)]
                        sms: resolver.resolve::<SmsSender>().await?,
                    }))
                })
            })
            .await?;
            Ok(())
        })
    }
}

/// 测试受控业务所用的两个目的地；不实现 Debug，避免记录个人数据。
#[cfg(test)]
pub(crate) struct WelcomeRecipient {
    email: WelcomeEmail,
    sms: SmsMessage,
}
#[cfg(test)]
impl WelcomeRecipient {
    /// 先验证两个目的地，再允许任何一条消息被发送。
    pub(crate) fn new(email: impl Into<String>, phone: impl Into<String>) -> Result<Self> {
        let email = WelcomeEmail::new(email)?;
        let sms = SmsMessage::text(phone, "欢迎加入 OpenRust 示例业务。")?;
        Ok(Self { email, sms })
    }
}

/// 通过公开 Mailer/SmsSender 调用业务欢迎消息。
pub(crate) struct WelcomeService {
    mailer: Arc<Mailer>,
    #[cfg(test)]
    sms: Arc<SmsSender>,
}
impl WelcomeService {
    /// HTTP 自助欢迎邮件只发往数据库确认的当前账户邮箱。
    #[cfg(all(feature = "auth", any(feature = "db-postgres", feature = "db-mysql")))]
    pub(crate) async fn send_email(
        &self,
        verified_email: String,
        i18n: &I18n,
        locale: &Locale,
    ) -> Result<()> {
        WelcomeEmail::new(verified_email.clone())?;
        let email = LocalizedEmail::new(
            i18n,
            locale,
            &MessageArgs::new(),
            "OpenRust <hello@example.test>",
            &verified_email,
            "example::welcome_subject",
            "example::welcome_body",
        )
        .map_err(|_| Error::custom("example_welcome_email_invalid", "欢迎邮件构造失败"))?;
        self.mailer.send(&email).await
    }

    /// 先投递邮件，再提交 SMS；只有显式业务调用才产生副作用。
    ///
    /// 两种传输不具备跨通道事务：SMS 失败不会撤回已受理邮件，错误原样向调用者传播。
    /// 调用者不能无条件重试整个操作并声称恰一次发送。
    #[cfg(test)]
    pub(crate) async fn send(&self, recipient: WelcomeRecipient) -> Result<()> {
        self.mailer.send(&recipient.email).await?;
        self.sms.send(recipient.sms).await?;
        Ok(())
    }
}

/// 以可信账户 ID 从同一数据库连接读取未停用、已验证的邮箱。
#[cfg(all(feature = "auth", any(feature = "db-postgres", feature = "db-mysql")))]
pub(crate) async fn verified_email<C: ConnectionTrait>(
    db: &openrust::Db,
    connection: &C,
    user_id: i64,
) -> Result<String> {
    let backend = connection.get_database_backend();
    let users = db.quoted_table_name("users")?;
    let lookup = match backend {
        DbBackend::Postgres => format!(
            "SELECT email FROM {users} WHERE id = $1 AND email_verified_at IS NOT NULL AND disabled_at IS NULL"
        ),
        DbBackend::MySql => format!(
            "SELECT email FROM {users} WHERE id = ? AND email_verified_at IS NOT NULL AND disabled_at IS NULL"
        ),
        _ => {
            return Err(Error::custom(
                "unsupported_database_backend",
                "不支持的数据库后端",
            ));
        }
    };
    let row = connection
        .query_one_raw(Statement::from_sql_and_values(
            backend,
            &lookup,
            [user_id.into()],
        ))
        .await
        .map_err(Error::internal)?
        .ok_or(Error::Unauthorized)?;
    row.try_get::<String>("", "email").map_err(Error::internal)
}

struct WelcomeEmail {
    recipient: String,
}
impl WelcomeEmail {
    fn new(recipient: impl Into<String>) -> Result<Self> {
        let email = Self {
            recipient: recipient.into(),
        };
        // 在进入服务前完成邮件目的地验证，不把用户提交的地址写入错误。
        email.message()?;
        Ok(email)
    }
}
impl Email for WelcomeEmail {
    fn message(&self) -> Result<Message> {
        let invalid = || Error::custom("example_welcome_email_invalid", "欢迎邮件地址无效");
        Message::builder()
            .from(
                "OpenRust <hello@example.test>"
                    .parse()
                    .map_err(|_| invalid())?,
            )
            .to(self.recipient.parse().map_err(|_| invalid())?)
            .subject("欢迎加入")
            .body("Welcome to OpenRust.".to_owned())
            .map_err(|_| Error::custom("example_welcome_email_invalid", "欢迎邮件构造失败"))
    }
}
