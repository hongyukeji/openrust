//! Fortify 密码首因子与同一 `StatefulGuard` 的 Session 双因素挑战。

use std::{fmt, sync::Arc, time::Duration};

use serde_json::Value;
use subtle::ConstantTimeEq;
use tokio::time::Instant;
use zeroize::Zeroizing;

use crate::support::BoxFuture;
use crate::{
    Error, Result,
    auth::{AuthManager, AuthUser, Credentials, FactorReceipt, FactorVerificationMethod, Guard},
    core::error::ValidationErrors,
    http::RequestInfo,
    session::Session,
};

use super::authority::FactorAuthoritySnapshot;
use super::{
    TwoFactorActions,
    actions::{RecoveryCode, RecoveryCodeReplaced, capability},
};

/// 密码首因子处理结果；挑战阶段没有正式Guard身份。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FortifyLoginOutcome {
    /// 账户未启用适用双因素，已通过同一Guard正常登录。
    Authenticated,
    /// 仅写入Session挑战，HTTP JSON对应`{"two_factor":true}`。
    TwoFactorChallenge,
}

/// 对应公开authenticateUsing回调，返回同一账户类型而非另一套凭据。
pub type AuthenticateUsing = Arc<
    dyn for<'a> Fn(&'a RequestInfo, &'a Credentials) -> BoxFuture<'a, Result<Option<AuthUser>>>
        + Send
        + Sync,
>;

/// 配置具名Guard下的登录管线；限流、guest、Web与密码确认由同一路由中间件装配。
pub struct FortifyLogin {
    name: String,
    guard: Arc<dyn Guard>,
    session: Session,
    actions: Arc<TwoFactorActions>,
    username: String,
    rehash_on_login: bool,
    authenticate_using: Option<AuthenticateUsing>,
    factor_authority_required: bool,
}

impl fmt::Debug for FortifyLogin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FortifyLogin")
            .field("guard", &self.name)
            .finish_non_exhaustive()
    }
}

impl FortifyLogin {
    /// 从本请求AuthManager解析真实具名Guard，不接受脱离配置的另一份Provider。
    pub async fn new(
        manager: &AuthManager,
        guard: &str,
        session: Session,
        actions: Arc<TwoFactorActions>,
    ) -> Result<Self> {
        let selected = manager.guard(Some(guard)).await?;
        let stateful = selected.as_stateful().ok_or_else(stateful_required)?;
        if !stateful.supports_session_regeneration_login() {
            return Err(Error::custom_with_status(
                crate::http::axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "fortify_session_login_unsupported",
                "StatefulGuard未实现可取消安全的Session组合登录",
            ));
        }
        Ok(Self {
            name: guard.into(),
            guard: selected,
            session,
            actions,
            username: "email".into(),
            rehash_on_login: true,
            authenticate_using: None,
            factor_authority_required: false,
        })
    }

    /// 对应Fortify username配置；组合账号入口通过authenticateUsing公开扩展实现。
    #[must_use]
    pub fn with_username(mut self, username: impl Into<String>) -> Self {
        self.username = username.into();
        self
    }
    /// 对应 `hashing.rehash_on_login` 配置。
    #[must_use]
    pub fn with_rehash_on_login(mut self, enabled: bool) -> Self {
        self.rehash_on_login = enabled;
        self
    }
    /// 对应 `Fortify::authenticateUsing`；回调只返回账户，不得先建立正式登录。
    #[must_use]
    pub fn with_authenticate_using(mut self, callback: AuthenticateUsing) -> Self {
        self.authenticate_using = Some(callback);
        self
    }

    /// 显式要求账户提供同数据库因子authority，即使双因素界面选项关闭也不跳过首阶段。
    #[must_use]
    pub fn with_factor_authority_required(mut self, required: bool) -> Self {
        self.factor_authority_required = required;
        self
    }

