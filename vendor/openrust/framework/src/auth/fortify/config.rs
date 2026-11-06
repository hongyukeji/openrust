//! Fortify HTTP层的可编辑配置；默认值对应固定包配置。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{Error, Result};

/// 固定包的具名限流配置；安装配置与包回退分别构造。
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct FortifyLimiters {
    /// 登录路由具名限流器；None使用包内失败计数管线。
    pub login: Option<String>,
    /// 挑战路由具名限流器；包回退未配置。
    #[serde(rename = "two-factor")]
    pub two_factor: Option<String>,
}

/// 官方登录/双因素HTTP接线配置，不将API前缀或JSON envelope强加为默认。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct FortifyConfig {
    /// 同一 `AuthManager` 内的 Guard 名称。
    pub guard: String,
    /// 官方 `auth_middleware` 配置；仍从同一参数化 `MiddlewareRegistry` 解析。
    pub auth_middleware: String,
    /// 默认登录查询与二维码账户标签字段。
    pub username: String,
    /// 正常登录的默认意图地址回退。
    pub home: String,
    /// 路由组前缀，默认空。
    pub prefix: String,
    /// 可选路由域名。
    pub domain: Option<String>,
    /// 通过同一MiddlewareRegistry装配的组/别名。
    pub middleware: Vec<String>,
    /// 是否登记登录、密码确认及双因素挑战视图GET路由。
    pub views: bool,
    /// 密码管线是否先规范化username；包回退false，发布模板true。
    pub lowercase_usernames: bool,
    /// 具名限流器选择。
    pub limiters: FortifyLimiters,
    /// 官方功能名称清单；双因素开关来自此处，不放入其选项树。
    pub features: Value,
    /// 官方路径覆写树，键与固定routes/routes.php一致。
    pub paths: BTreeMap<String, Value>,
    /// 官方响应重定向覆写。
    pub redirects: BTreeMap<String, Option<String>>,
}

impl Default for FortifyConfig {
    fn default() -> Self {
        Self {
            guard: "web".into(),
            auth_middleware: "auth".into(),
            username: "email".into(),
            home: "/home".into(),
            prefix: String::new(),
            domain: None,
            middleware: vec!["web".into()],
            views: true,
            lowercase_usernames: false,
            limiters: FortifyLimiters::default(),
            features: serde_json::json!([
                "registration",
                "reset-passwords",
                "email-verification",
                "update-profile-information",
                "update-passwords",
                "two-factor-authentication",
                "passkeys"
            ]),
            paths: BTreeMap::new(),
            redirects: BTreeMap::new(),
        }
    }
}
impl FortifyConfig {
    /// 对应官方发布模板，应用可继续覆写同一ConfigRepository。
    pub fn installed() -> Self {
        Self {
            features: serde_json::json!([
                "registration",
                "reset-passwords",
                "update-profile-information",
                "update-passwords",
                "two-factor-authentication",
                "passkeys"
            ]),
            lowercase_usernames: true,
            limiters: FortifyLimiters {
                login: Some("login".into()),
                two_factor: Some("two-factor".into()),
            },
            ..Self::default()
        }
    }

    /// 对应官方宽松 `in_array` 功能判断；关联数组按值查找，非数组明确报错。
    pub fn two_factor_enabled(&self) -> Result<bool> {
        super::options::features_enabled(&self.features)
    }

    /// 按官方点路径查询覆写；null/缺失使用默认，不接受非字符串部署值。
    pub fn route_path(&self, key: &str, default: &str) -> Result<String> {
        let mut parts = key.split('.');
        let mut value = parts.next().and_then(|key| self.paths.get(key));
        for key in parts {
            value = value.and_then(|value| value.get(key));
        }
        match value {
            None | Some(Value::Null) => Ok(default.into()),
            Some(Value::String(path)) => Ok(path.clone()),
            _ => Err(Error::custom_with_status(
                crate::http::axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "fortify_path_invalid",
                "Fortify路由路径必须为字符串或null",
            )),
        }
    }

    /// 返回响应重定向设置，不在此提前消费Session意图地址。
    pub fn redirect_for<'a>(&'a self, key: &str, fallback: &'a str) -> &'a str {
        self.redirects
            .get(key)
            .and_then(|value| value.as_deref())
            .unwrap_or(fallback)
    }
}
