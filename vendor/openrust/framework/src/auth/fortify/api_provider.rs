//! 显式 JWT API profile 的服务与无状态具名 Guard 装配。

use super::api_audit::ApiFactorManagementAuditSink;
use super::{
    CredentialAuthority, FortifyCallbacks, FortifyConfig, FortifyJwtApi, FortifyJwtApiConfig,
    FortifyServiceProvider, RequireFactorAuthority, TwoFactorActions,
};
use crate::Error;
use crate::Result;
use crate::auth::AuthDrivers;
use crate::auth::AuthManager;
use crate::auth::AuthUser;
use crate::auth::Credentials;
use crate::auth::Guard;
use crate::auth::GuardConfig;
use crate::config::repository::SectionSchema as S;
use crate::foundation::BootContext;
use crate::foundation::RegisterContext;
use crate::support::{BoxFuture, ServiceProvider};
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

/// 安装明确选择的无 Cookie JWT API 扩展；官方 Web Provider 的默认行为保持独立。
///
/// 应用须以此类型替换 Web Fortify Provider，并显式配置 jwt-api Guard/Provider。
#[derive(Default)]
pub struct FortifyJwtApiServiceProvider {
    database_audit: bool,
}
impl FortifyJwtApiServiceProvider {
    /// 创建显式 API profile，不隐式选择审计表。
    #[must_use]
    pub const fn new() -> Self {
        Self {
            database_audit: false,
        }
    }
    /// 明确选用同数据库的通用脱敏事务审计；应用迁移须包含 `audit_records`。
    #[must_use]
    pub const fn with_database_audit(mut self) -> Self {
        self.database_audit = true;
        self
    }
}
impl ServiceProvider for FortifyJwtApiServiceProvider {
    fn register<'a>(&'a self, app: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            // 复用同一 register 内核；不调用官方 Web boot 或挂载其路由。
            FortifyServiceProvider::new().register(app).await?;
            app.instance(Arc::new(RequireFactorAuthority)).await?;
            app.config_section(
                "fortify-api",
                FortifyJwtApiConfig::default(),
                S::object([("guard", S::String), ("prefix", S::String)]),
                |_| Ok(()),
            )?;
            let drivers = app.resolve::<AuthDrivers>().await?;
            drivers.extend("jwt-api", |context| {
                Box::pin(async move {
                    let service = context.resolver.resolve::<FortifyJwtApi>().await?;
                    if context.name != service.scope.guard
                        || context.config.provider.as_deref()
                            != Some(service.scope.provider.as_str())
                    {
                        return Err(Error::Unauthorized);
                    }
                    let request = context.request().await?;
                    Ok(Arc::new(JwtApiGuard {
                        service,
                        request,
                        has_user: AtomicBool::new(false),
                    }) as Arc<dyn Guard>)
                })
            })?;
            app.scoped::<FortifyJwtApi, _>(|resolver| {
                Box::pin(async move {
                    let profile = resolver.section::<FortifyJwtApiConfig>("fortify-api")?;
                    let guards =
                        resolver.section::<BTreeMap<String, GuardConfig>>("auth.guards")?;
                    let guard = guards.get(&profile.guard).ok_or_else(profile_invalid)?;
                    if guard.driver != "jwt-api" {
                        return Err(profile_invalid());
                    }
                    let provider_name = guard
                        .provider
                        .clone()
                        .filter(|name| !name.is_empty())
                        .ok_or_else(profile_invalid)?;
                    let manager = resolver.resolve::<AuthManager>().await?;
                    let provider = manager
                        .create_user_provider(Some(&provider_name))
                        .await?
                        .ok_or_else(profile_invalid)?;
                    let authority = resolver.resolve::<CredentialAuthority>().await?;
                    if !provider.uses_database_accounts(authority.db(), "users")? {
                        return Err(Error::Unauthorized);
                    }
                    let tokens = resolver.resolve::<crate::auth::JwtTokens>().await?;
                    let (issuer, audience) = tokens.credential_domain();
                    Ok(Arc::new(FortifyJwtApi {
                        scope: crate::auth::credential::CredentialGuardBinding {
                            issuer: issuer.into(),
                            audience: audience.into(),
                            guard: profile.guard,
                            provider: provider_name,
                        },
                        provider,
                        flows: resolver.resolve::<crate::auth::AuthFlows>().await?,
                        authority,
                        tokens,
                        callbacks: resolver.resolve::<FortifyCallbacks>().await?,
                        config: resolver.section::<FortifyConfig>("fortify")?,
                        rehash: resolver.section::<bool>("hashing.rehash_on_login")?,
                        app_name: resolver.section::<crate::config::AppConfig>("app")?.name,
                        timebox: Duration::from_micros(
                            resolver.section::<u64>("auth.timebox_duration")?,
                        ),
                    }))
                })
            })
            .await?;
            register_management(app).await?;
            if self.database_audit {
                register_database_audit(app).await?;
            }
            Ok(())
        })
    }
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let profile = app.section::<FortifyJwtApiConfig>("fortify-api")?;
            if profile.prefix.is_empty()
                || !profile.prefix.starts_with('/')
                || profile.prefix.contains(['?', '#'])
            {
                return Err(profile_invalid());
            }
            let api = app.section::<crate::config::ApiConfig>("api")?;
            let prefix = format!(
                "{}{}",
                api.normalized_prefix()?.trim_end_matches('/'),
                profile.prefix.trim_end_matches('/')
            );
            let error_prefix = prefix.clone();
            app.exceptions(move |handler| handler.no_store_errors_under(error_prefix))?;
            app.routes(super::api_http::routes(prefix, profile.guard))?;
            Ok(())
        })
    }
}
async fn register_management(app: &mut RegisterContext<'_>) -> Result<()> {
    app.scoped::<super::JwtApiFactorManagement, _>(|resolver| {
        Box::pin(async move {
            let api = resolver.resolve::<FortifyJwtApi>().await?;
            let mut actions = (*resolver.resolve::<TwoFactorActions>().await?).clone();
            if resolver.is_bound::<dyn ApiFactorManagementAuditSink>()? {
                let audit = resolver
                    .resolve::<dyn ApiFactorManagementAuditSink>()
                    .await?;
                audit.require_management_store(api.authority.db())?;
                actions = actions.with_api_management_audit(audit);
            }
            let encrypter = resolver
                .resolve::<crate::encryption::ContextualEncrypter>()
                .await?;
            let options = super::JwtApiFactorManagementOptions {
                encrypter: (*encrypter).clone(),
                guard: api.scope.guard.clone(),
                provider_name: Some(api.scope.provider.clone()),
                confirm_passwords: api.callbacks.confirm_passwords.clone(),
            };
            Ok(Arc::new(super::JwtApiFactorManagement::new(
                (*api.authority).clone(),
                actions,
                (*api.tokens).clone(),
                api.provider.clone(),
                options,
            )?))
        })
    })
    .await
}

