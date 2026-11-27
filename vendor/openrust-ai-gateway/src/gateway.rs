use std::{
    collections::{BTreeMap, HashSet},
    io,
    sync::Arc,
};

use openrust::{
    cache::limiters::ConcurrencyLease,
    config::Environment,
    container::ServiceKey,
    database::{DatabaseError, DatabaseResult},
    log::Logger,
    support::facades::Facade,
};
use openrust_ai::{AiManager, TextEndpoint, TextProvider};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use crate::{
    accounts::{Account, MAX_SECONDS},
    billing::{Amount, ChargePlan, PriceBook},
    budget::{Budget, BudgetReservation, BudgetReservations, BudgetScope},
    codex_catalog::CodexMetadata,
    invocation::CallIdentity,
    managed_keys::KeyStore,
    output_quota::{OutputQuota, QuotaReservation},
    session_affinity::{self, SessionAffinity},
    usage::{Settlement, UsageAttempt, UsageOwner, UsageStore},
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
    #[serde(default)]
    billing: PriceBook,
}

#[derive(Deserialize)]
struct ModelConfiguration {
    #[serde(flatten)]
    primary: TargetConfiguration,
    endpoints: Option<Vec<String>>,
    codex: Option<CodexMetadata>,
    #[serde(default)]
    server_tools: HashSet<ResponsesServerTool>,
    #[serde(default)]
    file_search_collections: HashSet<String>,
    #[serde(default)]
    fallbacks: Vec<TargetConfiguration>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq)]
#[serde(rename_all = "snake_case")]
enum ResponsesServerTool {
    WebSearch,
    CodeInterpreter,
    FileSearch,
}

impl ResponsesServerTool {
    fn kind(self) -> &'static str {
        match self {
            Self::WebSearch => "web_search",
            Self::CodeInterpreter => "code_interpreter",
            Self::FileSearch => "file_search",
        }
    }
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
    budget: Option<Budget>,
}

#[derive(Deserialize)]
struct GroupConfiguration {
    quota: Option<OutputQuota>,
    budget: Option<Budget>,
    rate_multiplier: Option<Amount>,
    models: HashSet<String>,
    providers: Option<HashSet<String>>,
}

struct Group {
    name: String,
    quota: Option<OutputQuota>,
    budget: Option<Budget>,
    rate_multiplier: Option<Amount>,
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
    pub(crate) fn rate_multiplier(&self) -> Option<Amount> {
        self.group.as_ref().and_then(|group| group.rate_multiplier)
    }

    pub(crate) fn limiter_key(&self) -> String {
        match &self.managed_id {
            Some(id) => format!("ai-gateway:managed-key:{id}"),
            None => format!("ai-gateway:{}", self.name),
        }
    }

    pub(crate) fn usage_owner(&self) -> UsageOwner<'_> {
        match (&self.managed_id, &self.digest) {
            (Some(id), _) => UsageOwner::Managed(id),
            (_, Some(digest)) => UsageOwner::Static(format!(
                "{:x}",
                Sha256::new()
                    .chain_update(b"openrust.ai-gateway.usage")
                    .chain_update(digest)
                    .chain_update(self.name.as_bytes())
                    .finalize(),
            )),
            _ => unreachable!("authorized keys have an identifier"),
        }
    }

    pub(crate) fn output_quota(&self) -> Option<OutputQuota> {
        self.group.as_ref().and_then(|group| group.quota)
    }

    pub(crate) fn output_reservation(
        &self,
        endpoint: TextEndpoint,
        target: &Target,
        payload: &mut Value,
    ) -> Result<Option<QuotaReservation>, &'static str> {
        let Some((group, quota)) = self
            .group
            .as_ref()
            .and_then(|group| group.quota.map(|quota| (group, quota)))
        else {
            return Ok(None);
        };
        Ok(quota
            .prepare(
                endpoint,
                target.provider.supports_endpoint(TextEndpoint::Responses),
                payload,
            )?
            .map(|amount| QuotaReservation::new(&group.name, quota, amount)))
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

