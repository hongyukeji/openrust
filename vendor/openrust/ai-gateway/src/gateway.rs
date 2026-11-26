use std::{
    collections::{BTreeMap, HashSet},
    io,
    sync::Arc,
};

use openrust::{
    cache::limiters::ConcurrencyLease, config::Environment, container::ServiceKey,
    database::DatabaseResult, log::Logger, support::facades::Facade,
};
use openrust_ai::{AiManager, TextEndpoint, TextProvider};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use crate::{
    accounts::{Account, MAX_SECONDS},
    managed_keys::KeyStore,
    session_affinity::{self, SessionAffinity},
    usage::{Settlement, UsageAttempt, UsageStore},
};

#[derive(Default, Deserialize)]
struct Configuration {
    #[serde(default)]
    enabled: bool,
    #[serde(default)]
    unversioned_paths: bool,
    #[serde(default)]
    models: BTreeMap<String, ModelConfiguration>,
    #[serde(default)]
    accounts: BTreeMap<String, AccountConfiguration>,
    account_cache_store: Option<String>,
    account_session_ttl: Option<i64>,
    #[serde(default)]
    groups: BTreeMap<String, GroupConfiguration>,
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
    cooldown_seconds: Option<i64>,
    max_concurrent_requests: Option<usize>,
}

#[derive(Deserialize)]
struct GroupConfiguration {
    models: HashSet<String>,
    providers: Option<HashSet<String>>,
}

struct Group {
    name: String,
    models: HashSet<String>,
    providers: Option<HashSet<String>>,
}

#[derive(Deserialize)]
struct KeyConfiguration {
    key_env: String,
    group: Option<String>,
    models: Option<HashSet<String>>,
    requests_per_minute: Option<u64>,
}

pub(crate) struct Key {
    pub(crate) name: String,
    managed_id: Option<String>,
    digest: Option<[u8; 32]>,
    group: Option<Arc<Group>>,
    models: Option<HashSet<String>>,
    pub(crate) requests_per_minute: Option<u64>,
}

impl Key {
    pub(crate) fn limiter_key(&self) -> String {
        match &self.managed_id {
            Some(id) => format!("ai-gateway:managed-key:{id}"),
            None => format!("ai-gateway:{}", self.name),
        }
    }

    pub(crate) fn allows_model(&self, name: &str) -> bool {
        self.models
            .as_ref()
            .is_none_or(|models| models.contains(name))
            && self
                .group
                .as_ref()
                .is_none_or(|group| group.models.contains(name))
    }

    fn allows_target(&self, target: &Target) -> bool {
        self.group
            .as_ref()
            .and_then(|group| group.providers.as_ref())
            .is_none_or(|providers| providers.contains(&target.provider_name))
    }
}

const ENDPOINTS: [TextEndpoint; 9] = [
    TextEndpoint::ChatCompletions,
    TextEndpoint::Responses,
    TextEndpoint::CountResponseTokens,
    TextEndpoint::Messages,
    TextEndpoint::CountMessageTokens,
    TextEndpoint::GeminiGenerateContent,
    TextEndpoint::GeminiStreamGenerateContent,
    TextEndpoint::GeminiCountTokens,
    TextEndpoint::Embeddings,
];

pub(crate) struct Target {
    pub(crate) affinity_key: String,
    provider_name: String,
    pub(crate) provider: Arc<dyn TextProvider>,
    pub(crate) upstream_model: String,
    pub(crate) account: Option<Arc<Account>>,
}

pub(crate) struct Model {
    targets: Vec<Target>,
    endpoints: Vec<TextEndpoint>,
}

impl Model {
    pub(crate) fn supports_key(&self, key: &Key) -> bool {
        self.endpoints
            .iter()
            .any(|endpoint| self.supports_for(*endpoint, key))
    }

    pub(crate) fn supports_for(&self, endpoint: TextEndpoint, key: &Key) -> bool {
        self.targets_for(endpoint, key).next().is_some()
    }

