use crate::{Error, Result};
use http::header::HeaderName;
use openrust::config::Environment;
use serde_json::Value;
use std::{
    collections::HashMap,
    fmt,
    sync::{Arc, RwLock},
};
use zeroize::Zeroizing;

/// 单次解析的不可变配置；不实现 Serialize，密钥不进入任务与诊断快照。
#[derive(Clone)]
pub struct WebhookConfig {
    name: String,
    revision: String,
    secret: Zeroizing<String>,
    verification_secrets: Vec<Zeroizing<String>>,
    signature_header: HeaderName,
    max_body_bytes: usize,
    options: Value,
}
impl fmt::Debug for WebhookConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WebhookConfig")
            .field("name", &self.name)
            .field("revision", &self.revision)
            .field("credentials", &"[REDACTED]")
            .finish()
    }
}
impl WebhookConfig {
    pub fn new(name: impl Into<String>, secret: impl Into<String>) -> Result<Self> {
        let name = name.into();
        let secret = Zeroizing::new(secret.into());
        if name.is_empty()
            || name.len() > 128
            || name.chars().any(char::is_control)
            || secret.len() > 8192
        {
            return Err(Error::InvalidConfig("name/signing_secret"));
        }
        Ok(Self {
            name,
            revision: "1".into(),
            secret,
            verification_secrets: vec![],
            signature_header: HeaderName::from_static("signature"),
            max_body_bytes: 1024 * 1024,
            options: Value::Null,
        })
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn revision(&self) -> &str {
        &self.revision
    }
    pub fn signing_secret(&self) -> &str {
        &self.secret
    }
    pub fn signature_header_name(&self) -> &HeaderName {
        &self.signature_header
    }
    pub fn max_body_bytes(&self) -> usize {
        self.max_body_bytes
    }
    /// 仅含安全配置元数据，环境变量引用不会被替换为密钥内容。
    pub fn options(&self) -> &Value {
        &self.options
    }
    /// 空列表不保存请求头；单独的 `*` 显式保存所有头，应用须考虑凭据与隐私。
    pub fn with_store_headers(mut self, names: &[&str]) -> Result<Self> {
        if names.len() > 128 || (names.contains(&"*") && names.len() != 1) {
            return Err(Error::InvalidConfig("store_headers"));
        }
        for name in names {
            if *name != "*" {
                HeaderName::from_bytes(name.as_bytes())
                    .map_err(|_| Error::InvalidConfig("store_headers"))?;
            }
        }
        if self.options.is_null() {
            self.options = serde_json::json!({});
        }
        self.options["store_headers"] = if names == ["*"] {
            Value::String("*".into())
        } else {
            serde_json::json!(names)
        };
        Ok(self)
    }
    pub fn headers_to_store(&self, headers: &http::HeaderMap) -> Result<http::HeaderMap> {
        let names = &self.options["store_headers"];
        if names.as_str() == Some("*") {
            return Ok(headers.clone());
        }
        let mut result = http::HeaderMap::new();
        if names.is_null() {
            return Ok(result);
        }
        let names = names
            .as_array()
            .ok_or(Error::InvalidConfig("store_headers"))?;
        for (name, value) in headers {
            if names.iter().any(|n| {
                n.as_str()
                    .is_some_and(|n| n.eq_ignore_ascii_case(name.as_str()))
            }) {
                result.append(name.clone(), value.clone());
            }
        }
        Ok(result)
    }
    fn with_implementation(mut self, field: &'static str, name: impl Into<String>) -> Result<Self> {
        let name = name.into();
        crate::factories::validate_reference(field, &name)?;
        if self.options.is_null() {
            self.options = serde_json::json!({});
        }
        self.options[field] = Value::String(name);
        Ok(self)
    }
    pub fn with_signature_validator(self, name: impl Into<String>) -> Result<Self> {
        self.with_implementation("signature_validator", name)
    }
    pub fn with_webhook_profile(self, name: impl Into<String>) -> Result<Self> {
        self.with_implementation("webhook_profile", name)
    }
    pub fn with_webhook_response(self, name: impl Into<String>) -> Result<Self> {
        self.with_implementation("webhook_response", name)
    }
    pub fn with_webhook_model(self, name: impl Into<String>) -> Result<Self> {
        self.with_implementation("webhook_model", name)
    }
    pub fn with_process_webhook_job(self, name: impl Into<String>) -> Result<Self> {
        self.with_implementation("process_webhook_job", name)
    }
    pub fn with_scope(mut self, scope: impl Into<String>) -> Result<Self> {
        let scope = scope.into();
        if scope.is_empty() || scope.len() > 128 || scope.chars().any(char::is_control) {
            return Err(Error::InvalidConfig("scope"));
        }
        if self.options.is_null() {
            self.options = serde_json::json!({});
        }
        self.options["scope"] = Value::String(scope);
        Ok(self)
    }
    pub fn with_connection(mut self, name: Option<&str>) -> Result<Self> {
        if name.is_some_and(|name| {
            name.is_empty() || name.len() > 128 || name.chars().any(char::is_control)
        }) {
            return Err(Error::InvalidConfig("connection"));
        }
        if self.options.is_null() {
            self.options = serde_json::json!({});
        }
        self.options["connection"] = name.map_or(Value::Null, |name| Value::String(name.into()));
        Ok(self)
    }
    pub fn with_signature_header(mut self, name: &str) -> Result<Self> {
        self.signature_header = HeaderName::from_bytes(name.as_bytes())
            .map_err(|_| Error::InvalidConfig("signature_header_name"))?;
        Ok(self)
    }
    pub fn with_revision(mut self, revision: impl Into<String>) -> Result<Self> {
        let revision = revision.into();
        if revision.is_empty()
            || revision.len() > 128
            || !revision
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        {
            return Err(Error::InvalidConfig("revision"));
        }
        self.revision = revision;
        Ok(self)
    }
    pub fn with_max_body_bytes(mut self, limit: usize) -> Result<Self> {
        if limit == 0 || limit > 16 * 1024 * 1024 {
            return Err(Error::InvalidConfig("max_body_bytes"));
        }
        self.max_body_bytes = limit;
        Ok(self)
    }
    pub fn with_verification_secrets(
        mut self,
        secrets: impl IntoIterator<Item = String>,
    ) -> Result<Self> {
        let mut result = Vec::new();
        for secret in secrets {
            let secret = Zeroizing::new(secret);
            if result.len() >= 7 || secret.is_empty() || secret.len() > 8192 {
                return Err(Error::InvalidConfig("verification_secrets"));
            }
            result.push(secret);
        }
        self.verification_secrets = result;
        Ok(self)
    }
    pub(crate) fn secrets(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.secret.as_str())
            .chain(self.verification_secrets.iter().map(|s| s.as_str()))
    }
    pub fn from_config(value: &Value, env: &Environment) -> Result<Self> {
        fn credential(value: &Value, env: &Environment) -> Result<String> {
            if value.is_null() {
                return Ok(String::new());
            }
            if value.as_object().is_none_or(|object| object.len() != 1) {
                return Err(Error::InvalidConfig("signing_secret.env"));
            }
            let name = value
                .get("env")
                .and_then(Value::as_str)
                .filter(|n| {
                    !n.is_empty()
                        && n.len() <= 128
                        && n.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
                })
                .ok_or(Error::InvalidConfig("signing_secret.env"))?;
            env.get_utf8(name)
                .map_err(|_| Error::InvalidConfig("signing_secret.env"))?
                .map(str::to_owned)
                .ok_or(Error::InvalidConfig("signing_secret.env 未配置"))
        }
        let secret = credential(&value["signing_secret"], env)?;
        let mut config = Self::new(
            value["name"].as_str().ok_or(Error::InvalidConfig("name"))?,
            secret,
        )?;
        if let Some(header) = value.get("signature_header_name") {
            config = config.with_signature_header(
                header
                    .as_str()
                    .ok_or(Error::InvalidConfig("signature_header_name"))?,
            )?;
        }
        if let Some(revision) = value.get("revision") {
            config =
                config.with_revision(revision.as_str().ok_or(Error::InvalidConfig("revision"))?)?;
        }
        if let Some(limit) = value.get("max_body_bytes") {
            config = config.with_max_body_bytes(
                limit
                    .as_u64()
                    .and_then(|n| usize::try_from(n).ok())
                    .ok_or(Error::InvalidConfig("max_body_bytes"))?,
            )?;
        }
        if let Some(secrets) = value.get("verification_secrets") {
            let secrets = secrets
                .as_array()
                .ok_or(Error::InvalidConfig("verification_secrets"))?;
            if secrets.len() > 7 {
                return Err(Error::InvalidConfig("verification_secrets"));
            }
            config = config.with_verification_secrets(
                secrets
                    .iter()
                    .map(|s| credential(s, env))
                    .collect::<Result<Vec<_>>>()?,
            )?;
        }
        config.options = value.clone();
        if let Some(names) = value.get("store_headers").filter(|names| !names.is_null()) {
            if names.as_str() == Some("*") {
                config = config.with_store_headers(&["*"])?;
            } else {
                let names = names
                    .as_array()
                    .ok_or(Error::InvalidConfig("store_headers"))?;
                let names = names
                    .iter()
                    .map(|name| name.as_str().ok_or(Error::InvalidConfig("store_headers")))
                    .collect::<Result<Vec<_>>>()?;
                if names.contains(&"*") {
                    return Err(Error::InvalidConfig("store_headers：全选使用字符串 *"));
                }
                config = config.with_store_headers(&names)?;
            }
        }
        Ok(config)
    }
}