async fn register_database_audit(app: &mut RegisterContext<'_>) -> Result<()> {
    if !app.is_bound::<super::DatabaseApiLoginAudit>() {
        app.singleton::<super::DatabaseApiLoginAudit, _>(|resolver| {
            Box::pin(async move {
                let db = resolver.resolve::<crate::database::Db>().await?;
                let audit = resolver.resolve::<crate::audit::Audit>().await?;
                Ok(Arc::new(super::DatabaseApiLoginAudit::new(
                    (*db).clone(),
                    (*audit).clone(),
                )))
            })
        })
        .await?;
    }
    if !app.is_bound::<dyn crate::auth::ApiLoginAuditSink>() {
        app.singleton::<dyn crate::auth::ApiLoginAuditSink, _>(|resolver| {
            Box::pin(async move {
                Ok(resolver.resolve::<super::DatabaseApiLoginAudit>().await?
                    as Arc<dyn crate::auth::ApiLoginAuditSink>)
            })
        })
        .await?;
    }
    if !app.is_bound::<dyn ApiFactorManagementAuditSink>() {
        app.singleton::<dyn ApiFactorManagementAuditSink, _>(|resolver| {
            Box::pin(async move {
                Ok(resolver.resolve::<super::DatabaseApiLoginAudit>().await?
                    as Arc<dyn ApiFactorManagementAuditSink>)
            })
        })
        .await?;
    }
    Ok(())
}

fn profile_invalid() -> Error {
    Error::custom(
        "fortify_jwt_api_profile_invalid",
        "JWT API profile需要明确的jwt-api Guard、Provider及绝对路由前缀",
    )
}
struct JwtApiGuard {
    service: Arc<FortifyJwtApi>,
    request: Arc<crate::http::RequestInfo>,
    has_user: AtomicBool,
}
impl Guard for JwtApiGuard {
    fn provider_name(&self) -> Option<&str> {
        Some(&self.service.scope.provider)
    }
    fn user(&self) -> BoxFuture<'_, Result<Option<AuthUser>>> {
        Box::pin(async move {
            let Some(raw) = super::api::bearer(&self.request)? else {
                self.has_user.store(false, Ordering::Release);
                return Ok(None);
            };
            let (user, _) = self.service.authenticate(raw).await?;
            self.has_user.store(true, Ordering::Release);
            Ok(Some(user))
        })
    }
    fn validate<'a>(&'a self, credentials: &'a Credentials) -> BoxFuture<'a, Result<bool>> {
        Box::pin(self.service.validate_password(credentials))
    }
    fn has_user(&self) -> Result<bool> {
        Ok(self.has_user.load(Ordering::Acquire))
    }
    fn set_user(&self, _user: AuthUser) -> BoxFuture<'_, Result<()>> {
        Box::pin(async {
            Err(Error::custom(
                "jwt_api_signed_identity_required",
                "JWT API Guard仅接受真实签名及当前凭据家族",
            ))
        })
    }
}