const ENDPOINTS: [TextEndpoint; 13] = [
    TextEndpoint::ChatCompletions,
    TextEndpoint::Responses,
    TextEndpoint::CountResponseTokens,
    TextEndpoint::Messages,
    TextEndpoint::CountMessageTokens,
    TextEndpoint::GeminiGenerateContent,
    TextEndpoint::GeminiStreamGenerateContent,
    TextEndpoint::GeminiCountTokens,
    TextEndpoint::GeminiEmbedContent,
    TextEndpoint::GeminiBatchEmbedContents,
    TextEndpoint::Embeddings,
    TextEndpoint::ImagesGenerations,
    TextEndpoint::ImagesEdits,
];

pub(crate) struct Target {
    pub(crate) affinity_key: String,
    provider_name: String,
    pub(crate) provider: Arc<dyn TextProvider>,
    pub(crate) upstream_model: String,
    pub(crate) account: Option<Arc<Account>>,
}

pub(crate) struct Model {
    pub(crate) codex: Option<CodexMetadata>,
    server_tools: HashSet<ResponsesServerTool>,
    file_search_collections: HashSet<String>,
    targets: Vec<Target>,
    endpoints: Vec<TextEndpoint>,
}

impl Model {
    pub(crate) fn allows_server_tool(&self, kind: &str, tool: &Value) -> bool {
        let allowed = match kind {
            "web_search" | "web_search_preview" => ResponsesServerTool::WebSearch,
            "code_interpreter" => ResponsesServerTool::CodeInterpreter,
            "file_search" => ResponsesServerTool::FileSearch,
            _ => return false,
        };
        if !self.server_tools.contains(&allowed) {
            return false;
        }
        match allowed {
            ResponsesServerTool::WebSearch => true,
            // 只允许新沙箱，禁止复用客户端指定的容器。
            ResponsesServerTool::CodeInterpreter => {
                tool.as_object().is_some_and(|fields| fields.len() == 1)
            }
            ResponsesServerTool::FileSearch => {
                let Ok(tool) = openrust_ai::ProviderTool::deserialize(tool) else {
                    return false;
                };
                tool.is_valid()
                    && matches!(tool, openrust_ai::ProviderTool::FileSearch { vector_store_ids, .. }
                        if vector_store_ids.iter().all(|id| self.file_search_collections.contains(id)))
            }
        }
    }

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
    accounts: BTreeMap<String, Arc<Account>>,
    groups: BTreeMap<String, Arc<Group>>,
    keys: Vec<Arc<Key>>,
    pub(crate) managed_keys: Option<KeyStore>,
    pub(crate) usage_store: Option<UsageStore>,
    billing: Option<PriceBook>,
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
        let billing = config.billing.enabled(usage_store.is_some())?;
        let mut accounts = BTreeMap::new();
        for (name, policy) in config.accounts {
            if policy
                .cooldown_seconds
                .is_some_and(|seconds| !(1..=MAX_SECONDS).contains(&seconds))
                || policy.max_concurrent_requests == Some(0)
                || policy
                    .budget
                    .as_ref()
                    .is_some_and(|budget| billing.is_none() || !budget.valid())
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
                    policy.budget,
                )),
            );
        }
        let models = config
            .models
            .into_iter()
            .map(|(name, model)| {
                if model
                    .codex
                    .as_ref()
                    .is_some_and(|metadata| !metadata.is_valid())
                {
                    return Err(invalid(format!("公开模型 {name} 的 Codex 描述无效")));
                }
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
                        model.endpoints.as_ref().map_or(
                            !endpoint.is_image() && !endpoint.is_gemini_embedding(),
                            |configured| configured.iter().any(|path| path == endpoint.path()),
                        )
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
                if model.server_tools.iter().any(|tool| {
                    !endpoints.contains(&TextEndpoint::Responses)
                        || !targets.iter().any(|target| {
                            target.provider.supports_responses_server_tool(tool.kind())
                        })
                }) {
                    return Err(invalid(format!(
                        "公开模型 {name} 的服务端工具要求原生 Responses 上游"
                    )));
                }
                if model
                    .server_tools
                    .contains(&ResponsesServerTool::FileSearch)
                    == model.file_search_collections.is_empty()
                    || model
                        .file_search_collections
                        .iter()
                        .any(|id| !valid_reported_identifier(id))
                {
                    return Err(invalid(format!("公开模型 {name} 的文件搜索集合授权无效")));
                }
                Ok((
                    name,
                    Model {
                        targets,
                        endpoints,
                        codex: model.codex,
                        server_tools: model.server_tools,
                        file_search_collections: model.file_search_collections,
                    },
                ))
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
            if group
                .quota
                .is_some_and(|quota| !quota.valid() || usage_store.is_none())
            {
                return Err(invalid(format!(
                    "网关分组 {name} 的输出配额无效或用量账本未启用"
                )));
            }
            if group
                .budget
                .as_ref()
                .is_some_and(|budget| billing.is_none() || !budget.valid())
            {
                return Err(invalid(format!(
                    "网关分组 {name} 的货币预算无效或计费未启用"
                )));
            }
            groups.insert(
                name.clone(),
                Arc::new(Group {
                    name,
                    quota: group.quota,
                    budget: group.budget,
                    rate_multiplier: group.rate_multiplier,
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
            accounts,
            keys,
            groups,
            managed_keys,
            billing,
            usage_store,
            logger,
        }))
    }

    pub(crate) fn has_group(&self, name: &str) -> bool {
        self.groups.contains_key(name)
    }

    pub(crate) fn group_budget(&self, name: &str) -> Option<&Budget> {
        self.groups.get(name)?.budget.as_ref()
    }

    pub(crate) fn account_budget(&self, name: &str) -> Option<&Budget> {
        self.accounts.get(name)?.budget.as_ref()
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
        identity: CallIdentity<'_>,
        admission: (
            Option<ConcurrencyLease>,
            Option<QuotaReservation>,
            BudgetReservations,
            usize,
        ),
    ) -> Result<Vec<Call>, UsageAdmission> {
        let (endpoint, upstream_endpoint) = endpoints;
        let (mut lease, reservation, budget, count) = admission;
        let mut context: openrust::log::LogContext = [
            ("key".into(), json!(key.name)),
            ("key_id".into(), json!(key.managed_id)),
            (
                "group".into(),
                json!(key.group.as_ref().map(|group| &group.name)),
            ),
            ("request_id".into(), json!(identity.request_id)),
            ("model".into(), json!(name)),
            ("upstream_model".into(), json!(target.upstream_model)),
            ("provider".into(), json!(target.provider_name)),
            ("endpoint".into(), json!(endpoint.path())),
            (
                "upstream_endpoint".into(),
                json!(target.provider.relay_endpoint_name(upstream_endpoint)),
            ),
            ("input_tokens".into(), Value::Null),
            ("output_tokens".into(), Value::Null),
        ]
        .into_iter()
        .collect();
        if let Some(invocation) = identity.invocation {
            context.insert("consumer_invocation".into(), json!(invocation.id));
            context.insert("model_turn".into(), json!(invocation.model_turn));
        }
        let attempts = match &self.usage_store {
            Some(store) => {
                let retry_after = reservation.as_ref().map(QuotaReservation::retry_after);
                let budget_retry = budget.group.as_ref().map(BudgetReservation::retry_after);
                let account_budget_retry =
                    budget.account.as_ref().map(BudgetReservation::retry_after);
                if let Some(reservation) = &reservation {
                    context.insert("reserved_output_tokens".into(), json!(reservation.amount));
                }
                let charges = self.charge_plan(key, name, target, upstream_endpoint);
                store
                    .begin_many(
                        &context,
                        reservation,
                        key.usage_owner(),
                        charges,
                        budget,
                        count,
                    )
                    .await
                    .map_err(|error| match error {
                        DatabaseError::InvalidQuery(crate::budget::DENIED) => budget_retry
                            .map_or(UsageAdmission::Unavailable, UsageAdmission::BudgetLimited),
                        DatabaseError::InvalidQuery(crate::budget::ACCOUNT_DENIED) => {
                            account_budget_retry.map_or(
                                UsageAdmission::Unavailable,
                                UsageAdmission::AccountBudgetLimited,
                            )
                        }
                        _ => UsageAdmission::Unavailable,
                    })?
                    .ok_or_else(|| {
                        retry_after
                            .map(UsageAdmission::Limited)
                            .unwrap_or(UsageAdmission::Unavailable)
                    })?
            }
            None => Vec::new(),
        };
        let mut attempts = attempts.into_iter();
        Ok((0..count)
            .map(|_| {
                let attempt = attempts.next();
                let mut context = context.clone();
                if let Some(attempt) = &attempt {
                    context.insert("attempt_id".into(), json!(attempt.id));
                }
                Call {
                    logger: self.logger.clone(),
                    provider: target.provider.clone(),
                    context,
                    reported_cost_usd_ticks: None,
                    attempt,
                    outcome: "cancelled",
                    dispatch_started: false,
                    affinity: None,
                    lease: lease.take(),
                }
            })
            .collect())
    }

    pub(crate) fn budget_reservations(
        &self,
        key: &Key,
        model: &str,
        target: &Target,
        endpoint: TextEndpoint,
        payload: &mut Value,
    ) -> Result<BudgetReservations, &'static str> {
        let group = key.group.as_ref().and_then(|group| {
            group
                .budget
                .as_ref()
                .map(|budget| (BudgetScope::Group(&group.name), budget))
        });
        let account = target.account.as_ref().and_then(|account| {
            account
                .budget
                .as_ref()
                .map(|budget| (BudgetScope::Account(&target.provider_name), budget))
        });
        if group.is_none() && account.is_none() {
            return Ok(BudgetReservations::default());
        }
        let cap = key
            .output_quota()
            .map(|quota| quota.max_output_tokens)
            .into_iter()
            .chain(group.map(|(_, budget)| budget.max_output_tokens))
            .chain(account.map(|(_, budget)| budget.max_output_tokens))
            .min();
        let charges = self.charge_plan(key, model, target, endpoint);
        let native_openai = target.provider.supports_endpoint(TextEndpoint::Responses);
        let mut prepare = |setting: Option<(BudgetScope<'_>, &Budget)>| match setting {
            Some((scope, budget)) => budget.prepare(
                scope,
                endpoint,
                native_openai,
                payload,
                charges.as_ref(),
                cap,
            ),
            None => Ok(None),
        };
        Ok(BudgetReservations {
            group: prepare(group)?,
            account: prepare(account)?,
        })
    }

    fn charge_plan(
        &self,
        key: &Key,
        name: &str,
        target: &Target,
        endpoint: TextEndpoint,
    ) -> Option<ChargePlan> {
        let billable = endpoint.is_embedding()
            || matches!(
                endpoint,
                TextEndpoint::Responses
                    | TextEndpoint::ChatCompletions
                    | TextEndpoint::Messages
                    | TextEndpoint::GeminiGenerateContent
                    | TextEndpoint::GeminiStreamGenerateContent
            );
        self.billing.as_ref().filter(|_| billable).map(|prices| {
            prices.select(
                &target.provider_name,
                &target.upstream_model,
                name,
                key.rate_multiplier(),
            )
        })
    }
}

