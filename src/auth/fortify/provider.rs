//! Fortify双因素服务登记，复用唯一ConfigRepository和Container。

use std::sync::Arc;

use crate::{
    Result,
    cache::Cache,
    config::repository::SectionSchema as S,
    core::{BootContext, BoxFuture, RegisterContext, ServiceProvider},
    events::Events,
    security::Encryptor,
};

use super::{
    FortifyConfig, GoogleTwoFactorAuthenticationProvider, TwoFactorActions,
    TwoFactorAuthenticationProvider, TwoFactorOptions,
};

/// 官方双因素服务Provider；应用显式登记，并通过普通Container覆盖公开契约。
///
/// 需要Auth、Session、Events、Cache与Encryption服务。路由装配和用户模型接入
/// 不通过此类型悄悄创建第二份账户或独立应用。
#[derive(Default)]
pub struct FortifyServiceProvider;

impl FortifyServiceProvider {
    /// 创建双因素服务登记器。
    pub fn new() -> Self {
        Self
    }
}

impl ServiceProvider for FortifyServiceProvider {
    fn register<'a>(&'a self, app: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.config_section(
                "fortify",
                FortifyConfig::default(),
                S::object([
                    ("guard", S::String),
                    ("auth_middleware", S::String),
                    ("username", S::String),
                    ("home", S::String),
                    ("prefix", S::String),
                    ("domain", S::String.nullable()),
                    ("middleware", S::array(S::String)),
                    ("views", S::Boolean),
                    ("features", S::Any),
                    ("lowercase_usernames", S::Boolean),
                    (
                        "limiters",
                        S::object([
                            ("login", S::String.nullable()),
                            ("two-factor", S::String.nullable()),
                        ]),
                    ),
                    ("paths", S::map(S::Any)),
                    ("redirects", S::map(S::String.nullable())),
                ])
                .open(),
                |_| Ok(()),
            )?;
            super::responses::register(app).await?;
            register_credential_authority(app).await?;
            app.config_section(
                "fortify-options.two-factor-authentication",
                serde_json::to_value(TwoFactorOptions::default())
                    .map_err(crate::Error::internal)?,
                S::object([
                    ("confirm", S::Any),
                    ("confirmPassword", S::Any),
                    ("secret-length", S::Any),
                    ("window", S::Any),
                ]),
                |_| Ok(()),
            )?;
            if !app.is_bound::<dyn TwoFactorAuthenticationProvider>() {
                let provider_config = app.config_repository();
                app.singleton::<dyn TwoFactorAuthenticationProvider, _>(move |resolver| {
                    let provider_config = provider_config.clone();
                    Box::pin(async move {
                        let cache = resolver.resolve::<Cache>().await?;
                        let provider =
                            GoogleTwoFactorAuthenticationProvider::new(Some((*cache).clone()))
                                .with_config_repository(provider_config);
                        Ok(Arc::new(provider) as Arc<dyn TwoFactorAuthenticationProvider>)
                    })
                })
                .await?;
            }
            if !app.is_bound::<TwoFactorActions>() {
                let actions_config = app.config_repository();
                app.scoped::<TwoFactorActions, _>(move |resolver| {
                    let actions_config = actions_config.clone();
                    Box::pin(async move {
                        let provider = resolver
                            .resolve::<dyn TwoFactorAuthenticationProvider>()
                            .await?;
                        let encrypter = resolver.resolve::<Encryptor>().await?;
                        let events = resolver.resolve::<Events>().await?;
                        Ok(Arc::new(
                            TwoFactorActions::new(
                                provider,
                                (*encrypter).clone(),
                                (*events).clone(),
                            )
                            .with_config_repository(actions_config),
                        ))
                    })
                })
                .await?;
            }
            if !app.is_bound::<super::FortifyRateLimiters>() {
                app.instance(Arc::new(super::FortifyRateLimiters::new()))
                    .await?;
            }
            super::runtime::register(app).await?;
            Ok(())
        })
    }
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let config = app.section::<FortifyConfig>("fortify")?;
            let options = TwoFactorOptions {
                confirm_password: super::options::password_confirmation(&app.config_repository())?,
                ..TwoFactorOptions::default()
            };
            let require_authority = app.is_bound::<super::RequireFactorAuthority>();
            app.routes(super::http::route_builder(
                config,
                &options,
                require_authority,
            )?)?;
            Ok(())
        })
    }
}

async fn register_credential_authority(app: &mut RegisterContext<'_>) -> Result<()> {
    let credential_config = app.config_repository();
    app.singleton::<super::CredentialAuthority, _>(move |resolver| {
        let config = credential_config.clone();
        Box::pin(async move {
            let db = resolver.resolve::<crate::db::Db>().await?;
            let encrypter = resolver.resolve::<Encryptor>().await?;
            let confirm = super::options::confirmation_required(&config)?;
            Ok(Arc::new(
                super::CredentialAuthority::new((*db).clone(), (*encrypter).clone(), confirm)
                    .with_config_repository(config),
            ))
        })
    })
    .await?;
    Ok(())
}
