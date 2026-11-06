//! 生成JWT API应用以公开回调实现已验证邮箱策略；不改变框架的可选邮箱默认。

use openrust::auth::fortify::{FortifyCallbacks, FortifyConfig};
use openrust::auth::{AuthManager, AuthUser, Credentials, UserProvider};
use openrust::foundation::RegisterContext;
use openrust::{Error, Result};
use serde_json::{Map, Value};
use std::{sync::Arc, time::Duration};

pub(super) async fn register(app: &mut RegisterContext<'_>) -> Result<()> {
    app.instance(Arc::new(openrust::auth::fortify::RequireFactorAuthority))
        .await?;
    app.scoped::<FortifyCallbacks, _>(|resolver| {
        Box::pin(async move {
            let config = resolver.section::<FortifyConfig>("fortify")?;
            let manager = resolver.resolve::<AuthManager>().await?;
            let profile =
                resolver.section::<openrust::auth::fortify::FortifyJwtApiConfig>("fortify-api")?;
            let guards = resolver
                .section::<std::collections::BTreeMap<String, openrust::auth::GuardConfig>>(
                    "auth.guards",
                )?;
            let provider_name = guards
                .get(&profile.guard)
                .and_then(|guard| guard.provider.as_deref())
                .ok_or(Error::Unauthorized)?;
            let provider = manager
                .create_user_provider(Some(provider_name))
                .await?
                .ok_or(Error::Unauthorized)?;
            let rehash = resolver.section::<bool>("hashing.rehash_on_login")?;
            let login_provider = provider.clone();
            let username = config.username;
            let callbacks = FortifyCallbacks::new()
                .authenticate_using(Arc::new(move |_, credentials| {
                    let provider = login_provider.clone();
                    let username = username.clone();
                    Box::pin(async move {
                        let deadline =
                            openrust::tokio::time::Instant::now() + Duration::from_millis(200);
                        let result =
                            authenticate(provider.as_ref(), credentials, &username, rehash).await;
                        openrust::tokio::time::sleep_until(deadline).await;
                        result
                    })
                }))
                .confirm_passwords_using(Arc::new(move |user, password| {
                    let provider = provider.clone();
                    Box::pin(async move {
                        let mut attributes = Map::new();
                        attributes.insert(
                            "password".into(),
                            password.map_or(Value::Null, Value::String),
                        );
                        provider
                            .validate_credentials(user.as_ref(), &Credentials::new(attributes))
                            .await
                    })
                }));
            Ok(Arc::new(callbacks))
        })
    })
    .await
}

async fn authenticate(
    provider: &dyn UserProvider,
    credentials: &Credentials,
    field: &str,
    rehash: bool,
) -> Result<Option<AuthUser>> {
    let Some(account) = credentials
        .attributes()
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };
    let lookup = Credentials::new(Map::from_iter([(
        "email".into(),
        Value::String(account.into()),
    )]));
    let Some(user) = provider.retrieve_by_credentials(&lookup).await? else {
        return Ok(None);
    };
    // 仅此生成应用选择邮箱登录/验证业务约束；框架API仍支持无邮箱账户。
    if user
        .auth_attribute("email")?
        .as_ref()
        .and_then(Value::as_str)
        != Some(account)
        || !has_verified_email(&user)?
    {
        return Ok(None);
    }
    if !provider
        .validate_credentials(user.as_ref(), credentials)
        .await?
    {
        return Ok(None);
    }
    if rehash {
        provider
            .rehash_password_if_required(user.as_ref(), credentials, false)
            .await?;
    }
    let identifier = user.auth_identifier()?;
    let Some(current) = provider.retrieve_by_id(&identifier).await? else {
        return Ok(None);
    };
    if current.auth_identifier()? != identifier
        || current
            .auth_attribute("email")?
            .as_ref()
            .and_then(Value::as_str)
            != Some(account)
        || !has_verified_email(&current)?
        || (current.auth_password()? != user.auth_password()?
            && !provider
                .validate_credentials(current.as_ref(), credentials)
                .await?)
    {
        return Ok(None);
    }
    Ok(Some(current))
}

fn has_verified_email(user: &AuthUser) -> Result<bool> {
    Ok(user
        .auth_attribute("email_verified_at")?
        .is_some_and(|value| !value.is_null()))
}