    async fn authority_snapshot(&self, user: &AuthUser) -> Result<Option<FactorAuthoritySnapshot>> {
        let authority = user
            .as_two_factor()
            .and_then(|model| model.two_factor_authority());
        match authority {
            Some(authority) => {
                let provider = self
                    .guard
                    .as_stateful()
                    .ok_or_else(stateful_required)?
                    .provider();
                authority.validate_account(user, provider.as_ref()).await?;
                authority.load().await.map(Some)
            }
            None if self.factor_authority_required => Err(Error::custom(
                "fortify_factor_authority_required",
                "当前账户未提供受信任因子authority",
            )),
            None => Ok(None),
        }
    }
    async fn challenge(
        &self,
        user: &AuthUser,
        remember: bool,
        authority: Option<&FactorAuthoritySnapshot>,
    ) -> Result<bool> {
        let challenged = if let Some(snapshot) = authority {
            snapshot.challenge_required(self.actions.confirmation_required()?)
        } else {
            match user.as_two_factor() {
                Some(model) => {
                    let state = model.two_factor_state().await?;
                    state
                        .secret
                        .as_ref()
                        .is_some_and(|secret| !secret.is_empty() && secret != "0")
                        && state.is_enabled(self.actions.confirmation_required()?)
                }
                None => false,
            }
        };
        if !challenged {
            return Ok(false);
        }
        self.session
            .mark_guard_authentication(&format!("fortify.challenge.{}", self.name), true)?;
        self.session
            .put_typed("login.id", user.auth_identifier()?)?;
        self.session.put_typed("login.remember", remember)?;
        self.session.put_typed("fortify.login.guard", &self.name)?;
        self.session
            .put_typed("fortify.login.provider", self.guard.provider_name())?;
        self.session.put_typed(
            "fortify.login.authority.source",
            authority.map(|snapshot| &snapshot.source),
        )?;
        self.session.put_typed(
            "fortify.login.authority.generation",
            authority.map(|snapshot| &snapshot.generation),
        )?;
        self.actions
            .events
            .dispatch(&TwoFactorAuthenticationChallenged { user: user.clone() })
            .await?;
        Ok(true)
    }
    async fn check_password_completion(&self) -> Result<()> {
        let Some(user) = self.guard.user().await? else {
            return if self.factor_authority_required {
                Err(Error::Unauthorized)
            } else {
                Ok(())
            };
        };
        let result = async {
            let snapshot = self.authority_snapshot(&user).await?;
            let confirm = self.actions.confirmation_required()?;
            if snapshot.is_some_and(|snapshot| snapshot.challenge_required(confirm)) {
                return Err(Error::Conflict(
                    "密码登录期间因子状态已变化，须重新完成挑战".into(),
                ));
            }
            Ok(())
        }
        .await;
        if result.is_err() {
            self.guard
                .as_stateful()
                .ok_or_else(stateful_required)?
                .logout()
                .await?;
        }
        result
    }

    /// 验证密码并决定正常登录或写入受限Session挑战，不签发JWT/refresh。
    pub async fn begin(
        &self,
        request: &RequestInfo,
        credentials: &Credentials,
        remember: bool,
    ) -> Result<FortifyLoginOutcome> {
        self.session.require_authenticated_storage()?;
        let stateful = self.guard.as_stateful().ok_or_else(stateful_required)?;
        let selected = Credentials::new(
            credentials
                .attributes()
                .iter()
                .filter(|(name, _)| name.as_str() == self.username || name.as_str() == "password")
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
        );
        if self.actions.feature_enabled()? || self.factor_authority_required {
            let user = if let Some(callback) = &self.authenticate_using {
                match callback(request, credentials).await? {
                    Some(user) => user,
                    None => return self.failed(None, credentials).await,
                }
            } else {
                // RedirectIfTwoFactorAuthenticatable的Timebox未提前返回，成功和错误都等待。
                let deadline = Instant::now() + Duration::from_millis(200);
                let result = async {
                    let provider = stateful.provider();
                    let user = provider.retrieve_by_credentials(&selected).await?;
                    match user {
                        Some(user)
                            if provider
                                .validate_credentials(user.as_ref(), &selected)
                                .await? =>
                        {
                            if self.rehash_on_login {
                                provider
                                    .rehash_password_if_required(user.as_ref(), &selected, false)
                                    .await?;
                            }
                            Ok(user)
                        }
                        user => self.failed(user, &selected).await,
                    }
                }
                .await;
                tokio::time::sleep_until(deadline).await;
                result?
            };
            let authority = self.authority_snapshot(&user).await?;
            if self.challenge(&user, remember, authority.as_ref()).await? {
                return Ok(FortifyLoginOutcome::TwoFactorChallenge);
            }
        }
        // 保留官方后继AttemptToAuthenticate调用链，不以一次密码查询替代Guard事件和策略。
        if let Some(callback) = &self.authenticate_using {
            match callback(request, credentials).await? {
                Some(user) => {
                    let authority = self.authority_snapshot(&user).await?;
                    if authority.is_some()
                        && self.challenge(&user, remember, authority.as_ref()).await?
                    {
                        return Ok(FortifyLoginOutcome::TwoFactorChallenge);
                    }
                    stateful
                        .login_with_session_regeneration(user, remember)
                        .await?;
                }
                None => return self.failed(None, credentials).await,
            }
        } else if !stateful
            .attempt_with_session_regeneration(&selected, remember)
            .await?
        {
            return Err(self.failed_validation());
        }
        self.check_password_completion().await?;
        Ok(FortifyLoginOutcome::Authenticated)
    }

