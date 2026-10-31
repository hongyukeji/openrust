//! 账户安全业务只调用 AuthFlows；TOTP、挑战、共享 Session 与一次性消费均由框架完成。

use crate::modules::example::http::requests::account_security::CredentialsInput;
#[cfg(feature = "auth-mfa")]
use crate::modules::example::http::requests::account_security::FactorInput;
use openrust::auth::{AuthFlows, LoginAttempt, Session, SessionCredentials};
use openrust::core::{RegisterContext, ServiceProvider};
use openrust::http::axum::http::{HeaderMap, header};
use openrust::{Error, Result};
use std::sync::Arc;

pub(crate) struct AccountSecurityProvider;
impl ServiceProvider for AccountSecurityProvider {
    fn register<'a>(
        &'a self,
        app: &'a mut RegisterContext<'_>,
    ) -> openrust::core::BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            // AuthFlows 是 request-scoped，业务包装不能在 singleton 中捕获它。
            app.scoped::<AccountSecurity, _>(|resolver| {
                Box::pin(async move {
                    let config = resolver.section::<openrust::config::AppConfig>("app")?;
                    let url = url::Url::parse(&config.url).map_err(|_| {
                        Error::custom(
                            "example_account_origin_invalid",
                            "app.url 必须是公开 HTTP(S) 地址",
                        )
                    })?;
                    if !matches!(url.scheme(), "http" | "https")
                        || url.host_str().is_none()
                        || !url.username().is_empty()
                        || url.password().is_some()
                    {
                        return Err(Error::custom(
                            "example_account_origin_invalid",
                            "app.url 必须是公开 HTTP(S) 地址",
                        ));
                    }
                    Ok(Arc::new(AccountSecurity {
                        flows: resolver.resolve::<AuthFlows>().await?,
                        origin: url.origin().ascii_serialization(),
                    }))
                })
            })
            .await?;
            Ok(())
        })
    }
}

pub(crate) struct AccountSecurity {
    flows: Arc<AuthFlows>,
    origin: String,
}
impl AccountSecurity {
    /// 无身份的登录也会改变浏览器 Session；要求同源 Origin 和非简单请求标记。
    pub(crate) fn require_same_origin(&self, headers: &HeaderMap) -> Result<()> {
        if headers.get_all(header::ORIGIN).iter().count() != 1
            || headers.get(header::ORIGIN).and_then(|v| v.to_str().ok())
                != Some(self.origin.as_str())
            || headers.get_all("x-example-request").iter().count() != 1
            || headers
                .get("x-example-request")
                .and_then(|v| v.to_str().ok())
                != Some("same-origin")
        {
            return Err(Error::Forbidden("账户请求必须来自应用同源页面".into()));
        }
        Ok(())
    }
    pub(crate) async fn login(
        &self,
        input: CredentialsInput,
        source: &str,
    ) -> Result<LoginAttempt> {
        self.flows
            .begin_login(&input.email, &input.password, source)
            .await
    }
    pub(crate) async fn session_login(
        &self,
        input: CredentialsInput,
        source: &str,
        session: &Session,
    ) -> Result<SessionCredentials> {
        // 框架拒绝已启用 MFA 的账户；此路径不能绕开第二因子。
        self.flows
            .login_session(&input.email, &input.password, source, session)
            .await
    }
    pub(crate) async fn session_logout(&self, source: &str, session: &Session) -> Result<()> {
        self.flows.logout_session(source, session).await
    }
    #[cfg(feature = "auth-mfa")]
    pub(crate) async fn complete(
        &self,
        input: crate::modules::example::http::requests::account_security::ChallengeInput,
        source: &str,
    ) -> Result<openrust::auth::AuthCredentials> {
        self.flows
            .complete_mfa_login(&input.challenge, &input.code, source)
            .await
    }
    #[cfg(feature = "auth-mfa")]
    pub(crate) async fn enroll(
        &self,
        user: i64,
        input: CredentialsInput,
        source: &str,
    ) -> Result<openrust::auth::TotpEnrollment> {
        self.flows
            .begin_totp_enrollment(
                user,
                &input.email,
                &input.password,
                "OpenRust example",
                source,
            )
            .await
    }
    #[cfg(feature = "auth-mfa")]
    pub(crate) async fn confirm(
        &self,
        user: i64,
        input: FactorInput,
        source: &str,
    ) -> Result<openrust::auth::RecoveryCodes> {
        self.reauthenticate(user, &input, source).await?;
        self.flows
            .confirm_totp_enrollment(user, &input.code, source)
            .await
    }
    #[cfg(feature = "auth-mfa")]
    pub(crate) async fn regenerate(
        &self,
        user: i64,
        input: FactorInput,
        source: &str,
    ) -> Result<openrust::auth::RecoveryCodes> {
        self.flows
            .regenerate_mfa_recovery_codes(user, &input.email, &input.password, &input.code, source)
            .await
    }
    #[cfg(feature = "auth-mfa")]
    pub(crate) async fn disable(&self, user: i64, input: FactorInput, source: &str) -> Result<()> {
        self.flows
            .disable_mfa(user, &input.email, &input.password, &input.code, source)
            .await
    }
    #[cfg(feature = "auth-mfa")]
    async fn reauthenticate(&self, user: i64, input: &FactorInput, source: &str) -> Result<()> {
        if self
            .flows
            .reauthenticate(&input.email, &input.password, source)
            .await?
            != user
        {
            return Err(Error::Unauthorized);
        }
        Ok(())
    }
}
