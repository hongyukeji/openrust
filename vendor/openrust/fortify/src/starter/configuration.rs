use openrust::{
    config::ConfigError, contracts::config::Repository, foundation::Application,
    support::ProviderResult,
};
use serde_json::{Value, json};

fn invalid(key: &str, expected: &'static str) -> ConfigError {
    ConfigError::InvalidType {
        key: key.into(),
        expected,
    }
}
pub(super) fn boolean(
    config: &dyn Repository,
    key: &str,
    default: bool,
) -> Result<bool, ConfigError> {
    match config.get(key) {
        None => Ok(default),
        Some(Value::Bool(value)) => Ok(value),
        Some(_) => Err(invalid(key, "布尔值")),
    }
}

/// 在所有 Provider 启动前应用套件明确管理的功能开关，保留 Guard、路由前缀和其他功能。
pub async fn configure(app: &Application) -> ProviderResult {
    let config = app.make::<dyn Repository>().await?;
    let profile = config.get("user.profile").unwrap_or(json!("full"));
    let full = match profile.as_str() {
        Some("full") => true,
        Some("api") => false,
        _ => return Err(invalid("user.profile", "full 或 api").into()),
    };
    if let Some(prefix) = config.get("api.prefix") {
        let prefix = prefix
            .as_str()
            .ok_or_else(|| invalid("api.prefix", "字符串"))?;
        if (full || boolean(config.as_ref(), "user.passkeys", false)?)
            && prefix.trim_matches('/').is_empty()
        {
            return Err(invalid(
                "api.prefix",
                "网页或通行密钥用户套件需要非空API前缀，避免与原生账户路由冲突",
            )
            .into());
        }
    }
    let choices = [
        (
            "registration",
            boolean(config.as_ref(), "user.registration", true)?,
        ),
        (
            "email-verification",
            boolean(config.as_ref(), "user.email_verification", true)?,
        ),
        (
            "two-factor-authentication",
            boolean(config.as_ref(), "user.two_factor", false)?,
        ),
        (
            "passkeys",
            boolean(config.as_ref(), "user.passkeys", false)?,
        ),
    ];
    #[cfg(feature = "sanctum")]
    {
        let providers = super::socialite::configured(config.as_ref())?;
        if !providers.is_empty() && !cfg!(feature = "socialite") {
            return Err(invalid(
                "user.socialite.providers",
                "先启用用户套件的socialite编译特性",
            )
            .into());
        }
    }
    let days = config
        .get("user.token_expiration_days")
        .unwrap_or(json!(30));
    if !days.as_u64().is_some_and(|days| {
        days > 0
            && days
                .checked_mul(86400)
                .is_some_and(|seconds| seconds <= i64::MAX as u64)
    }) {
        return Err(invalid("user.token_expiration_days", "可转换为秒的正整数天数").into());
    }
    let mut features = match config.get("fortify.features") {
        None => vec![],
        Some(Value::Array(features)) if features.iter().all(Value::is_string) => features,
        Some(_) => return Err(invalid("fortify.features", "功能名称数组").into()),
    };
    let mut middleware = match config.get("fortify.middleware") {
        None => vec![json!("web")],
        Some(Value::Array(values)) if values.iter().all(Value::is_string) => values,
        Some(_) => return Err(invalid("fortify.middleware", "中间件名称数组").into()),
    };
    // 先完成全部读取／类型校验，再更新配置，避免无效配置留下部分功能开关。
    for (feature, enabled) in choices {
        features.retain(|value| value.as_str() != Some(feature));
        if enabled {
            features.push(json!(feature));
        }
    }
    for feature in [
        "reset-passwords",
        "update-profile-information",
        "update-passwords",
    ] {
        if !features.iter().any(|value| value.as_str() == Some(feature)) {
            features.push(json!(feature));
        }
    }
    for name in [
        "web",
        "sensitive",
        "auth.session",
        "cache.headers:no_store;private",
    ] {
        if !middleware.iter().any(|value| value.as_str() == Some(name)) {
            middleware.push(json!(name));
        }
    }
    app.make::<dyn openrust::contracts::debug::ExceptionHandler>()
        .await?
        .configure(&mut |exceptions| {
            exceptions.render_async::<openrust::http::HttpError>(|error, request| {
                Box::pin(async move {
                    if matches!(error, openrust::http::HttpError::Unauthenticated)
                        && !request.expects_json()
                        && !request.is_api_path()
                    {
                        let login = openrust::support::facades::Url::route("login", &[], false)?;
                        return Ok(Some(
                            openrust::support::facades::Redirect::guest(
                                request,
                                &login,
                                Default::default(),
                            )?
                            .into(),
                        ));
                    }
                    Ok(None)
                })
            });
            #[cfg(feature = "sanctum")]
            {
                exceptions.dont_report::<super::passkey_confirmation::ConfirmationRequired>();
                exceptions.render_async::<super::passkey_confirmation::ConfirmationRequired>(
                    |_, request| {
                        Box::pin(async move {
                            super::web::forget(request)?;
                            if !request.expects_json()
                                && !request.is_api_path()
                                && let Some(response) =
                                    openrust::auth::middleware::RequirePassword::default()
                                        .challenge(request)?
                            {
                                return Ok(Some(super::client::no_store(response)));
                            }
                            Ok(Some(super::client::no_store(
                                openrust::http::ApiResponse::failure(
                                    openrust::http::StatusCode::LOCKED,
                                    "password_confirmation_required",
                                    "",
                                )
                                .with_message_key("user.confirmation_required", Default::default())
                                .into_response()?,
                            )))
                        })
                    },
                );
            }
            exceptions.dont_flash([
                "password",
                "password_confirmation",
                "current_password",
                "token",
                "code",
                "recovery_code",
                "two_factor_secret",
                "two_factor_recovery_codes",
                "credential",
                "challenge",
                "proof",
                "reauthentication_token",
                "access_token",
            ]);
        })?;
    config.set("auth.session_fingerprint_on_login", json!(true))?;
    #[cfg(not(feature = "sanctum"))]
    {
        let guard = config
            .get("fortify.guard")
            .and_then(|value| {
                value
                    .as_str()
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned)
            })
            .or_else(|| {
                config
                    .get("auth.defaults.guard")
                    .and_then(|value| value.as_str().map(str::to_owned))
            })
            .unwrap_or_else(|| "web".into());
        let guards = config
            .get("auth.guards")
            .ok_or_else(|| invalid("auth.guards", "会话Guard配置"))?;
        let provider = guards
            .get(&guard)
            .and_then(|value| value.get("provider"))
            .cloned()
            .ok_or_else(|| invalid("auth.guards", "用户Provider"))?;
        let selected: Vec<_> = guards
            .as_object()
            .ok_or_else(|| invalid("auth.guards", "对象"))?
            .iter()
            .filter_map(|(name, value)| {
                (value.get("driver") == Some(&json!("session"))
                    && value.get("provider") == Some(&provider))
                .then_some(name.clone())
            })
            .collect();
        config.set(
            "auth.session_credential_version_scope",
            json!({"guards":selected,"tables":[]}),
        )?;
    }
    #[cfg(feature = "sanctum")]
    config.set(
        "auth.session_credential_version_attribute",
        json!(super::credentials::VERSION_ATTRIBUTE),
    )?;
    #[cfg(feature = "sanctum")]
    if config.get("fortify.verification_auth_middleware").is_none()
        && config
            .get("fortify.auth_middleware")
            .is_none_or(|value| value == json!("auth"))
    {
        let guard = config
            .get("fortify.guard")
            .and_then(|value| value.as_str().map(str::to_owned))
            .or_else(|| {
                config
                    .get("auth.defaults.guard")
                    .and_then(|value| value.as_str().map(str::to_owned))
            })
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "web".into());
        config.set(
            "fortify.verification_auth_middleware",
            json!(format!("auth:{guard},sanctum")),
        )?;
    }
    config.set("fortify.features", Value::Array(features))?;
    config.set("fortify.middleware", Value::Array(middleware))?;
    config.set("fortify.views", json!(full))?;

    if !full {
        let prefix = config
            .get("fortify.prefix")
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_default();
        let landing = if config.get("user.email_verification") == Some(json!(false)) {
            "/".to_owned()
        } else {
            format!("/{}/email/verify", prefix.trim_matches('/')).replace("//", "/")
        };
        if config
            .get("fortify.home")
            .is_none_or(|value| value.as_str() == Some("/home"))
        {
            config.set("fortify.home", json!(landing))?;
        }
        if config
            .get("fortify.redirects.password-reset")
            .is_none_or(|value| value.is_null())
        {
            config.set(
                "fortify.redirects.password-reset",
                json!(format!("/{}/login", prefix.trim_matches('/')).replace("//", "/")),
            )?;
        }
    }
    Ok(())
}
