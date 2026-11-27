use std::sync::Arc;

use hmac_sha256::HMAC;
use openrust::{encryption::Encrypter, http::client, log::LogContext, support::facades::Crypt};
use openrust_ai::TextProvider;
use serde_json::json;

use crate::{
    billing::Amount,
    gateway::Call,
    usage::{Settlement, UsageStore},
};
use serde::{Deserialize, Serialize};
use serde_json::Map;
use subtle::ConstantTimeEq;

use super::{
    Admission, AiError, Facade as _, Gateway, HttpFuture, HttpResult, Key, Protocol, Request,
    Response, StatusCode, Target, TextEndpoint, Value, account_state_unavailable, admit, authorize,
    compact, downstream_headers, rename_model,
};

const PREFIX: &str = "resp_or_";
const ITEM_PREFIX: &str = "item_or_";
const PROTOCOL: Protocol = Protocol::OpenAi;

#[derive(Serialize, Deserialize)]
struct Binding {
    model: String,
    upstream: String,
    scope: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    attempt: Option<String>,
}

pub(super) struct State {
    pub(super) store: bool,
    pub(super) background: bool,
    previous: Option<String>,
}

pub(super) struct Context {
    crypt: Arc<Encrypter>,
    model: String,
    scope: String,
    store: bool,
    previous: Option<String>,
    issued: Option<(String, String)>,
    attempt: Option<String>,
}

pub(super) fn prepare(
    data: &Map<String, Value>,
    enabled: bool,
) -> Result<Option<State>, &'static str> {
    let background = match data.get("background") {
        None | Some(Value::Null | Value::Bool(false)) => false,
        Some(Value::Bool(true)) => true,
        _ => return Err("background 须为布尔值"),
    };
    let store = match data.get("store") {
        None | Some(Value::Null) => background,
        Some(Value::Bool(value)) => *value,
        _ => return Err("store 须为布尔值"),
    };
    let previous = match data.get("previous_response_id") {
        None | Some(Value::Null) => None,
        Some(Value::String(id)) if token(id).is_some() => Some(id.clone()),
        _ => return Err("previous_response_id 须使用当前网关签发的标识"),
    };
    if !store && !background && previous.is_none() {
        return Ok(None);
    }
    if !enabled {
        return Err("此公开模型未启用 Responses 持久状态");
    }
    Ok(Some(State {
        store,
        background,
        previous,
    }))
}

impl State {
    pub(super) fn bind(
        &self,
        key: &Key,
        model: &str,
        target: &Target,
        payload: &mut Map<String, Value>,
    ) -> Option<Context> {
        let crypt = Crypt::get_facade_root().ok()?;
        let scope = scope(key, model, target)?;
        if let Some(id) = &self.previous {
            let binding = read(&crypt, id)?;
            if binding.model != model || binding.scope != scope {
                return None;
            }
            payload.insert("previous_response_id".into(), binding.upstream.into());
        }
        Some(Context {
            crypt,
            model: model.into(),
            scope,
            store: self.store || self.background,
            previous: self.previous.clone(),
            issued: None,
            attempt: None,
        })
    }
}

impl Context {
    pub(super) fn rewrite(&mut self, response: &mut Value, complete: bool) -> bool {
        let Some(fields) = response.as_object_mut() else {
            return false;
        };
        if let Some(previous) = fields
            .get_mut("previous_response_id")
            .filter(|value| !value.is_null())
        {
            *previous = self.previous.clone().into();
        }
        if !self.store {
            return true;
        }
        let Some(id) = fields.get("id").and_then(Value::as_str) else {
            return !complete;
        };
        let Some(public) = self.issue(id) else {
            return false;
        };
        fields.insert("id".into(), public.into());
        true
    }

    pub(super) fn rewrite_event(&mut self, event: &mut Value, complete: bool) -> bool {
        if let Some(response) = event.get_mut("response") {
            if !self.rewrite(response, complete) {
                return false;
            }
        } else if complete && self.store {
            return false;
        }
        if self.store
            && let Some(id) = event.get_mut("response_id")
        {
            let Some(public) = id.as_str().and_then(|id| self.issue(id)) else {
                return false;
            };
            *id = public.into();
        }
        true
    }

