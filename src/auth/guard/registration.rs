//! 认证 driver 注册复用 `AuthServiceProvider` 与请求 scope。

use super::{
    AuthDefaults, AuthDrivers, AuthManager, DatabaseUserProvider, Guard, GuardConfig, SessionGuard,
    UserProvider, UserProviderConfig,
};

use crate::{
    Error, Result,
    config::{AppConfig, repository::SectionSchema as S},
    core::{RegisterContext, container::Scope},
    db::Db,
    http::{CookieJar, axum::http::StatusCode, cookie::CookieProtection},
    session::Session,
};
use serde_json::{Map, Value, json};
use std::{
    collections::BTreeMap,
    sync::{Arc, OnceLock},
};

pub(crate) struct AuthRequestContext {
    pub cookies: Value,
    pub info: Arc<crate::http::RequestInfo>,
    pub protection: Option<CookieProtection>,
}
#[derive(Default)]
struct AuthRequestSlot(OnceLock<AuthRequestContext>);

pub(crate) async fn install_request(scope: &Scope, context: AuthRequestContext) -> Result<()> {
    let slot = scope.resolve::<AuthRequestSlot>().await?;
    if slot.0.get().is_none() {
        slot.0
            .set(context)
            .map_err(|_| Error::custom("auth_request_already_bound", "当前认证请求上下文已绑定"))?;
    }
    Ok(())
}

pub(in crate::auth) async fn request_info(
    resolver: &crate::core::container::Resolver,
) -> Result<Arc<crate::http::RequestInfo>> {
    let slot = resolver.resolve::<AuthRequestSlot>().await?;
    Ok(slot
        .0
        .get()
        .ok_or_else(|| {
            Error::custom_with_status(
                StatusCode::INTERNAL_SERVER_ERROR,
                "auth_request_missing",
                "认证driver需要当前请求上下文",
            )
        })?
        .info
        .clone())
}

pub(crate) async fn session_user_id(scope: &Scope) -> Result<Option<sea_orm::Value>> {
    let slot = scope.resolve::<AuthRequestSlot>().await?;
    // 旧显式凭据路径尚未采用具名 Guard 时，保留其现有元数据适配。
    if slot.0.get().is_none() {
        return Ok(None);
    }
    let manager = scope.resolve::<AuthManager>().await?;
    let id = manager.guard(None).await?.id().await?;
    let id = match id {
        None => sea_orm::Value::BigInt(None),
        Some(Value::String(id)) if !id.trim().is_empty() => id.into(),
        Some(Value::Number(id)) => {
            if let Some(value) = id.as_i64() {
                value.into()
            } else if let Some(value) = id.as_u64() {
                value.into()
            } else {
                id.as_f64()
                    .ok_or_else(|| Error::custom("auth_identifier_invalid", "账户数字ID无效"))?
                    .into()
            }
        }
        Some(_) => {
            return Err(Error::custom(
                "auth_identifier_invalid",
                "账户ID必须为非空字符串或数字",
            ));
        }
    };
    Ok(Some(id))
}

pub(in crate::auth) async fn register(app: &mut RegisterContext<'_>) -> Result<()> {
    register_configuration(app)?;
    if !app.is_bound::<crate::auth::AuthenticationRedirects>() {
        app.instance(Arc::new(crate::auth::AuthenticationRedirects::default()))
            .await?;
    }
    if !app.is_bound::<AuthDrivers>() {
        app.instance(Arc::new(default_drivers(app.config_repository())?))
            .await?;
    }
    app.scoped::<AuthRequestSlot, _>(|_| {
        Box::pin(async { Ok(Arc::new(AuthRequestSlot::default())) })
    })
    .await?;
    app.scoped::<AuthManager, _>(|resolver| {
        Box::pin(async move {
            let drivers = resolver.resolve::<AuthDrivers>().await?;
            Ok(Arc::new(AuthManager::new(resolver, (*drivers).clone())))
        })
    })
    .await?;
    Ok(())
}

fn register_configuration(app: &mut RegisterContext<'_>) -> Result<()> {
    app.config_section(
        "auth.defaults",
        AuthDefaults {
            guard: "web".into(),
            passwords: "users".into(),
            provider: None,
        },
        S::object([
            ("guard", S::String),
            ("passwords", S::String),
            ("provider", S::String.nullable()),
        ]),
        |_| Ok(()),
    )?;
    app.config_section(
        "auth.guards",
        BTreeMap::from([(
            "web".to_owned(),
            GuardConfig {
                driver: "session".into(),
                provider: Some("users".into()),
                options: Map::new(),
            },
        )]),
        S::map(S::object([("driver", S::String), ("provider", S::String.nullable())]).open()),
        |_| Ok(()),
    )?;
    // 模型按宿主编译清单注册；不把database provider别名当Eloquent已实现。
    app.config_section(
        "auth.providers",
        BTreeMap::from([(
            "users".to_owned(),
            UserProviderConfig {
                driver: "eloquent".into(),
                options: Map::from_iter([("model".into(), json!("app::models::User"))]),
            },
        )]),
        S::map(S::object([("driver", S::String)]).open()),
        |_| Ok(()),
    )?;
    app.config_section("auth.timebox_duration", 200_000_u64, S::Integer, |_| Ok(()))?;
    app.config_section("auth.password_timeout", 10_800_u64, S::Integer, |_| Ok(()))?;
    Ok(())
}

fn default_drivers(config: crate::config::repository::ConfigRepository) -> Result<AuthDrivers> {
    let drivers = AuthDrivers::default();
    drivers.provider("database", |resolver, config| {
        Box::pin(create_database_provider(resolver, config))
    })?;
    drivers.extend("session", move |context| {
        Box::pin(create_session_guard(context, config.clone()))
    })?;
    Ok(drivers)
}

