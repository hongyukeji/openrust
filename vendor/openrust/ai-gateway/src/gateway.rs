use std::{
    collections::{BTreeMap, HashSet},
    io,
    sync::Arc,
};

use openrust::{config::Environment, container::ServiceKey, log::Logger, support::facades::Facade};
use openrust_ai::{AiManager, TextEndpoint, TextProvider};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use crate::account_cooldown::{AccountCooldown, MAX_SECONDS};

#[derive(Default, Deserialize)]
struct Configuration {
    #[serde(default)]
    enabled: bool,
    #[serde(default)]
    models: BTreeMap<String, ModelConfiguration>,
    #[serde(default)]
    accounts: BTreeMap<String, AccountConfiguration>,
    account_cache_store: Option<String>,
    #[serde(default)]
    keys: BTreeMap<String, KeyConfiguration>,
}

#[derive(Deserialize)]
struct ModelConfiguration {
    #[serde(flatten)]
    primary: TargetConfiguration,
    endpoints: Option<Vec<String>>,
    #[serde(default)]
    fallbacks: Vec<TargetConfiguration>,
}

#[derive(Deserialize)]
struct TargetConfiguration {
    provider: String,
    model: String,
}

#[derive(Deserialize)]
struct AccountConfiguration {
    cooldown_seconds: i64,
}

#[derive(Deserialize)]
struct KeyConfiguration {
    key_env: String,
    models: Vec<String>,
    requests_per_minute: Option<u64>,
}

pub(crate) struct Key {
    pub(crate) name: String,
    digest: [u8; 32],
    pub(crate) models: Vec<String>,
    pub(crate) requests_per_minute: Option<u64>,
}

const ENDPOINTS: [TextEndpoint; 8] = [
    TextEndpoint::ChatCompletions,
    TextEndpoint::Responses,
    TextEndpoint::Messages,
    TextEndpoint::CountMessageTokens,
    TextEndpoint::GeminiGenerateContent,
    TextEndpoint::GeminiStreamGenerateContent,
    TextEndpoint::GeminiCountTokens,
    TextEndpoint::Embeddings,
];

pub(crate) struct Target {
    pub(crate) provider: Arc<dyn TextProvider>,
    pub(crate) upstream_model: String,
    pub(crate) cooldown: Option<Arc<AccountCooldown>>,
}

pub(crate) struct Model {
    targets: Vec<Target>,
    endpoints: Vec<TextEndpoint>,
}

impl Model {
    pub(crate) fn upstream_endpoint(&self, endpoint: TextEndpoint) -> Option<TextEndpoint> {
        self.targets_for(endpoint)
            .next()
            .map(|(_, upstream)| upstream)
    }

    pub(crate) fn targets_for(
        &self,
        endpoint: TextEndpoint,
    ) -> impl Iterator<Item = (&Target, TextEndpoint)> {
        self.targets
            .iter()
            .filter(move |_| self.endpoints.contains(&endpoint))
            .filter_map(move |target| {
                resolve_endpoint(target.provider.as_ref(), endpoint)
                    .map(|upstream| (target, upstream))
            })
    }
}

fn resolve_endpoint(provider: &dyn TextProvider, endpoint: TextEndpoint) -> Option<TextEndpoint> {
    if provider.supports_endpoint(endpoint) {
        return Some(endpoint);
    }
    let translated = match endpoint {
        TextEndpoint::Responses => TextEndpoint::Messages,
        TextEndpoint::Messages => TextEndpoint::Responses,
        _ => return None,
    };
    provider.supports_endpoint(translated).then_some(translated)
}

pub(crate) struct Gateway {
    pub(crate) models: BTreeMap<String, Model>,
    pub(crate) account_cache_store: Option<String>,
    keys: Vec<Key>,
    logger: Arc<Logger>,
}

impl Facade for Gateway {
    type Service = Self;

    fn get_facade_accessor() -> ServiceKey {
        ServiceKey::typed::<Self>()
    }
}