    fn failed_validation(&self) -> Error {
        let mut errors = ValidationErrors::new();
        errors.add(
            &self.username,
            "These credentials do not match our records.",
        );
        Error::Validation(errors)
    }

    async fn failed<T>(&self, user: Option<AuthUser>, credentials: &Credentials) -> Result<T> {
        self.actions
            .events
            .dispatch(&crate::auth::Failed {
                guard: self.name.clone(),
                user,
                credentials: credentials.clone(),
            })
            .await?;
        Err(self.failed_validation())
    }

    /// 使用当前Guard的Provider重取挑战账户；缺失/串来源返回None，不建立身份。
    pub async fn challenged_user(&self) -> Result<Option<AuthUser>> {
        self.session.require_authenticated_storage()?;
        let Some(id) = self.session.get_typed::<Value>("login.id")? else {
            return Ok(None);
        };
        let saved_guard = self.session.get_typed::<String>("fortify.login.guard")?;
        let saved_provider = self
            .session
            .get_typed::<Option<String>>("fortify.login.provider")?;
        if saved_guard.as_deref() != Some(&self.name)
            || saved_provider.as_ref().map(|p| p.as_deref()) != Some(self.guard.provider_name())
        {
            return Ok(None);
        }
        let Some(user) = self
            .guard
            .as_stateful()
            .ok_or_else(stateful_required)?
            .provider()
            .retrieve_by_id(&id)
            .await?
        else {
            return Ok(None);
        };
        let authority = self.authority_snapshot(&user).await?;
        if let Some(snapshot) = authority {
            let source = self
                .session
                .get_typed::<Option<String>>("fortify.login.authority.source")?
                .flatten();
            let generation = self
                .session
                .get_typed::<Option<String>>("fortify.login.authority.generation")?
                .flatten();
            if source.as_deref() != Some(snapshot.source.as_str())
                || generation.as_deref() != Some(snapshot.generation.as_str())
            {
                return Ok(None);
            }
        } else if self
            .session
            .get_typed::<Option<String>>("fortify.login.authority.source")?
            .flatten()
            .is_some()
        {
            return Ok(None);
        }
        Ok(Some(user))
    }