    fn issue(&mut self, id: &str) -> Option<String> {
        if let Some((original, public)) = &self.issued {
            return (original == id).then(|| public.clone());
        }
        if !valid_upstream(id) {
            return None;
        }
        let binding = Binding {
            model: self.model.clone(),
            upstream: id.into(),
            scope: self.scope.clone(),
            attempt: self.attempt.clone(),
        };
        let raw = self
            .crypt
            .encrypt_string(&serde_json::to_vec(&binding).ok()?)
            .ok()?;
        let public = format!(
            "{PREFIX}{}",
            raw.trim_end_matches('=')
                .replace('+', "-")
                .replace('/', "_")
        );
        token(&public)?;
        self.issued = Some((id.into(), public.clone()));
        Some(public)
    }
}

fn scope(key: &Key, model: &str, target: &Target) -> Option<String> {
    let namespace = target
        .provider
        .step_text_gateway()?
        .approval_replay_scope()
        .ok()??;
    Some(crate::session_affinity::key(&[
        "responses-state",
        &key.relay_scope(model, target),
        &namespace,
    ]))
}

fn token(id: &str) -> Option<&str> {
    let data = id.strip_prefix(PREFIX)?;
    (id.len() <= 4096
        && !data.is_empty()
        && data
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-')))
    .then_some(data)
}

pub(super) fn is_public_id(id: &str) -> bool {
    token(id).is_some()
}

fn valid_upstream(id: &str) -> bool {
    id.starts_with("resp_")
        && id.len() <= 128
        && id.len() > 5
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn read(crypt: &Encrypter, id: &str) -> Option<Binding> {
    let mut data = token(id)?.replace('-', "+").replace('_', "/");
    let padding = (4 - data.len() % 4) % 4;
    data.extend(std::iter::repeat_n('=', padding));
    let binding: Binding = serde_json::from_slice(&crypt.decrypt_string(&data).ok()?).ok()?;
    valid_upstream(&binding.upstream).then_some(binding)
}

pub(super) fn get(request: &mut Request) -> HttpFuture<'_, HttpResult> {
    Box::pin(resource(request, Resource::Get))
}

pub(super) fn delete(request: &mut Request) -> HttpFuture<'_, HttpResult> {
    Box::pin(resource(request, Resource::Delete))
}

pub(super) fn cancel(request: &mut Request) -> HttpFuture<'_, HttpResult> {
    Box::pin(resource(request, Resource::Cancel))
}

pub(super) fn input_items(request: &mut Request) -> HttpFuture<'_, HttpResult> {
    Box::pin(resource(request, Resource::InputItems))
}

#[derive(Copy, Clone, PartialEq)]
enum Resource {
    Get,
    Delete,
    Cancel,
    InputItems,
}

