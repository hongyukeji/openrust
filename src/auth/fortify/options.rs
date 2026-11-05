//! 固定 Fortify 包回退与安装模板的双因素选项。

use serde::{Deserialize, Serialize};

/// `Features::twoFactorAuthentication` 的选项，不替代既有应用配置仓储。
///
/// `Default` 对应包回退；安装器须显式使用 `installed`，不能将两者混为一套默认。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TwoFactorOptions {
    /// 是否要求首次验证码确认后才用于登录。
    pub confirm: bool,
    /// 管理操作是否通过近期密码确认中间件。
    #[serde(rename = "confirmPassword", alias = "confirm_password")]
    pub confirm_password: bool,
    /// 随机密钥的 Base32 字符数，官方默认16。
    #[serde(rename = "secret-length", alias = "secret_length")]
    pub secret_length: usize,
    /// 覆写引擎容差窗口；None保留引擎默认1。
    pub window: Option<i64>,
}

impl Default for TwoFactorOptions {
    fn default() -> Self {
        Self {
            confirm: false,
            confirm_password: false,
            secret_length: 16,
            window: None,
        }
    }
}

impl TwoFactorOptions {
    /// 对应固定官方发布配置的安装默认：确认绑定与近期密码确认均开启。
    pub fn installed() -> Self {
        Self {
            confirm: true,
            confirm_password: true,
            ..Self::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::TwoFactorOptions;

    #[test]
    fn package_and_installed_defaults_remain_distinct() {
        let package = TwoFactorOptions::default();
        assert!(!package.confirm && !package.confirm_password);
        assert_eq!(package.secret_length, 16);
        assert_eq!(package.window, None);
        let installed = TwoFactorOptions::installed();
        assert!(installed.confirm && installed.confirm_password);
    }

    #[test]
    fn empty_options_do_not_silently_apply_installer_defaults() -> crate::Result<()> {
        let options: TwoFactorOptions =
            serde_json::from_str("{}").map_err(crate::Error::internal)?;
        assert_eq!(options, TwoFactorOptions::default());
        let options: TwoFactorOptions = serde_json::from_value(serde_json::json!({
            "confirm": true, "confirmPassword": false, "secret-length": 32, "window": 0
        }))
        .map_err(crate::Error::internal)?;
        assert!(options.confirm);
        assert!(!options.confirm_password);
        assert_eq!(options.secret_length, 32);
        assert_eq!(options.window, Some(0));
        Ok(())
    }
}

/// 此固定功能名是非数值、非空字符串；PHP宽松比较仅字符串同名或布尔true可匹配。
pub(super) fn features_enabled(features: &serde_json::Value) -> crate::Result<bool> {
    fn selected(value: &serde_json::Value) -> bool {
        value.as_str() == Some("two-factor-authentication")
            || value == &serde_json::Value::Bool(true)
    }
    match features {
        serde_json::Value::Array(values) => Ok(values.iter().any(selected)),
        serde_json::Value::Object(values) => Ok(values.values().any(selected)),
        _ => Err(crate::Error::custom_with_status(
            crate::http::axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "fortify_features_invalid",
            "Fortify功能清单必须为数组",
        )),
    }
}

/// 调用时读取官方功能清单；缺键按 `Features::enabled` 的空清单回退。
pub(super) fn feature_enabled(
    config: &crate::config::repository::ConfigRepository,
) -> crate::Result<bool> {
    features_enabled(
        &config
            .get("fortify.features")?
            .unwrap_or_else(|| serde_json::json!([])),
    )
}

/// optionEnabled只接受布尔true，且功能自身必须启用；两键来自同一仓储快照。
fn option_enabled(
    config: &crate::config::repository::ConfigRepository,
    key: &str,
) -> crate::Result<bool> {
    let empty = serde_json::json!([]);
    let values = config.get_many_with_defaults(&[
        ("fortify.features", empty.clone()),
        (key, serde_json::Value::Null),
    ])?;
    let features = values
        .iter()
        .find(|(name, _)| name == "fortify.features")
        .map_or(&empty, |(_, value)| value);
    Ok(features_enabled(features)?
        && values
            .iter()
            .any(|(name, value)| name == key && value == &serde_json::Value::Bool(true)))
}

pub(super) fn confirmation_required(
    config: &crate::config::repository::ConfigRepository,
) -> crate::Result<bool> {
    option_enabled(config, "fortify-options.two-factor-authentication.confirm")
}

pub(super) fn password_confirmation(
    config: &crate::config::repository::ConfigRepository,
) -> crate::Result<bool> {
    option_enabled(
        config,
        "fortify-options.two-factor-authentication.confirmPassword",
    )
}

fn parsed_secret_length(value: &serde_json::Value) -> crate::Result<usize> {
    usize::try_from(crate::support::scalars::integer(value)).map_err(|_| {
        crate::Error::custom(
            "fortify_totp_secret_invalid",
            "密钥长度不能表示为Rust非负长度",
        )
    })
}

pub(super) fn secret_length(
    config: &crate::config::repository::ConfigRepository,
) -> crate::Result<usize> {
    config
        .get("fortify-options.two-factor-authentication.secret-length")?
        .as_ref()
        .map_or(Ok(16), parsed_secret_length)
}

pub(super) fn snapshot(
    config: &crate::config::repository::ConfigRepository,
) -> crate::Result<TwoFactorOptions> {
    let prefix = "fortify-options.two-factor-authentication";
    let feature_key = "fortify.features";
    let confirm_key = format!("{prefix}.confirm");
    let password_key = format!("{prefix}.confirmPassword");
    let length_key = format!("{prefix}.secret-length");
    let window_key = format!("{prefix}.window");
    let empty = serde_json::json!([]);
    let missing = serde_json::Value::Null;
    let default_length = serde_json::json!(16);
    let values = config.get_many_with_defaults(&[
        (feature_key, empty.clone()),
        (&confirm_key, missing.clone()),
        (&password_key, missing.clone()),
        (&length_key, default_length.clone()),
        (&window_key, missing.clone()),
    ])?;
    let value = |key: &str| {
        values
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value)
    };
    let enabled = features_enabled(value(feature_key).unwrap_or(&empty))?;
    Ok(TwoFactorOptions {
        confirm: enabled && value(&confirm_key) == Some(&serde_json::Value::Bool(true)),
        confirm_password: enabled && value(&password_key) == Some(&serde_json::Value::Bool(true)),
        secret_length: parsed_secret_length(value(&length_key).unwrap_or(&default_length))?,
        window: value(&window_key).and_then(serde_json::Value::as_i64),
    })
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    use crate::{Result, config::repository::ConfigRepository};
    use serde_json::json;

    #[test]
    fn package_features_and_installed_features_follow_the_fixed_files() {
        let package = crate::auth::fortify::FortifyConfig::default();
        let installed = crate::auth::fortify::FortifyConfig::installed();
        assert_eq!(package.features.as_array().map(Vec::len), Some(7));
        assert_eq!(installed.features.as_array().map(Vec::len), Some(6));
        assert!(
            package
                .features
                .as_array()
                .is_some_and(|items| items.iter().any(|name| name == "email-verification"))
        );
        assert!(
            !installed
                .features
                .as_array()
                .is_some_and(|items| items.iter().any(|name| name == "email-verification"))
        );
        for config in [&package, &installed] {
            assert!(
                config.features.as_array().is_some_and(|items| items
                    .iter()
                    .any(|name| name == "two-factor-authentication"))
            );
            assert!(
                config
                    .features
                    .as_array()
                    .is_some_and(|items| items.iter().any(|name| name == "passkeys"))
            );
        }
    }

    #[test]
    fn option_enabled_uses_canonical_feature_and_strict_true_at_each_call() -> Result<()> {
        let config = ConfigRepository::new(figment::Figment::new());
        config.set(
            "fortify-options.two-factor-authentication.confirm",
            json!(true),
        )?;
        assert!(!confirmation_required(&config)?);
        config.set("fortify.features", json!(["two-factor-authentication"]))?;
        assert!(confirmation_required(&config)?);
        for value in [
            json!(false),
            json!(null),
            json!(1),
            json!("true"),
            json!([]),
        ] {
            config.set("fortify-options.two-factor-authentication.confirm", value)?;
            assert!(!confirmation_required(&config)?);
        }
        config.set(
            "fortify-options.two-factor-authentication.confirmPassword",
            json!(true),
        )?;
        assert!(password_confirmation(&config)?);
        config.set("fortify.features", json!([]))?;
        assert!(!password_confirmation(&config)?);
        Ok(())
    }

    #[test]
    fn secret_length_uses_existing_php_integer_cast_without_missing_null_aliasing() -> Result<()> {
        let config = ConfigRepository::new(figment::Figment::new());
        assert_eq!(secret_length(&config)?, 16);
        for (value, expected) in [
            (json!(null), 0),
            (json!("32tail"), 32),
            (json!(32.9), 32),
            (json!(true), 1),
            (json!([]), 0),
            (json!([9]), 1),
        ] {
            config.set(
                "fortify-options.two-factor-authentication.secret-length",
                value,
            )?;
            assert_eq!(secret_length(&config)?, expected);
        }
        config.set(
            "fortify-options.two-factor-authentication.secret-length",
            json!(-16),
        )?;
        assert!(secret_length(&config).is_err());
        Ok(())
    }
}

#[cfg(test)]
mod feature_value_tests {
    use super::features_enabled;
    use serde_json::json;

    #[test]
    fn fixed_nonnumeric_feature_follows_php_array_values_and_loose_boolean_comparison()
    -> crate::Result<()> {
        for value in [
            json!(["two-factor-authentication"]),
            json!([true]),
            json!({"named":"two-factor-authentication"}),
            json!({"enabled":true}),
        ] {
            assert!(features_enabled(&value)?);
        }
        for value in [
            json!([]),
            json!([false, null, 0, 1, "0", "true"]),
            json!({"two-factor-authentication":false}),
        ] {
            assert!(!features_enabled(&value)?);
        }
        for value in [
            json!(null),
            json!(true),
            json!(1),
            json!("two-factor-authentication"),
        ] {
            let error = features_enabled(&value)
                .err()
                .ok_or(crate::Error::Unauthorized)?;
            assert_eq!(
                error.status(),
                crate::http::axum::http::StatusCode::INTERNAL_SERVER_ERROR
            );
        }
        Ok(())
    }
}
