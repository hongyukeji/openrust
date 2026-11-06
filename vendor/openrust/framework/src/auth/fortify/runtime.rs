//! 请求级Fortify装配，不缓存全局已登录账户。

use std::{fmt, sync::Arc};

use serde_json::{Map, Value};

use super::{AuthenticateUsing, FortifyConfig, FortifyLogin, TwoFactorActions, TwoFactorOptions};
use crate::Error;
use crate::Result;
use crate::auth::AuthManager;
use crate::auth::AuthUser;
use crate::auth::Credentials;
use crate::auth::Guard;
use crate::config::AppConfig;
use crate::container::Resolver;
use crate::foundation::RegisterContext;
use crate::http::Request;
use crate::support::BoxFuture;

/// 应用在同一Container中显式选择统一因子保护，不受官方路由开关静默解除。
///
/// 选定后每个账户必须具备对应authority；关闭新绑定功能不静默解除已有因子保护。
#[derive(Clone, Copy, Debug, Default)]
pub struct RequireFactorAuthority;

/// 对应confirmPasswordsUsing公开回调，不要求默认登录字段可用。
pub type ConfirmPasswordsUsing =
    Arc<dyn Fn(AuthUser, Option<String>) -> BoxFuture<'static, Result<bool>> + Send + Sync>;

/// 同一Container内的Fortify认证回调；由应用scoped或singleton绑定。
#[derive(Clone, Default)]
pub struct FortifyCallbacks {
    pub(super) authenticate: Option<AuthenticateUsing>,
    pub(super) confirm_passwords: Option<ConfirmPasswordsUsing>,
}
impl fmt::Debug for FortifyCallbacks {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FortifyCallbacks").finish_non_exhaustive()
    }
}
impl FortifyCallbacks {
    /// 默认使用同一Guard/Provider的官方查询与密码验证。
    pub fn new() -> Self {
        Self::default()
    }
    /// 对应authenticateUsing，适合组合账号入口，不在回调中提前登录。
    #[must_use]
    pub fn authenticate_using(mut self, callback: AuthenticateUsing) -> Self {
        self.authenticate = Some(callback);
        self
    }
    /// 对应confirmPasswordsUsing，不增加每个动作重新提交密码的交互。
    #[must_use]
    pub fn confirm_passwords_using(mut self, callback: ConfirmPasswordsUsing) -> Self {
        self.confirm_passwords = Some(callback);
        self
    }
}

/// 同请求Guard、动作、配置与可替换响应的装配实例。
pub struct Fortify {
    pub(super) resolver: Resolver,
    pub(super) config: FortifyConfig,
    pub(super) actions: Arc<TwoFactorActions>,
    pub(super) manager: Arc<AuthManager>,
    pub(super) callbacks: Arc<FortifyCallbacks>,
    pub(super) password_timeout: u64,
    pub(super) rehash_on_login: bool,
    pub(super) app_name: String,
    pub(super) require_factor_authority: bool,
}
impl fmt::Debug for Fortify {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Fortify")
            .field("guard", &self.config.guard)
            .finish_non_exhaustive()
    }
}
impl Fortify {
    pub(super) async fn management(&self) -> Result<(AuthUser, TwoFactorActions)> {
        let guard = self.guard().await?;
        let account = guard.user().await?.ok_or(Error::Unauthorized)?;
        let authority = account
            .as_two_factor()
            .and_then(super::TwoFactorAuthenticatable::two_factor_authority);
        if self.require_factor_authority || authority.is_some() {
            let authority = authority.ok_or(Error::Unauthorized)?;
            let stateful = guard.as_stateful().ok_or(Error::Unauthorized)?;
            authority
                .validate_account(&account, stateful.provider().as_ref())
                .await?;
            let context = stateful.factor_management_context().await?;
            let snapshot = authority.load().await?;
            let confirm = self.actions.confirmation_required()?;
            self.actions
                .configured_authority(authority)
                .authorize_management(&snapshot, &context, confirm)
                .await?;
            Ok((
                account,
                self.actions.with_management_context(context, confirm),
            ))
        } else {
            Ok((account, (*self.actions).clone()))
        }
    }
    /// 当前绑定确认要求；仅读取相关策略，不解析密钥生成长度。
    pub fn confirms_two_factor_authentication(&self) -> Result<bool> {
        self.actions.confirms_two_factor_authentication()
    }

    /// 返回已由同一配置仓储求值的HTTP配置。
    pub fn config(&self) -> &FortifyConfig {
        &self.config
    }
    /// 同一当前仓储快照的有效选项；确认开关受功能清单约束，负密钥长度会报错。
    /// 仅读取确认策略时使用 `confirms_two_factor_authentication`，避免解析无关生成参数。
    pub fn two_factor_options(&self) -> Result<TwoFactorOptions> {
        self.actions.options_snapshot()
    }
    /// 当前请求实际配置Guard，不从另一个账户来源创建身份。
    pub async fn guard(&self) -> Result<Arc<dyn Guard>> {
        self.manager.guard(Some(&self.config.guard)).await
    }
    pub(super) async fn login(&self, request: &Request) -> Result<FortifyLogin> {
        let login = FortifyLogin::new(
            &self.manager,
            &self.config.guard,
            request.session()?,
            self.actions.clone(),
        )
        .await?
        .with_username(&self.config.username)
        .with_rehash_on_login(self.rehash_on_login)
        .with_factor_authority_required(self.require_factor_authority);
        Ok(match &self.callbacks.authenticate {
            Some(callback) => login.with_authenticate_using(callback.clone()),
            None => login,
        })
    }
    pub(super) async fn confirm_password(
        &self,
        user: AuthUser,
        password: Option<String>,
    ) -> Result<bool> {
        if let Some(callback) = &self.callbacks.confirm_passwords {
            return callback(user, password).await;
        }
        let username = user
            .auth_attribute(&self.config.username)?
            .unwrap_or(Value::Null);
        let mut attributes = Map::new();
        attributes.insert(self.config.username.clone(), username);
        attributes.insert(
            "password".into(),
            password.map_or(Value::Null, Value::String),
        );
        self.guard()
            .await?
            .validate(&Credentials::new(attributes))
            .await
    }
}

pub(super) async fn register(app: &mut RegisterContext<'_>) -> Result<()> {
    if !app.is_bound::<FortifyCallbacks>() {
        app.instance(Arc::new(FortifyCallbacks::default())).await?;
    }
    app.scoped::<Fortify, _>(|resolver| {
        Box::pin(async move {
            let config = resolver.section::<FortifyConfig>("fortify")?;
            let app_config = resolver.section::<AppConfig>("app")?;
            let password_timeout = resolver.section::<u64>("auth.password_timeout")?;
            let rehash_on_login = resolver.section::<bool>("hashing.rehash_on_login")?;
            let actions = resolver.resolve::<TwoFactorActions>().await?;
            let manager = resolver.resolve::<AuthManager>().await?;
            let callbacks = resolver.resolve::<FortifyCallbacks>().await?;
            let require_factor_authority = resolver.is_bound::<RequireFactorAuthority>()?;
            if config.guard.is_empty() || config.username.is_empty() {
                return Err(Error::Config(crate::config::ConfigError::Invalid(
                    "fortify.guard与username不得为空".into(),
                )));
            }
            Ok(Arc::new(Fortify {
                resolver,
                config,
                actions,
                manager,
                callbacks,
                password_timeout,
                rehash_on_login,
                app_name: app_config.name,
                require_factor_authority,
            }))
        })
    })
    .await?;
    Ok(())
}
