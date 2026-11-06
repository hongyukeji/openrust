//! 旧协议夹具复用真实应用迁移和密码凭据；不构造或伪造因子证明。
#[cfg(feature = "auth-fortify")]
use openrust::Error;
use openrust::database::sea_orm_migration::{MigrationTrait, MigratorTrait};
use openrust::{Application, Result};

pub(super) fn migrations() -> Vec<Box<dyn MigrationTrait>> {
    #[cfg(feature = "auth-fortify")]
    {
        crate::database::Migrator::migrations()
    }
    #[cfg(not(feature = "auth-fortify"))]
    {
        openrust::database::FrameworkMigrator::migrations()
    }
}

/// 为隔离协议夹具显式准备测试密码，再经正式密码登录取得受管凭据。
/// 只供不测试密码本身的夹具使用；已有用户的非空邮箱、禁用及其它业务属性保持。
pub(super) async fn seeded_password_access(app: &Application, id: i64) -> Result<String> {
    #[cfg(feature = "auth-fortify")]
    {
        let db = app.state().resolve::<openrust::Db>().await?;
        let passwords = app
            .state()
            .resolve::<openrust::hashing::HashManager>()
            .await?;
        let password = "selected-protocol-fixture-password";
        let hash = openrust::auth::EncodedPasswordHash::make(passwords.as_ref(), password).await?;
        let user = db.table("users")?.where_eq("id", id)?.first().await?;
        let email = user
            .as_ref()
            .map(|row| {
                row.try_get::<Option<String>>("", "email")
                    .map_err(Error::internal)
            })
            .transpose()?
            .flatten()
            .unwrap_or_else(|| format!("fixture-{id}@example.test"));
        let now = chrono::Utc::now();
        if user.is_some() {
            db.table("users")?
                .where_eq("id", id)?
                .update(vec![
                    ("email", email.as_str().into()),
                    ("password_hash", hash.as_str().into()),
                    ("email_verified_at", now.into()),
                ])
                .await?;
        } else {
            db.table("users")?
                .insert(vec![
                    ("id", id.into()),
                    ("email", email.as_str().into()),
                    ("password_hash", hash.as_str().into()),
                    ("email_verified_at", now.into()),
                    ("created_at", now.into()),
                    ("updated_at", now.into()),
                ])
                .await?;
        }
        let flows = app.state().resolve::<openrust::auth::AuthFlows>().await?;
        let credentials = flows
            .login(&email, password, &format!("selected-fixture-{id}"))
            .await?;
        assert_eq!(credentials.user_id(), id);
        Ok(credentials.access_token().to_owned())
    }
    #[cfg(not(feature = "auth-fortify"))]
    {
        app.state()
            .resolve::<openrust::auth::JwtTokens>()
            .await?
            .issue_access_token(id)
    }
}