    pub(crate) fn targets_for<'a>(
        &'a self,
        endpoint: TextEndpoint,
        key: &'a Key,
    ) -> impl Iterator<Item = (&'a Target, TextEndpoint)> {
        self.targets
            .iter()
            .filter(move |target| self.endpoints.contains(&endpoint) && key.allows_target(target))
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
    let translated: &[TextEndpoint] = match endpoint {
        TextEndpoint::Responses => &[TextEndpoint::Messages, TextEndpoint::ChatCompletions],
        TextEndpoint::ChatCompletions => &[TextEndpoint::Messages],
        TextEndpoint::Messages => &[TextEndpoint::Responses, TextEndpoint::ChatCompletions],
        TextEndpoint::CountResponseTokens => &[TextEndpoint::CountMessageTokens],
        TextEndpoint::CountMessageTokens => &[TextEndpoint::CountResponseTokens],
        _ => &[],
    };
    translated
        .iter()
        .copied()
        .find(|endpoint| provider.supports_endpoint(*endpoint))
}

pub(crate) struct Gateway {
    pub(crate) models: BTreeMap<String, Model>,
    pub(crate) unversioned_paths: bool,
    pub(crate) account_cache_store: Option<String>,
    pub(crate) account_session_ttl: Option<i64>,
    groups: BTreeMap<String, Arc<Group>>,
    keys: Vec<Arc<Key>>,
    pub(crate) managed_keys: Option<KeyStore>,
    pub(crate) usage_store: Option<UsageStore>,
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
        managed_keys: Option<KeyStore>,
        usage_store: Option<UsageStore>,
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
        if config
            .account_session_ttl
            .is_some_and(|seconds| !(1..=MAX_SECONDS).contains(&seconds))
        {
            return Err(invalid("account_session_ttl 必须介于 1 和 86400 秒"));
        }
        let mut accounts = BTreeMap::new();
        for (name, policy) in config.accounts {
            if policy
                .cooldown_seconds
                .is_some_and(|seconds| !(1..=MAX_SECONDS).contains(&seconds))
                || policy.max_concurrent_requests == Some(0)
                || ai.get_instance_config(&name).is_err()
            {
                return Err(invalid(format!("上游账号 {name} 的连接或冷却配置无效")));
            }
            accounts.insert(
                name.clone(),
                Arc::new(Account::new(
                    &name,
                    policy.cooldown_seconds,
                    policy.max_concurrent_requests,
                )),
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
                        affinity_key: session_affinity::key(&[&target.provider, &target.model]),
                        provider,
                        upstream_model: target.model,
                        account: accounts.get(&target.provider).cloned(),
                        provider_name: target.provider,
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

        let mut groups = BTreeMap::new();
        for (name, group) in config.groups {
            if name.trim().is_empty()
                || name.chars().count() > 128
                || name.chars().any(char::is_control)
                || group.models.iter().any(|model| !models.contains_key(model))
                || group.providers.as_ref().is_some_and(|providers| {
                    providers
                        .iter()
                        .any(|provider| ai.get_instance_config(provider).is_err())
                })
            {
                return Err(invalid(format!("网关分组 {name} 的模型或连接配置无效")));
            }
            groups.insert(
                name.clone(),
                Arc::new(Group {
                    name,
                    models: group.models,
                    providers: group.providers,
                }),
            );
        }

        let mut keys = Vec::<Arc<Key>>::new();
        for (name, key) in config.keys {
            if key
                .requests_per_minute
                .is_some_and(|limit| limit == 0 || limit > i64::MAX as u64)
            {
                return Err(invalid(format!("Key {name} 的每分钟请求限额无效")));
            }
            let group = key
                .group
                .map(|group| {
                    groups
                        .get(&group)
                        .cloned()
                        .ok_or_else(|| invalid(format!("Key {name} 的分组未配置")))
                })
                .transpose()?;
            if key
                .models
                .as_ref()
                .is_some_and(|scope| scope.iter().any(|model| !models.contains_key(model)))
            {
                return Err(invalid(format!("Key {name} 授权了未发布的模型")));
            }
            if key.models.is_none() && group.is_none() {
                return Err(invalid(format!("Key {name} 必须指定分组或模型")));
            }
            let value = env
                .get_utf8(&key.key_env)
                .ok()
                .flatten()
                .filter(|value| !value.is_empty() && !value.chars().any(char::is_whitespace))
                .ok_or_else(|| invalid(format!("Key {name} 的环境凭证缺失或无效")))?;
            let digest: [u8; 32] = Sha256::digest(value.as_bytes()).into();
            if keys
                .iter()
                .filter_map(|key| key.digest.as_ref())
                .any(|existing| bool::from(existing.ct_eq(&digest)))
            {
                return Err(invalid("ai_gateway.keys 的凭证重复"));
            }
            keys.push(Arc::new(Key {
                name,
                managed_id: None,
                digest: Some(digest),
                models: key.models,
                group,
                requests_per_minute: key.requests_per_minute,
            }));
        }
        Ok(Some(Self {
            models,
            unversioned_paths: config.unversioned_paths,
            account_cache_store: config.account_cache_store,
            account_session_ttl: config.account_session_ttl,
            keys,
            groups,
            managed_keys,
            usage_store,
            logger,
        }))
    }

    pub(crate) fn has_group(&self, name: &str) -> bool {
        self.groups.contains_key(name)
    }

    pub(crate) async fn authorize(&self, bearer: &str) -> DatabaseResult<Option<Arc<Key>>> {
        let digest: [u8; 32] = Sha256::digest(bearer.as_bytes()).into();
        let mut matched = None;
        for key in &self.keys {
            if key
                .digest
                .as_ref()
                .is_some_and(|expected| expected.ct_eq(&digest).into())
            {
                matched = Some(key);
            }
        }
        if let Some(key) = matched {
            return Ok(Some(key.clone()));
        }
        let Some(store) = &self.managed_keys else {
            return Ok(None);
        };
        let Some(record) = store.authenticate(bearer).await? else {
            return Ok(None);
        };
        let Some(group) = self.groups.get(&record.group) else {
            return Ok(None);
        };
        Ok(Some(Arc::new(Key {
            name: record.name,
            managed_id: Some(record.id),
            digest: None,
            group: Some(group.clone()),
            models: None,
            requests_per_minute: record
                .requests_per_minute
                .and_then(|limit| u64::try_from(limit).ok()),
        })))
    }

    pub(crate) async fn usage(
        &self,
        key: &Key,
        name: &str,
        target: &Target,
        endpoints: (TextEndpoint, TextEndpoint),
        request_id: &str,
        lease: Option<ConcurrencyLease>,
    ) -> DatabaseResult<Call> {
        let (endpoint, upstream_endpoint) = endpoints;
        let mut context: openrust::log::LogContext = [
            ("key".into(), json!(key.name)),
            ("key_id".into(), json!(key.managed_id)),
            (
                "group".into(),
                json!(key.group.as_ref().map(|group| &group.name)),
            ),
            ("request_id".into(), json!(request_id)),
            ("model".into(), json!(name)),
            ("upstream_model".into(), json!(target.upstream_model)),
            ("provider".into(), json!(target.provider_name)),
            ("endpoint".into(), json!(endpoint.path())),
            ("upstream_endpoint".into(), json!(upstream_endpoint.path())),
            ("input_tokens".into(), Value::Null),
            ("output_tokens".into(), Value::Null),
        ]
        .into_iter()
        .collect();
        let attempt = match &self.usage_store {
            Some(store) => {
                let attempt = store.begin(&context).await?;
                context.insert("attempt_id".into(), json!(attempt.id));
                Some(attempt)
            }
            None => None,
        };
        Ok(Call {
            logger: self.logger.clone(),
            context,
            attempt,
            outcome: "cancelled",
            affinity: None,
            lease,
        })
    }
}

pub(crate) struct Call {
    logger: Arc<Logger>,
    context: openrust::log::LogContext,
    attempt: Option<UsageAttempt>,
    outcome: &'static str,
    affinity: Option<(SessionAffinity, String)>,
    lease: Option<ConcurrencyLease>,
}

impl Call {
    pub(crate) fn bind_affinity(&mut self, affinity: SessionAffinity, target: String) {
        self.affinity = Some((affinity, target));
    }