async fn resource(request: &mut Request, resource: Resource) -> HttpResult {
    let gateway = Gateway::get_facade_root()?;
    let key = match authorize(&gateway, PROTOCOL.token(request), PROTOCOL).await {
        Ok(key) => key,
        Err(response) => return response,
    };
    if resource != Resource::InputItems
        && request.uri().query().is_some_and(|query| !query.is_empty())
    {
        return PROTOCOL.error(
            StatusCode::BAD_REQUEST,
            "unsupported_parameter",
            "持久响应读取仅支持 JSON",
        );
    }
    let Some((id, binding, crypt)) = request.route("response_id").and_then(|id| {
        let crypt = Crypt::get_facade_root().ok()?;
        Some((id, read(&crypt, id)?, crypt))
    }) else {
        return not_found();
    };
    let Some(model) = gateway
        .models
        .get(&binding.model)
        .filter(|model| model.responses_state && key.allows_model(&binding.model))
    else {
        return not_found();
    };
    let Some((target, _)) =
        model
            .targets_for(TextEndpoint::Responses, &key)
            .find(|(target, endpoint)| {
                *endpoint == TextEndpoint::Responses
                    && target.provider.supports_response_state()
                    && scope(&key, &binding.model, target).as_ref() == Some(&binding.scope)
            })
    else {
        return not_found();
    };
    if resource == Resource::Cancel && binding.attempt.is_none() {
        return PROTOCOL.error(
            StatusCode::BAD_REQUEST,
            "unsupported_state",
            "只有后台响应可取消",
        );
    }
    let background_store = if binding.attempt.is_some() {
        let Some(store) = &gateway.usage_store else {
            return storage_error();
        };
        Some(store)
    } else {
        None
    };
    if resource == Resource::Delete
        && let Some((store, attempt)) = background_store.zip(binding.attempt.as_deref())
    {
        match store
            .background_pending(attempt, &background_resource(&binding.upstream))
            .await
        {
            Ok(false) => {}
            Ok(true) => {
                return PROTOCOL.error(
                    StatusCode::CONFLICT,
                    "response_pending",
                    "请先查询后台响应终态或取消响应",
                );
            }
            Err(_) => return storage_error(),
        }
    }
    let query = if resource == Resource::InputItems {
        match items_query(request, &crypt, &binding) {
            Ok(query) => query,
            Err(message) => {
                return PROTOCOL.error(StatusCode::BAD_REQUEST, "invalid_request", message);
            }
        }
    } else {
        Vec::new()
    };
    if let Some(response) = admit(&key, PROTOCOL).await? {
        return Ok(response);
    }
    let lease = if let Some(account) = &target.account {
        match account
            .acquire(gateway.account_cache_store.as_deref())
            .await
        {
            Ok(Admission::Allowed(lease)) => lease,
            Ok(Admission::Busy) => {
                return PROTOCOL.error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "upstream_busy",
                    "上游连接并发槽位已满",
                );
            }
            Err(_) => return account_state_unavailable(PROTOCOL),
        }
    } else {
        None
    };
    let result = match resource {
        Resource::Get | Resource::Delete => {
            target
                .provider
                .response_resource(&binding.upstream, resource == Resource::Delete)
                .await
        }
        Resource::Cancel => target.provider.cancel_response(&binding.upstream).await,
        Resource::InputItems => {
            target
                .provider
                .response_input_items(&binding.upstream, &query)
                .await
        }
    };
    if let Some(lease) = lease {
        let _ = lease.release().await;
    }
    let reply = match result {
        Ok(reply) => reply,
        Err(AiError::Timeout) => {
            return PROTOCOL.error(
                StatusCode::GATEWAY_TIMEOUT,
                "upstream_timeout",
                "上游请求超时",
            );
        }
        Err(_) => return PROTOCOL.error(StatusCode::BAD_GATEWAY, "upstream_error", "上游连接失败"),
    };
    match reply.status() {
        StatusCode::NOT_FOUND => return not_found(),
        status if !status.is_success() => {
            return PROTOCOL.error(
                if status.is_client_error() || status.is_server_error() {
                    status
                } else {
                    StatusCode::BAD_GATEWAY
                },
                "upstream_error",
                "上游未能处理持久响应请求",
            );
        }
        _ => {}
    }
    let mut data = reply.json::<Value>().unwrap_or(Value::Null);
    let mut context = model
        .supports_for(TextEndpoint::CompactResponses, &key)
        .then(|| compact::Context::new(&key, &binding.model, target))
        .flatten();
    if resource == Resource::InputItems {
        if !protect_items(&mut data, &binding, &crypt, context.as_mut()) {
            return malformed();
        }
    } else {
        if data["id"].as_str() != Some(binding.upstream.as_str()) {
            return malformed();
        }
        if matches!(resource, Resource::Get | Resource::Cancel)
            && let Some((store, attempt)) = background_store.zip(binding.attempt.as_deref())
            && let Err(error) = finish_background(
                store,
                attempt,
                &binding.upstream,
                &data,
                target.provider.as_ref(),
            )
            .await
        {
            return error;
        }
        if !compact::protect_output(&mut data, context.as_mut(), true) {
            return malformed();
        }
        data["id"] = id.into();
        if let Some(previous) = data.get_mut("previous_response_id") {
            *previous = Value::Null;
        }
        rename_model(&mut data, &binding.model);
    }
    Response::json(reply.status(), data)
        .map(|response| response.with_headers(downstream_headers(reply.headers())))
}