pub(crate) enum UsageAdmission {
    Limited(u64),
    BudgetLimited(u64),
    AccountBudgetLimited(u64),
    Unavailable,
}

pub(crate) struct Call {
    logger: Arc<Logger>,
    provider: Arc<dyn TextProvider>,
    reported_cost_usd_ticks: Option<u64>,
    context: openrust::log::LogContext,
    attempt: Option<UsageAttempt>,
    outcome: &'static str,
    dispatch_started: bool,
    affinity: Option<(SessionAffinity, String)>,
    lease: Option<ConcurrencyLease>,
}

impl Call {
    pub(crate) async fn finish_batch(calls: &mut [Self], successful: bool) -> bool {
        let attempts = calls
            .iter()
            .filter_map(|call| {
                let attempt = call.attempt.clone()?;
                let mut settlement = call.settlement();
                if successful {
                    settlement.outcome = "completed";
                }
                Some((attempt, settlement))
            })
            .collect();
        if UsageAttempt::finish_many(attempts).await.is_err() {
            return false;
        }
        for call in calls {
            call.attempt = None;
            if successful {
                call.outcome("completed");
                if let Some((affinity, target)) = call.affinity.take() {
                    affinity.remember(target).await;
                }
            }
            call.release().await;
        }
        true
    }

