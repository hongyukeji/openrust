//! 显式无 Cookie JWT API 登录适配；复用同一账户 Provider 与 Fortify 因子内核。

use super::{CredentialAuthority, FortifyCallbacks, FortifyConfig};
use crate::auth::credential::CredentialGuardBinding;
use crate::{
    Error, Result,
    auth::{
        AuthCredentials, AuthFlows, AuthUser, Credentials, JwtTokens, LoginAttempt, UserProvider,
        VerifiedCredential,
    },
    http::RequestInfo,
};
use serde::{Deserialize, Serialize};
use std::{fmt, sync::Arc, time::Duration};

/// 显式 API profile 的路由与具名身份域；不覆盖官方 Fortify Web 配置。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FortifyJwtApiConfig {
    /// 已配置为 jwt-api driver 的具名 Guard。
    pub guard: String,
    /// 相对应用 API 前缀的认证路径。
    pub prefix: String,
}
impl Default for FortifyJwtApiConfig {
    fn default() -> Self {
        Self {
            guard: "api".into(),
            prefix: "/auth".into(),
        }
    }
}

/// 请求级 JWT 登录服务，不建立 Web Session 或 Cookie。
///
/// 当前持久凭据适配使用同一 users.id 正整数域；泛型 Fortify 账户能力仍保留原始 ID。
/// 自定义字符串 ID 的 JWT 持久适配尚须单独完成，不能从通用模型契约推断已支持。
pub struct FortifyJwtApi {
    pub(super) scope: CredentialGuardBinding,
    pub(super) provider: Arc<dyn UserProvider>,
    pub(super) flows: Arc<AuthFlows>,
    pub(super) authority: Arc<CredentialAuthority>,
    pub(super) tokens: Arc<JwtTokens>,
    pub(super) callbacks: Arc<FortifyCallbacks>,
    pub(super) config: FortifyConfig,
    pub(super) rehash: bool,
    pub(super) timebox: Duration,
    pub(super) app_name: String,
}
impl fmt::Debug for FortifyJwtApi {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FortifyJwtApi")
            .field("guard", &self.scope.guard)
            .finish_non_exhaustive()
    }
}
impl FortifyJwtApi {
    /// 验证密码后返回短期挑战或 access/refresh；不把待确认绑定当作已开启。
    pub async fn begin_login(
        &self,
        request: &RequestInfo,
        credentials: &Credentials,
    ) -> Result<LoginAttempt> {
        let source = request_source(request)?;
        let account = credentials
            .get(&self.config.username)
            .and_then(serde_json::Value::as_str)
            .ok_or(Error::Unauthorized)?;
        if account.is_empty() || account.len() > 1_024 {
            return Err(Error::Unauthorized);
        }
        self.flows
            .limit_api("jwt_api_login", &source, account)
            .await?;
        let deadline = tokio::time::Instant::now() + self.timebox;
        let result = self.password_user(request, credentials).await;
        tokio::time::sleep_until(deadline).await;
        let user = result?.ok_or(Error::Unauthorized)?;
        self.flows
            .begin_verified_api_login(&user, self.provider.as_ref(), self.scope.clone(), &source)
            .await
    }
    async fn password_user(
        &self,
        request: &RequestInfo,
        credentials: &Credentials,
    ) -> Result<Option<AuthUser>> {
        let user = if let Some(callback) = &self.callbacks.authenticate {
            self.authority
                .db()
                .with_primary(callback(request, credentials))
                .await??
        } else {
            let Some(user) = self.provider.retrieve_by_credentials(credentials).await? else {
                return Ok(None);
            };
            if !self
                .provider
                .validate_credentials(user.as_ref(), credentials)
                .await?
            {
                return Ok(None);
            }
            if self.rehash {
                self.provider
                    .rehash_password_if_required(user.as_ref(), credentials, false)
                    .await?;
            }
            Some(user)
        };
        let Some(user) = user else {
            return Ok(None);
        };
        // 回调必须返回同一实际账户域，不能仅用相同数字 ID 替换另一个 Provider。
        let authority = user
            .as_two_factor()
            .and_then(super::TwoFactorAuthenticatable::two_factor_authority)
            .ok_or(Error::Unauthorized)?;
        authority
            .validate_account(&user, self.provider.as_ref())
            .await?;
        let identifier = user.auth_identifier()?;
        let current = self
            .authority
            .db()
            .with_primary(self.provider.retrieve_by_id(&identifier))
            .await??
            .ok_or(Error::Unauthorized)?;
        if current.auth_identifier()? != identifier {
            return Err(Error::Unauthorized);
        }
        if current.auth_password()? != user.auth_password()?
            && !self
                .provider
                .validate_credentials(current.as_ref(), credentials)
                .await?
        {
            return Err(Error::Unauthorized);
        }
        Ok(Some(current))
    }
    /// 消费绑定本 Guard/Provider 的挑战；恢复码与 TOTP 分别传入。
    pub async fn complete_login(
        &self,
        request: &RequestInfo,
        challenge: &str,
        code: Option<&str>,
        recovery_code: Option<&str>,
    ) -> Result<AuthCredentials> {
        self.flows
            .complete_two_factor_login_in_scope(
                challenge,
                code,
                recovery_code,
                &request_source(request)?,
                Some(&self.scope),
                Some(self.provider.as_ref()),
            )
            .await
    }
    /// 仅接受本 profile 的当前真实 JWT 家族，不接受管理 grant 或裸签名 ID。
    pub async fn authenticate(&self, raw: &str) -> Result<(AuthUser, VerifiedCredential)> {
        let credential = self
            .authority
            .authenticate_access_token(&self.tokens, raw)
            .await?;
        self.require_scope(&credential)?;
        let identifier = serde_json::Value::from(credential.user_id());
        let user = self
            .authority
            .db()
            .with_primary(self.provider.retrieve_by_id(&identifier))
            .await??
            .ok_or(Error::Unauthorized)?;
        if user.auth_identifier()? != identifier {
            return Err(Error::Unauthorized);
        }
        user.as_two_factor()
            .and_then(super::TwoFactorAuthenticatable::two_factor_authority)
            .ok_or(Error::Unauthorized)?
            .validate_account(&user, self.provider.as_ref())
            .await?;
        // Provider 的异步查询之后重新核验主库、家族与期限，不能借旧快照放行。
        let current = self
            .authority
            .authenticate_access_token(&self.tokens, raw)
            .await?;
        self.require_scope(&current)?;
        Ok((user, current))
    }
    #[cfg(feature = "grpc")]
    pub(crate) async fn authenticate_rpc_user(
        &self,
        raw: &str,
    ) -> Result<crate::auth::CurrentUser> {
        let (_, credential) = self.authenticate(raw).await?;
        Ok(crate::auth::CurrentUser {
            id: credential.user_id().into(),
            source: crate::auth::AuthSource::Guard,
            guard: Some(self.scope.guard.clone()),
            provider: Some(self.scope.provider.clone()),
            token_abilities: None,
        })
    }