async fn create_database_provider(
    resolver: crate::core::container::Resolver,
    config: UserProviderConfig,
) -> Result<Arc<dyn UserProvider>> {
    let table = config
        .options
        .get("table")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            Error::custom_with_status(
                StatusCode::INTERNAL_SERVER_ERROR,
                "auth_provider_table_missing",
                "数据库账户Provider必须配置table",
            )
        })?;
    let db = match config.options.get("connection") {
        None | Some(Value::Null) => (*resolver.resolve::<Db>().await?).clone(),
        Some(Value::String(name)) if name.is_empty() => (*resolver.resolve::<Db>().await?).clone(),
        Some(Value::String(name)) => resolver
            .resolve::<crate::db::Databases>()
            .await?
            .get(name)
            .map_err(|error| match error {
                Error::Custom { code, message, .. } if code == "database_instance_not_found" => {
                    Error::custom_with_status(StatusCode::INTERNAL_SERVER_ERROR, code, message)
                }
                other => other,
            })?,
        _ => {
            return Err(Error::custom_with_status(
                StatusCode::INTERNAL_SERVER_ERROR,
                "auth_provider_connection_invalid",
                "数据库账户Provider的connection必须为字符串或null",
            ));
        }
    };
    let passwords: std::sync::Arc<dyn crate::hashing::Hasher> =
        resolver.resolve::<crate::hashing::HashManager>().await?;
    Ok(Arc::new(DatabaseUserProvider::new(db, passwords.clone(), table)?) as Arc<dyn UserProvider>)
}

fn remember_duration(config: &GuardConfig) -> Result<Option<i64>> {
    config
        .options
        .get("remember")
        .map(|value| {
            value.as_i64().ok_or_else(|| {
                Error::custom_with_status(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "auth_remember_duration_invalid",
                    "记住我持续分钟数必须是有符号整数",
                )
            })
        })
        .transpose()
}

#[cfg(feature = "auth-fortify")]
fn configure_factor_guard(
    mut guard: SessionGuard,
    resolver: &crate::core::container::Resolver,
    config: crate::config::repository::ConfigRepository,
) -> Result<SessionGuard> {
    let required = resolver.is_bound::<crate::auth::fortify::RequireFactorAuthority>()?;
    if required || resolver.is_bound::<crate::auth::fortify::Fortify>()? {
        let confirm = crate::auth::fortify::configured_confirmation(&config)?;
        guard = guard
            .with_factor_authority_policy(required, confirm)
            .with_factor_config(config);
    } else {
        guard = guard.with_factor_authority_policy(required, false);
    }
    Ok(guard)
}

async fn create_session_guard(
    context: super::GuardContext,
    config: crate::config::repository::ConfigRepository,
) -> Result<Arc<dyn Guard>> {
    #[cfg(not(feature = "auth-fortify"))]
    drop(config);
    let provider = context.user_provider().await?.ok_or_else(|| {
        Error::custom_with_status(
            StatusCode::INTERNAL_SERVER_ERROR,
            "auth_guard_provider_missing",
            "Session Guard需要账户Provider",
        )
    })?;
    let slot = context.resolver.resolve::<AuthRequestSlot>().await?;
    let request = slot.0.get().ok_or_else(|| {
        Error::custom_with_status(
            StatusCode::INTERNAL_SERVER_ERROR,
            "auth_request_missing",
            "Session Guard需要当前请求的Web/Session上下文",
        )
    })?;
    let session = context.resolver.resolve::<Session>().await?;
    let cookies = context.resolver.resolve::<CookieJar>().await?;
    let events = if context.resolver.is_bound::<crate::events::Events>()? {
        Some((*context.resolver.resolve::<crate::events::Events>().await?).clone())
    } else {
        None
    };
    let app_config = context.resolver.section::<AppConfig>("app")?;
    let provider_name = context.user_provider_name()?;
    let rehash = context
        .resolver
        .section::<bool>("hashing.rehash_on_login")?;
    let timebox = context.resolver.section::<u64>("auth.timebox_duration")?;
    let remember = remember_duration(&context.config)?;

    let mut guard = SessionGuard::new(
        context.name,
        provider,
        (*session).clone(),
        (*cookies).clone(),
        None,
        events,
        app_config.key,
    );
    #[cfg(feature = "auth-fortify")]
    {
        guard = configure_factor_guard(guard, &context.resolver, config)?;
    }
    let name = guard.recaller_name();
    let session_cookie = if context
        .resolver
        .is_bound::<crate::session::SessionSettings>()?
    {
        context
            .resolver
            .resolve::<crate::session::SessionSettings>()
            .await?
            .cookie_name
            .clone()
    } else {
        session.name()?
    };
    let original = crate::http::cookie::parse_headers(request.info.headers())?.original;
    for name in [&session_cookie, &name] {
        // Cookie解密视图按PHP规则折叠重复根名；认证前必须核对原始wire，不能接受歧义凭据。
        if original
            .root_names
            .iter()
            .filter(|root| *root == name)
            .count()
            > 1
        {
            return Err(crate::auth::ambiguous_credentials());
        }
    }
    let recaller = request
        .cookies
        .get(&name)
        .and_then(Value::as_str)
        .map(str::to_owned);
    let protected = request.protection.as_ref().is_some_and(|protection| {
        protection.encrypted && !protection.except.iter().any(|except| except == &name)
    });
    guard = guard
        .with_decrypted_recaller(recaller)
        .with_remember_cookie_protected(protected)
        .with_provider_name(provider_name)
        .with_validation_options(rehash, std::time::Duration::from_micros(timebox));
    if let Some(minutes) = remember {
        guard = guard.with_remember_duration(minutes);
    }
    Ok(Arc::new(guard) as Arc<dyn Guard>)
}
