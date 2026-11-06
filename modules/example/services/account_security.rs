//! 账户安全业务只调用 AuthFlows；TOTP、挑战、共享 Session 与一次性消费均由框架完成。

use crate::modules::example::http::requests::account_security::CredentialsInput;
use openrust::auth::{AuthFlows, AuthManager, LoginAttempt, Session, SessionCredentials};
use openrust::foundation::RegisterContext;
use openrust::http::axum::http::{HeaderMap, header};
use openrust::support::ServiceProvider;
use openrust::{Error, Result};
use std::sync::Arc;

pub(crate) struct AccountSecurityProvider;
impl ServiceProvider for AccountSecurityProvider {
    fn register<'a>(
        &'a self,
        app: &'a mut RegisterContext<'_>,
    ) -> openrust::support::BoxFuture<'a, Result<()>> {
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
                        auth: resolver.resolve::<AuthManager>().await?,
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
    auth: Arc<AuthManager>,
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
        // 密码登录由所选账户的双因素策略裁决。
        let guard = self.auth.guard(Some("web")).await?;
        let guard = guard.as_stateful().ok_or_else(|| {
            Error::custom(
                "example_web_guard_not_stateful",
                "账户 web Guard 必须支持 Session 登录",
            )
        })?;
        self.flows
            .login_session_with_guard(&input.email, &input.password, source, session, guard)
            .await
    }
    pub(crate) async fn session_logout(&self, source: &str, session: &Session) -> Result<()> {
        let guard = self.auth.guard(Some("web")).await?;
        let guard = guard.as_stateful().ok_or_else(|| {
            Error::custom(
                "example_web_guard_not_stateful",
                "账户 web Guard 必须支持 Session 注销",
            )
        })?;
        self.flows
            .logout_session_with_guard(source, session, guard)
            .await
    }
    #[cfg(feature = "auth-fortify")]
    pub(crate) async fn complete(
        &self,
        input: crate::modules::example::http::requests::account_security::ChallengeInput,
        source: &str,
    ) -> Result<openrust::auth::AuthCredentials> {
        self.flows
            .complete_two_factor_login(
                &input.challenge,
                input.code.as_deref(),
                input.recovery_code.as_deref(),
                source,
            )
            .await
    }
}