/// 当前应用的具名配置；替换不改变已取得的 Arc 快照。
#[derive(Default)]
pub struct WebhookConfigRepository {
    configs: RwLock<HashMap<String, Arc<WebhookConfig>>>,
    source: Option<(Vec<Value>, Arc<Environment>)>,
}
impl WebhookConfigRepository {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn from_configuration(value: &Value, env: Arc<Environment>) -> Result<Self> {
        let configs = value["configs"]
            .as_array()
            .filter(|configs| configs.len() <= 128)
            .ok_or(Error::InvalidConfig("configs"))?;
        for config in configs {
            if config["name"]
                .as_str()
                .is_none_or(|name| name.is_empty() || name.len() > 128)
            {
                return Err(Error::InvalidConfig("name"));
            }
        }
        Ok(Self {
            configs: RwLock::default(),
            source: Some((configs.clone(), env)),
        })
    }
    /// 按固定 Provider 的当前路由名选择配置；唯一令牌模式取第一个点之前的名称。
    pub fn get_config_for_route(
        &self,
        route_name: Option<&str>,
        unique_token: bool,
    ) -> Result<Arc<WebhookConfig>> {
        let route_name = route_name.unwrap_or_default();
        let suffix = route_name
            .split_once("webhook-client-")
            .map_or(route_name, |(_, suffix)| suffix);
        let name = if unique_token {
            suffix.split_once('.').map_or(suffix, |(name, _)| name)
        } else {
            suffix
        };
        self.get_config(name)?.ok_or_else(|| Error::ConfigNotFound {
            name: name.to_owned(),
        })
    }
    pub fn add_config(&self, config: WebhookConfig) {
        self.configs
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(config.name.clone(), Arc::new(config));
    }
    pub fn get_config(&self, name: &str) -> Result<Option<Arc<WebhookConfig>>> {
        if let Some(config) = self
            .configs
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(name)
        {
            return Ok(Some(config.clone()));
        }
        let Some((configs, env)) = &self.source else {
            return Ok(None);
        };
        // 对照上游按名称后值覆盖；按需读取凭据，不让未使用的账号阻断其他配置。
        let Some(value) = configs
            .iter()
            .rev()
            .find(|config| config["name"].as_str() == Some(name))
        else {
            return Ok(None);
        };
        let config = Arc::new(WebhookConfig::from_config(value, env)?);
        let mut resolved = self
            .configs
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Ok(Some(resolved.entry(name.into()).or_insert(config).clone()))
    }
}