    pub(crate) fn bind_affinity(&mut self, affinity: SessionAffinity, target: String) {
        self.affinity = Some((affinity, target));
    }

    pub(crate) async fn begin_dispatch(&mut self) -> bool {
        if let Some(attempt) = &self.attempt
            && attempt.begin_dispatch().await.is_err()
        {
            return false;
        }
        self.dispatch_started = true;
        true
    }

    pub(crate) async fn persist_observation(&mut self) -> bool {
        if let Some(attempt) = &self.attempt {
            attempt.observe(&self.settlement()).await.is_ok()
        } else {
            true
        }
    }

    fn settlement(&self) -> Settlement {
        let mut settlement =
            Settlement::from_context(self.outcome, &self.context, self.dispatch_started);
        settlement.reported_cost_usd = self.reported_cost_usd_ticks.map(Amount::from_usd_ticks);
        settlement
    }

    pub(crate) async fn complete(&mut self) -> bool {
        self.outcome("completed");
        if !self.settle().await {
            return false;
        }
        if let Some((affinity, target)) = self.affinity.take() {
            affinity.remember(target).await;
        }
        true
    }

    pub(crate) async fn settle(&mut self) -> bool {
        if let Some(attempt) = &self.attempt {
            if attempt.finish(self.settlement()).await.is_err() {
                return false;
            }
            self.attempt = None;
        }
        true
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

    pub(crate) fn observe_headers(&mut self, headers: &http::HeaderMap) {
        for name in ["x-request-id", "request-id", "x-goog-request-id"] {
            let mut values = headers.get_all(name).iter();
            if let Some(id) = values.next().and_then(|header| header.to_str().ok())
                && values.next().is_none()
                && valid_reported_identifier(id)
            {
                self.context.insert("upstream_request_id".into(), json!(id));
                break;
            }
        }
    }

    pub(crate) fn observe_final_cost(&mut self, payload: &Value) {
        if self.dispatch_started && self.outcome != "completed" {
            self.reported_cost_usd_ticks = self
                .provider
                .normalize_reported_usage(&payload["usage"])
                .and_then(|usage| usage.cost_in_usd_ticks)
                .or(self.reported_cost_usd_ticks);
        }
    }

    pub(crate) fn observe_usage(&mut self, payload: &Value) -> bool {
        if self.outcome == "completed" {
            return false;
        }
        let mut changed = false;
        for field in ["model", "modelVersion"] {
            if let Some(model) = payload[field]
                .as_str()
                .filter(|model| valid_reported_identifier(model))
            {
                changed |= self.observe_field("reported_model", json!(model));
                break;
            }
        }
        let usage = &payload["usage"];
        let google = &payload["usageMetadata"];
        let normalized_output = self
            .provider
            .normalize_reported_usage(usage)
            .and_then(|usage| usage.completion_tokens)
            .map_or(Value::Null, |count| json!(count));
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
                    &normalized_output,
                    &usage["output_tokens"],
                    &usage["completion_tokens"],
                    &google["candidatesTokenCount"],
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
                    &usage["output_tokens_details"]["thinking_tokens"],
                    &google["thoughtsTokenCount"],
                ],
            ),
        ] {
            if let Some(count) = candidates.into_iter().find_map(Value::as_u64) {
                changed |= self.observe_field(metric, json!(count));
            }
        }
        changed
    }

    fn observe_field(&mut self, field: &str, value: Value) -> bool {
        if self.context.get(field) == Some(&value) {
            false
        } else {
            self.context.insert(field.into(), value);
            true
        }
    }
}

fn valid_reported_identifier(id: &str) -> bool {
    !id.is_empty() && id.len() <= 255 && id.bytes().all(|byte| byte.is_ascii_graphic())
}

impl Drop for Call {
    fn drop(&mut self) {
        if let Some(attempt) = self.attempt.take()
            && let Ok(runtime) = tokio::runtime::Handle::try_current()
        {
            let settle = self.settlement();
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