    /// 恢复码优先于TOTP；成功后替换恢复码，再经同一Guard登录并轮换Session。
    // 恢复码优先、事务消费、挑战移除及receipt登录必须顺序可见，不能拆散失败/取消边界。
    #[allow(clippy::too_many_lines)]
    pub async fn complete(&self, code: Option<&str>, recovery_code: Option<&str>) -> Result<()> {
        let user = self.challenged_user().await?.ok_or(Error::Unauthorized)?;
        let model = capability(&user)?;
        let authority_snapshot = self.authority_snapshot(&user).await?;
        if let Some(snapshot) = &authority_snapshot {
            let source = self
                .session
                .get_typed::<Option<String>>("fortify.login.authority.source")?
                .flatten();
            let generation = self
                .session
                .get_typed::<Option<String>>("fortify.login.authority.generation")?
                .flatten();
            if source.as_deref() != Some(snapshot.source.as_str())
                || generation.as_deref() != Some(snapshot.generation.as_str())
            {
                return Err(Error::Unauthorized);
            }
        }
        let before = match &authority_snapshot {
            Some(snapshot) => snapshot.state.clone(),
            None => model.two_factor_state().await?,
        };
        let encrypter = self.actions.encrypter(model).await?;
        let mut verified_authority = None;
        let mut method = FactorVerificationMethod::Totp;
        let mut matched_recovery = None;
        if let (Some(input), Some(ciphertext)) = (
            recovery_code.filter(|s| !s.is_empty() && *s != "0"),
            before.recovery_codes.as_ref(),
        ) {
            let json = encrypter.decrypt_serialized_string(ciphertext)?;
            let stored: Zeroizing<Vec<String>> =
                Zeroizing::new(serde_json::from_slice(&json).map_err(Error::internal)?);
            for stored in stored.iter() {
                if bool::from(stored.as_bytes().ct_eq(input.as_bytes())) {
                    matched_recovery = Some(RecoveryCode::new(stored.clone()));
                    break;
                }
            }
        }
        if let Some(recovery) = matched_recovery {
            // 官方Request匹配即消费，后续随机源/加密/持久化失败也不保留挑战。
            self.session.remove("login.id")?;
            let original = before.recovery_codes.as_ref().ok_or(Error::Unauthorized)?;
            let json = super::actions::decrypt_string(encrypter.as_ref(), original)?;
            let replacement_code = Zeroizing::new((self.actions.recovery_generator)()?);
            // 固定trait以str_replace作用于JSON原文；保留公开自定义生成器下的替换语义。
            let replaced = Zeroizing::new(json.replace(recovery.expose(), &replacement_code));
            let mut after = before.clone();
            after.recovery_codes = Some(encrypter.encrypt_serialized_string(replaced.as_bytes())?);
            if let Some(snapshot) = &authority_snapshot {
                verified_authority = Some(
                    model
                        .two_factor_authority()
                        .ok_or(Error::Unauthorized)?
                        .consume_modern(snapshot, Some(&after), None)
                        .await?,
                );
            } else {
                super::actions::replace(model, &before, &after).await?;
            }
            method = FactorVerificationMethod::RecoveryCode;
            self.actions
                .events
                .dispatch(&RecoveryCodeReplaced {
                    user: user.clone(),
                    code: recovery,
                })
                .await?;
        } else {
            let verification = match (
                code.filter(|s| !s.is_empty() && *s != "0"),
                before.secret.as_ref(),
            ) {
                (Some(code), Some(ciphertext)) => {
                    let secret = super::actions::decrypt_string(encrypter.as_ref(), ciphertext)?;
                    self.actions
                        .provider
                        .verify_with_context(&secret, code)
                        .await?
                }
                _ => super::TwoFactorCodeVerification::rejected(),
            };
            if !verification.is_valid() {
                self.actions
                    .events
                    .dispatch(&TwoFactorAuthenticationFailed { user })
                    .await?;
                let mut errors = ValidationErrors::new();
                errors.add(
                    "code",
                    "The provided two factor authentication code was invalid.",
                );
                return Err(Error::Validation(errors));
            }
            self.session.remove("login.id")?;
            if let Some(snapshot) = &authority_snapshot {
                verified_authority = Some(
                    model
                        .two_factor_authority()
                        .ok_or(Error::Unauthorized)?
                        .clone()
                        .with_provider_verification(&verification)?
                        .consume_modern(snapshot, None, code)
                        .await?,
                );
            }
        }
        let verified_at = u64::try_from(chrono::Utc::now().timestamp()).map_err(Error::internal)?;
        self.actions
            .events
            .dispatch(&ValidTwoFactorAuthenticationCodeProvided { user: user.clone() })
            .await?;
        let remember = self
            .session
            .pull_json("login.remember")?
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let stateful = self.guard.as_stateful().ok_or_else(stateful_required)?;
        if let Some(snapshot) = verified_authority {
            let receipt = FactorReceipt::issue(
                user.auth_identifier()?,
                self.name.clone(),
                self.guard.provider_name().map(str::to_owned),
                snapshot.source,
                snapshot.generation,
                method,
                verified_at,
            )?;
            stateful
                .login_with_factor_receipt(user, remember, receipt)
                .await?;
        } else {
            stateful
                .login_with_session_regeneration(user, remember)
                .await?;
        }
        Ok(())
    }
}

fn stateful_required() -> Error {
    Error::custom(
        "fortify_stateful_guard_required",
        "Fortify需要StatefulGuard",
    )
}

macro_rules! user_event {
    ($name:ident, $description:literal) => {
        #[doc = $description]
        #[derive(Clone)]
        pub struct $name {
            /// 当前Provider返回的同一账户。
            pub user: AuthUser,
        }
        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.debug_struct(stringify!($name))
                    .field("user", &"[REDACTED]")
                    .finish()
            }
        }
    };
}
user_event!(
    TwoFactorAuthenticationChallenged,
    "已建立密码首因子挑战，尚未正式登录。"
);

impl crate::events::Event for TwoFactorAuthenticationChallenged {
    const NAME: &'static str = r"OpenRust\Fortify\Events\TwoFactorAuthenticationChallenged";
}

user_event!(
    TwoFactorAuthenticationFailed,
    "双因素验证码或恢复码验证失败。"
);

impl crate::events::Event for TwoFactorAuthenticationFailed {
    const NAME: &'static str = r"OpenRust\Fortify\Events\TwoFactorAuthenticationFailed";
}

user_event!(
    ValidTwoFactorAuthenticationCodeProvided,
    "双因素验证成功，即将通过同一StatefulGuard登录。"
);

impl crate::events::Event for ValidTwoFactorAuthenticationCodeProvided {
    const NAME: &'static str = r"OpenRust\Fortify\Events\ValidTwoFactorAuthenticationCodeProvided";
}

#[cfg(test)]
#[path = "../../../tests/unit/auth/fortify_login.rs"]
mod tests;