pub(super) async fn respond_background(
    upstream: client::Response,
    target: &Target,
    mut context: Context,
    mut compact_context: Option<compact::Context>,
    mut call: Call,
    store: &UsageStore,
) -> HttpResult {
    let status = upstream.status();
    let headers = downstream_headers(upstream.headers());
    let Some(mut data) = upstream.json::<Value>().ok() else {
        call.outcome("invalid_upstream_response");
        return malformed();
    };
    let Some(id) = data["id"]
        .as_str()
        .filter(|id| valid_upstream(id))
        .map(str::to_owned)
    else {
        call.outcome("invalid_upstream_response");
        return malformed();
    };
    let Some(attempt) = call.attempt_id().map(str::to_owned) else {
        return storage_error();
    };
    if !call.park_background(&background_resource(&id)).await {
        return storage_error();
    }
    call.release().await;
    context.attempt = Some(attempt.clone());
    if let Err(error) =
        finish_background(store, &attempt, &id, &data, target.provider.as_ref()).await
    {
        return error;
    }
    if !compact::protect_output(&mut data, compact_context.as_mut(), true)
        || !context.rewrite(&mut data, true)
    {
        return malformed();
    }
    rename_model(&mut data, &context.model);
    Response::json(status, data).map(|response| response.with_headers(headers))
}

fn background_resource(id: &str) -> String {
    format!("openai-response:{id}")
}

async fn finish_background(
    store: &UsageStore,
    attempt: &str,
    id: &str,
    data: &Value,
    provider: &dyn TextProvider,
) -> Result<(), HttpResult> {
    if data["object"] != "response" {
        return Err(malformed());
    }
    let outcome = match data["status"].as_str() {
        Some("queued" | "in_progress") => return Ok(()),
        Some("completed") => "completed",
        Some("failed") => "background_failed",
        Some("cancelled") => "background_cancelled",
        Some("incomplete") => "background_incomplete",
        _ => return Err(malformed()),
    };
    let mut context = LogContext::new();
    context.insert("upstream_endpoint".into(), json!("responses"));
    if let Some(model) = data["model"]
        .as_str()
        .filter(|model| crate::gateway::valid_reported_identifier(model))
    {
        context.insert("reported_model".into(), json!(model));
    }
    let usage = provider
        .normalize_reported_usage(&data["usage"])
        .unwrap_or_default();
    for (field, count) in [
        ("input_tokens", usage.prompt_tokens),
        ("output_tokens", usage.completion_tokens),
        ("cached_input_tokens", usage.cache_read_input_tokens),
        ("cache_write_input_tokens", usage.cache_write_input_tokens),
        ("reasoning_tokens", usage.reasoning_tokens),
    ] {
        if let Some(count) = count.and_then(|count| i64::try_from(count).ok()) {
            context.insert(field.into(), json!(count));
        }
    }
    // 终态的上游用量可确认为成本。失败、取消和不完整响应不产生下游应收。
    context.insert("confirmed_error_usage".into(), json!(true));
    let mut settlement = Settlement::from_context(outcome, &context, true);
    settlement.reported_cost_usd = usage.cost_in_usd_ticks.map(Amount::from_usd_ticks);
    store
        .finish_background(attempt, &background_resource(id), settlement)
        .await
        .map_err(|_| storage_error())
}

fn storage_error() -> HttpResult {
    PROTOCOL.error(
        StatusCode::SERVICE_UNAVAILABLE,
        "usage_unavailable",
        "后台响应用量暂无法持久化",
    )
}

fn item_signature(key: &[u8], binding: &Binding, item: &str) -> String {
    let message = crate::session_affinity::key(&[
        "responses-input-item",
        &binding.scope,
        &binding.upstream,
        item,
    ]);
    let signature: sha2::digest::Output<sha2::Sha256> = HMAC::mac(message, key).into();
    format!("{signature:x}")
}

