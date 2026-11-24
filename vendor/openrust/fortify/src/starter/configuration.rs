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
fn boolean(config: &dyn Repository, key: &str, default: bool) -> Result<bool, ConfigError> {
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
    for name in ["web", "auth.session"] {
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
                "reauthentication_token",
                "access_token",
            ]);
        })?;
    config.set("auth.session_fingerprint_on_login", json!(true))?;
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