    pub(crate) async fn complete(&mut self) {
        self.outcome("completed");
        if let Some((affinity, target)) = self.affinity.take() {
            affinity.remember(target).await;
        }
    }

    pub(crate) async fn settle(&mut self) {
        if let Some(attempt) = &self.attempt
            && attempt
                .finish(Settlement::from_context(self.outcome, &self.context))
                .await
                .is_ok()
        {
            self.attempt = None;
        }
    }

    pub(crate) async fn release(&mut self) {
        if let Some(lease) = self.lease.take()
            && lease.release().await.is_err()
        {
            eprintln!("AI 网关账号槽位释放失败");
        }
    }

    pub(crate) fn outcome(&mut self, outcome: &'static str) {
        self.outcome = outcome;
    }

    pub(crate) fn status(&mut self, status: http::StatusCode) {
        self.context.insert("status".into(), json!(status.as_u16()));
    }

    pub(crate) fn observe_usage(&mut self, payload: &Value) {
        let usage = &payload["usage"];
        let google = &payload["usageMetadata"];
        for (metric, candidates) in [
            (
                "input_tokens",
                [
                    &usage["input_tokens"],
                    &usage["prompt_tokens"],
                    &google["promptTokenCount"],
                    &Value::Null,
                ],
            ),
            (
                "output_tokens",
                [
                    &usage["output_tokens"],
                    &usage["completion_tokens"],
                    &google["candidatesTokenCount"],
                    &Value::Null,
                ],
            ),
            (
                "cached_input_tokens",
                [
                    &usage["input_tokens_details"]["cached_tokens"],
                    &usage["prompt_tokens_details"]["cached_tokens"],
                    &usage["cache_read_input_tokens"],
                    &google["cachedContentTokenCount"],
                ],
            ),
            (
                "cache_write_input_tokens",
                [
                    &usage["input_tokens_details"]["cache_write_tokens"],
                    &usage["cache_creation_input_tokens"],
                    &Value::Null,
                    &Value::Null,
                ],
            ),
            (
                "reasoning_tokens",
                [
                    &usage["output_tokens_details"]["reasoning_tokens"],
                    &usage["completion_tokens_details"]["reasoning_tokens"],
                    &google["thoughtsTokenCount"],
                    &Value::Null,
                ],
            ),
        ] {
            if let Some(count) = candidates.into_iter().find_map(Value::as_u64) {
                self.context.insert(metric.into(), json!(count));
            }
        }
    }
}

impl Drop for Call {
    fn drop(&mut self) {
        if let Some(attempt) = self.attempt.take()
            && let Ok(runtime) = tokio::runtime::Handle::try_current()
        {
            let settle = Settlement::from_context(self.outcome, &self.context);
            let logger = self.logger.clone();
            runtime.spawn(async move {
                if attempt.finish(settle).await.is_err() {
                    let context = [("attempt_id".into(), json!(attempt.id))]
                        .into_iter()
                        .collect();
                    let _ = logger.log(
                        openrust::log::Level::Warning,
                        "ai_gateway.usage_settlement_failed",
                        &context,
                    );
                }
            });
        }
        self.context.insert("outcome".into(), json!(self.outcome));
        if let Err(error) = self.logger.info("ai_gateway.call", &self.context) {
            eprintln!("AI 网关用量日志写入失败：{error}");
        }
    }
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}