impl Gateway {
    pub(crate) fn configure(
        config: Option<Value>,
        env: &Environment,
        ai: &AiManager,
        logger: Arc<Logger>,
    ) -> Result<Option<Self>, io::Error> {
        let config = config.unwrap_or_else(|| json!({}));
        // 关闭时允许保留待启用的配置，启动时不会解析连接或 Key。
        if config.get("enabled") == Some(&Value::Bool(false)) {
            return Ok(None);
        }
        let config: Configuration =
            serde_json::from_value(config).map_err(|_| invalid("ai_gateway 配置无效"))?;
        if !config.enabled {
            return Ok(None);
        }
        let mut accounts = BTreeMap::new();
        for (name, policy) in config.accounts {
            if !(1..=MAX_SECONDS).contains(&policy.cooldown_seconds)
                || ai.get_instance_config(&name).is_err()
            {
                return Err(invalid(format!("上游账号 {name} 的连接或冷却配置无效")));
            }
            accounts.insert(
                name.clone(),
                Arc::new(AccountCooldown::new(&name, policy.cooldown_seconds)),
            );
        }
        let models = config
            .models
            .into_iter()
            .map(|(name, model)| {
                if name.trim().is_empty() {
                    return Err(invalid("ai_gateway.models 的公开模型名称不能为空"));
                }
                let mut seen = HashSet::new();
                let mut targets = Vec::new();
                for target in std::iter::once(model.primary).chain(model.fallbacks) {
                    if target.model.trim().is_empty() {
                        return Err(invalid(format!("公开模型 {name} 的上游模型名称不能为空")));
                    }
                    if !seen.insert((target.provider.clone(), target.model.clone())) {
                        return Err(invalid(format!("公开模型 {name} 的上游目标重复")));
                    }
                    let provider = ai
                        .text_provider(Some(&target.provider))
                        .map_err(|_| invalid(format!("公开模型 {name} 的上游连接无效")))?;
                    targets.push(Target {
                        provider,
                        upstream_model: target.model,
                        cooldown: accounts.get(&target.provider).cloned(),
                    });
                }
                let endpoints: Vec<_> = ENDPOINTS
                    .into_iter()
                    .filter(|endpoint| {
                        targets.iter().any(|target| {
                            resolve_endpoint(target.provider.as_ref(), *endpoint).is_some()
                        })
                    })
                    .filter(|endpoint| {
                        model.endpoints.as_ref().is_none_or(|configured| {
                            configured.iter().any(|path| path == endpoint.path())
                        })
                    })
                    .collect();
                if endpoints.is_empty()
                    || model
                        .endpoints
                        .as_ref()
                        .is_some_and(|configured| configured.len() != endpoints.len())
                {
                    return Err(invalid(format!(
                        "公开模型 {name} 的端点为空、重复或不受该连接支持"
                    )));
                }
                Ok((name, Model { targets, endpoints }))
            })
            .collect::<Result<BTreeMap<_, _>, _>>()?;

        let mut keys = Vec::<Key>::new();
        for (name, key) in config.keys {
            if key
                .requests_per_minute
                .is_some_and(|limit| limit == 0 || limit > i64::MAX as u64)
            {
                return Err(invalid(format!("Key {name} 的每分钟请求限额无效")));
            }
            if key.models.iter().any(|model| !models.contains_key(model)) {
                return Err(invalid(format!("Key {name} 授权了未发布的模型")));
            }
            let value = env
                .get_utf8(&key.key_env)
                .ok()
                .flatten()
                .filter(|value| !value.is_empty() && !value.chars().any(char::is_whitespace))
                .ok_or_else(|| invalid(format!("Key {name} 的环境凭证缺失或无效")))?;
            let digest: [u8; 32] = Sha256::digest(value.as_bytes()).into();
            if keys.iter().any(|key| bool::from(key.digest.ct_eq(&digest))) {
                return Err(invalid("ai_gateway.keys 的凭证重复"));
            }
            keys.push(Key {
                name,
                digest,
                models: key.models,
                requests_per_minute: key.requests_per_minute,
            });
        }
        Ok(Some(Self {
            models,
            account_cache_store: config.account_cache_store,
            keys,
            logger,
        }))
    }

    pub(crate) fn authorize(&self, bearer: &str) -> Option<&Key> {
        let digest: [u8; 32] = Sha256::digest(bearer.as_bytes()).into();
        let mut matched = None;
        for key in &self.keys {
            if key.digest.ct_eq(&digest).into() {
                matched = Some(key);
            }
        }
        matched
    }

    pub(crate) fn usage(
        &self,
        key: &Key,
        name: &str,
        target: &Target,
        endpoints: (TextEndpoint, TextEndpoint),
        request_id: &str,
    ) -> Call {
        let (endpoint, upstream_endpoint) = endpoints;
        Call {
            logger: self.logger.clone(),
            context: [
                ("key".into(), json!(key.name)),
                ("request_id".into(), json!(request_id)),
                ("model".into(), json!(name)),
                ("upstream_model".into(), json!(target.upstream_model)),
                ("provider".into(), json!(target.provider.name())),
                ("endpoint".into(), json!(endpoint.path())),
                ("upstream_endpoint".into(), json!(upstream_endpoint.path())),
                ("input_tokens".into(), Value::Null),
                ("output_tokens".into(), Value::Null),
            ]
            .into_iter()
            .collect(),
            outcome: "cancelled",
        }
    }
}

pub(crate) struct Call {
    logger: Arc<Logger>,
    context: openrust::log::LogContext,
    outcome: &'static str,
}

impl Call {
    pub(crate) fn outcome(&mut self, outcome: &'static str) {
        self.outcome = outcome;
    }

    pub(crate) fn status(&mut self, status: http::StatusCode) {
        self.context.insert("status".into(), json!(status.as_u16()));
    }

    pub(crate) fn observe_usage(&mut self, payload: &Value) {
        for (metric, fallback, google) in [
            ("input_tokens", "prompt_tokens", "promptTokenCount"),
            ("output_tokens", "completion_tokens", "candidatesTokenCount"),
        ] {
            let count = payload["usage"]
                .get(metric)
                .or_else(|| payload["usage"].get(fallback))
                .or_else(|| payload["usageMetadata"].get(google))
                .and_then(Value::as_u64);
            if let Some(count) = count {
                self.context.insert(metric.into(), json!(count));
            }
        }
    }
}

impl Drop for Call {
    fn drop(&mut self) {
        self.context.insert("outcome".into(), json!(self.outcome));
        if let Err(error) = self.logger.info("ai_gateway.call", &self.context) {
            eprintln!("AI 网关用量日志写入失败：{error}");
        }
    }
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}