fn open_cursor<'a>(crypt: &Encrypter, binding: &Binding, cursor: &'a str) -> Option<&'a str> {
    let (item, signature) = cursor.strip_prefix(ITEM_PREFIX)?.split_once('.')?;
    if !valid_item_id(item) || signature.len() != 64 {
        return None;
    }
    crypt
        .get_all_keys()
        .into_iter()
        .any(|key| {
            bool::from(
                signature
                    .as_bytes()
                    .ct_eq(item_signature(key, binding, item).as_bytes()),
            )
        })
        .then_some(item)
}

fn items_query(
    request: &Request,
    crypt: &Encrypter,
    binding: &Binding,
) -> Result<Vec<(String, String)>, &'static str> {
    let mut query = Vec::<(String, String)>::new();
    let mut includes = 0;
    for (name, value) in
        url::form_urlencoded::parse(request.uri().query().unwrap_or_default().as_bytes())
    {
        let name = name.as_ref();
        if name != "include" && name != "include[]" && query.iter().any(|(key, _)| key == name) {
            return Err("输入条目查询参数重复");
        }
        match name {
            "after" => {
                let item = open_cursor(crypt, binding, &value)
                    .ok_or("after 须使用此响应的输入条目标识")?;
                query.push((name.into(), item.into()));
            }
            "limit"
                if value
                    .parse::<u8>()
                    .is_ok_and(|limit| (1..=100).contains(&limit)) =>
            {
                query.push((name.into(), value.into_owned()));
            }
            "order" if matches!(value.as_ref(), "asc" | "desc") => {
                query.push((name.into(), value.into_owned()));
            }
            "include" | "include[]"
                if matches!(
                    value.as_ref(),
                    "file_search_call.results"
                        | "web_search_call.results"
                        | "web_search_call.action.sources"
                        | "message.input_image.image_url"
                        | "computer_call_output.output.image_url"
                        | "code_interpreter_call.outputs"
                        | "reasoning.encrypted_content"
                        | "message.output_text.logprobs"
                ) =>
            {
                includes += 1;
                if includes > 8 {
                    return Err("include 字段过多");
                }
                query.push(("include".into(), value.into_owned()));
            }
            _ => return Err("输入条目查询参数无效"),
        }
    }
    Ok(query)
}

fn valid_item_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn protect_items(
    data: &mut Value,
    binding: &Binding,
    crypt: &Encrypter,
    mut compact: Option<&mut compact::Context>,
) -> bool {
    if data["object"] != "list" || !data["has_more"].is_boolean() {
        return false;
    }
    let first = data["first_id"].clone();
    let last = data["last_id"].clone();
    let Some(items) = data["data"].as_array_mut() else {
        return false;
    };
    if items.is_empty() {
        return first.is_null() && last.is_null() && data["has_more"] == false;
    }
    if items.len() > 100 || items[0]["id"] != first || items[items.len() - 1]["id"] != last {
        return false;
    }
    for item in items.iter_mut() {
        let Some(raw) = item["id"].as_str().filter(|id| valid_item_id(id)) else {
            return false;
        };
        let public = format!(
            "{ITEM_PREFIX}{raw}.{}",
            item_signature(crypt.get_key(), binding, raw)
        );
        let upstream_id = item["id"].clone();
        item["id"] = public.into();
        if !compact::protect_list_item(item, compact.as_deref_mut(), upstream_id) {
            return false;
        }
    }
    let first = items[0]["id"].clone();
    let last = items[items.len() - 1]["id"].clone();
    data["first_id"] = first;
    data["last_id"] = last;
    true
}

fn malformed() -> HttpResult {
    PROTOCOL.error(
        StatusCode::BAD_GATEWAY,
        "invalid_upstream_response",
        "上游未返回有效的持久响应资源",
    )
}

fn not_found() -> HttpResult {
    PROTOCOL.error(StatusCode::NOT_FOUND, "response_not_found", "响应不可用")
}