    fn require_scope(&self, credential: &VerifiedCredential) -> Result<()> {
        if credential
            .context
            .as_ref()
            .and_then(|context| context.scope.as_ref())
            != Some(&self.scope)
        {
            return Err(Error::Unauthorized);
        }
        Ok(())
    }
    /// 轮换本 profile 的 refresh 家族，保留既有因子事实和绝对期限。
    pub async fn refresh(&self, request: &RequestInfo, refresh: &str) -> Result<AuthCredentials> {
        self.flows
            .refresh_in_scope(refresh, &request_source(request)?, Some(&self.scope))
            .await
    }
    /// 注销真实 refresh 家族，不创建或清除 Web Session。
    pub async fn logout(&self, request: &RequestInfo, refresh: &str) -> Result<()> {
        self.flows
            .limit_api("jwt_api_logout", &request_source(request)?, refresh)
            .await?;
        crate::auth::RefreshTokens::new(self.authority.db().clone())
            .revoke_scoped(refresh, &self.scope)
            .await
    }
    pub(super) async fn validate_password(&self, credentials: &Credentials) -> Result<bool> {
        let Some(user) = self.provider.retrieve_by_credentials(credentials).await? else {
            return Ok(false);
        };
        self.provider
            .validate_credentials(user.as_ref(), credentials)
            .await
    }
}

pub(super) fn request_source(request: &RequestInfo) -> Result<String> {
    request
        .client_ip()?
        .map(|ip| ip.to_string())
        .ok_or_else(|| Error::BadRequest("JWT API 登录需要可信客户端地址".into()))
}

pub(super) fn bearer(request: &RequestInfo) -> Result<Option<&str>> {
    use crate::http::axum::http::header::AUTHORIZATION;
    if request.headers().get_all(AUTHORIZATION).iter().count() > 1 {
        return Err(Error::Unauthorized);
    }
    let Some(raw) = request.header("authorization")? else {
        return Ok(None);
    };
    let (kind, token) = raw.split_once(' ').ok_or(Error::Unauthorized)?;
    if !kind.eq_ignore_ascii_case("Bearer")
        || token.is_empty()
        || token.bytes().any(|b| b.is_ascii_whitespace() || b == b',')
    {
        return Err(Error::Unauthorized);
    }
    Ok(Some(token))
}
