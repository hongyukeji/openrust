use crate::{Error, Result};
use http::header::HeaderName;
use openrust::config::Environment;
use serde::Deserialize as _;
use serde_json::Value;
use std::{
    collections::HashMap,
    fmt,
    sync::{Arc, RwLock},
};
use zeroize::Zeroizing;

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProcessingRecovery {
    pub after_seconds: u32,
    pub max_requeues: u32,
}

impl ProcessingRecovery {
    fn validate(self) -> Result<Self> {
        if !(60..=86_400).contains(&self.after_seconds) || !(1..=10).contains(&self.max_requeues) {
            return Err(Error::InvalidConfig("recover_processing"));
        }
        Ok(self)
    }
}

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

    /// 是否在接收流程结束后派发脱敏观测事件。
    pub fn observe_receipts(&self) -> Result<bool> {
        match self.options.get("observe_receipts") {
            None | Some(Value::Null) => Ok(false),
            Some(Value::Bool(value)) => Ok(*value),
            _ => Err(Error::InvalidConfig("observe_receipts")),
        }
    }

    /// 为手动创建的接收配置启用观测事件。
    pub fn with_receipt_observation(mut self) -> Self {
        if self.options.is_null() {
            self.options = serde_json::json!({});
        }
        self.options["observe_receipts"] = Value::Bool(true);
        self
    }

    pub fn verified_requests_per_minute(&self) -> Result<Option<u32>> {
        match self.options.get("verified_requests_per_minute") {
            None | Some(Value::Null) => Ok(None),
            Some(value) => value
                .as_u64()
                .and_then(|value| u32::try_from(value).ok())
                .filter(|value| *value > 0)
                .map(Some)
                .ok_or(Error::InvalidConfig("verified_requests_per_minute")),
        }
    }

    /// 对当前范围及 profile 内已经验签且通过筛选的请求设置每分钟额度。
    pub fn with_verified_requests_per_minute(mut self, limit: u32) -> Result<Self> {
        if limit == 0 {
            return Err(Error::InvalidConfig("verified_requests_per_minute"));
        }
        if self.options.is_null() {
            self.options = serde_json::json!({});
        }
        self.options["verified_requests_per_minute"] = Value::from(limit);
        Ok(self)
    }

    pub(crate) fn verified_rate_scope(&self) -> Result<&str> {
        self.options["scope"]
            .as_str()
            .filter(|scope| valid_scope(scope))
            .ok_or(Error::InvalidConfig("verified_requests_per_minute.scope"))
    }
    /// 是否为持久附件读取保存 multipart 元数据。
    pub fn store_attachments(&self) -> Result<bool> {
        match self.options.get("store_attachments") {
            None | Some(Value::Null) => Ok(true),
            Some(Value::Bool(value)) => Ok(*value),
            _ => Err(Error::InvalidConfig("store_attachments")),
        }
    }

    pub fn atomic_database_queue(&self) -> Result<bool> {
        match self.options.get("atomic_database_queue") {
            None | Some(Value::Null) => Ok(false),
            Some(Value::Bool(enabled)) => Ok(*enabled),
            _ => Err(Error::InvalidConfig("atomic_database_queue")),
        }
    }

    pub fn with_atomic_database_queue(mut self, enabled: bool) -> Self {
        if self.options.is_null() {
            self.options = serde_json::json!({});
        }
        self.options["atomic_database_queue"] = Value::Bool(enabled);
        self
    }

    pub fn recover_dispatch(&self) -> Result<bool> {
        match self.options.get("recover_dispatch") {
            None | Some(Value::Null) => Ok(false),
            Some(Value::Bool(enabled)) if !enabled || !self.atomic_database_queue()? => {
                Ok(*enabled)
            }
            _ => Err(Error::InvalidConfig("recover_dispatch")),
        }
    }

    pub fn with_dispatch_recovery(mut self, enabled: bool) -> Self {
        if self.options.is_null() {
            self.options = serde_json::json!({});
        }
        self.options["recover_dispatch"] = Value::Bool(enabled);
        self
    }

    /// 为已入列但逾期未完成的接收记录启用有界补派。
    pub fn with_processing_recovery(
        mut self,
        after_seconds: u32,
        max_requeues: u32,
    ) -> Result<Self> {
        let policy = ProcessingRecovery {
            after_seconds,
            max_requeues,
        }
        .validate()?;
        if self.options.is_null() {
            self.options = serde_json::json!({});
        }
        self.options["recover_processing"] = serde_json::json!({
            "after_seconds": policy.after_seconds,
            "max_requeues": policy.max_requeues,
        });
        Ok(self)
    }

    pub(crate) fn processing_recovery(&self) -> Result<Option<ProcessingRecovery>> {
        let value = &self.options["recover_processing"];
        if value.is_null() {
            return Ok(None);
        }
        if !self.recover_dispatch()? {
            return Err(Error::InvalidConfig("recover_processing"));
        }
        ProcessingRecovery::deserialize(value)
            .map_err(|_| Error::InvalidConfig("recover_processing"))?
            .validate()
            .map(Some)
    }

    pub fn deduplicate_verified_events(&self) -> Result<bool> {
        match self.options.get("deduplicate_verified_events") {
            None | Some(Value::Null) => Ok(false),
            Some(Value::Bool(enabled))
                if !enabled || self.atomic_database_queue()? || self.recover_dispatch()? =>
            {
                Ok(*enabled)
            }
            _ => Err(Error::InvalidConfig("deduplicate_verified_events")),
        }
    }

    pub fn with_verified_event_deduplication(mut self, enabled: bool) -> Self {
        if self.options.is_null() {
            self.options = serde_json::json!({});
        }
        self.options["deduplicate_verified_events"] = Value::Bool(enabled);
        self
    }

    pub fn body_deduplication_window(&self) -> Result<Option<u32>> {
        let value = &self.options["deduplicate_body_seconds"];
        if value.is_null() {
            return Ok(None);
        }
        if (!self.atomic_database_queue()? && !self.recover_dispatch()?)
            || self.deduplicate_verified_events()?
        {
            return Err(Error::InvalidConfig("deduplicate_body_seconds"));
        }
        value
            .as_u64()
            .filter(|seconds| (1..=3600).contains(seconds))
            .and_then(|seconds| u32::try_from(seconds).ok())
            .map(Some)
            .ok_or(Error::InvalidConfig("deduplicate_body_seconds"))
    }

    pub fn with_body_deduplication(mut self, window_seconds: u32) -> Result<Self> {
        if !(1..=3600).contains(&window_seconds) {
            return Err(Error::InvalidConfig("deduplicate_body_seconds"));
        }
        if self.options.is_null() {
            self.options = serde_json::json!({});
        }
        self.options["deduplicate_body_seconds"] = Value::from(window_seconds);
        Ok(self)
    }

    pub fn attachment_disk(&self) -> Result<Option<&str>> {
        match self.options.get("attachment_disk") {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(disk)) => {
                crate::factories::validate_reference("attachment_disk", disk)?;
                Ok(Some(disk))
            }
            _ => Err(Error::InvalidConfig("attachment_disk")),
        }
    }

    pub fn with_attachment_disk(self, disk: impl Into<String>) -> Result<Self> {
        self.with_implementation("attachment_disk", disk)
    }

    pub fn with_store_attachments(mut self, enabled: bool) -> Self {
        if self.options.is_null() {
            self.options = serde_json::json!({});
        }
        self.options["store_attachments"] = Value::Bool(enabled);
        self
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
        if !valid_scope(&scope) {
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
        config.observe_receipts()?;
        if config.verified_requests_per_minute()?.is_some() {
            config.verified_rate_scope()?;
        }
        config.store_attachments()?;
        config.attachment_disk()?;
        config.atomic_database_queue()?;
        config.recover_dispatch()?;
        config.processing_recovery()?;
        config.deduplicate_verified_events()?;
        config.body_deduplication_window()?;
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

fn valid_scope(scope: &str) -> bool {
    !scope.is_empty() && scope.len() <= 128 && !scope.chars().any(char::is_control)
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
        let configs = configured_profiles(&value["configs"])?;
        Ok(Self {
            configs: RwLock::default(),
            source: Some((configs.to_vec(), env)),
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

pub(crate) fn configured_profiles(configs: &Value) -> Result<&[Value]> {
    let configs = configs
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
    Ok(configs)
}
