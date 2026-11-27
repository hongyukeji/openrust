use openrust::{
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    log::LogContext,
    routing::{Router, RoutingError},
    support::{Str, facades::Facade},
};
use openrust_ai::{AiError, GeminiBatchRequest, TextEndpoint};
use serde_json::{Map, Value, json};

use super::{
    Admission, AttemptAdmission, CallIdentity, Gateway, HttpResult, Key, Protocol, Target,
    UsageAdmission, account_state_unavailable, admit, authorize, rename_model, resources,
};
use crate::usage::{Settlement, UsageStore, gemini_batches::Batch};

const PROTOCOL: Protocol = Protocol::Gemini;

pub(super) fn routes(router: &mut Router) -> Result<(), RoutingError> {
    router.get("/batches", list_route)?;
    router.get("/batches/{id}", get_route)?;
    router.post("/batches/{id}", cancel_route)?;
    router.delete("/batches/{id}", delete_route)?;
    Ok(())
}

fn list_route(request: &mut Request) -> HttpFuture<'_, HttpResult> {
    Box::pin(list(request))
}

fn get_route(request: &mut Request) -> HttpFuture<'_, HttpResult> {
    Box::pin(manage(request, Action::Get))
}

fn cancel_route(request: &mut Request) -> HttpFuture<'_, HttpResult> {
    Box::pin(manage(request, Action::Cancel))
}

fn delete_route(request: &mut Request) -> HttpFuture<'_, HttpResult> {
    Box::pin(manage(request, Action::Delete))
}

pub(super) async fn create(
    request: &mut Request,
    model: String,
    endpoint: TextEndpoint,
) -> HttpResult {
    let gateway = Gateway::get_facade_root()?;
    let key = match authorize(&gateway, PROTOCOL.token(request), PROTOCOL).await {
        Ok(key) => key,
        Err(error) => return error,
    };
    let Some(model_config) = gateway
        .models
        .get(&model)
        .filter(|_| key.allows_model(&model))
    else {
        return not_found();
    };
    if !model_config.supports_for(endpoint, &key) {
        return not_found();
    }
    let Some(store) = &gateway.usage_store else {
        return storage_error();
    };
    let body = match request.body_bytes().await {
        Ok(body) => body,
        Err(HttpError::PayloadTooLarge) => return super::request_too_large(PROTOCOL),
        Err(_) => return invalid("无法读取批任务正文"),
    };
    let Ok(mut data) = serde_json::from_slice::<Value>(&body) else {
        return invalid("批任务需要 JSON 对象");
    };
    if data.as_object().is_none_or(|body| body.len() != 1) {
        return invalid("批任务需要 batch 对象");
    }
    let count = match validate_inline(&data["batch"], &model, endpoint) {
        Ok(count) => count,
        Err(message) => return invalid(message),
    };
    let payload = &mut data["batch"];
    let cache_store = gateway.account_cache_store.as_deref();
    for (target, _) in model_config.targets_for(endpoint, &key) {
        let Some(provider) = target.provider.gemini_batch_gateway() else {
            continue;
        };
        let Ok(scope) = provider.batch_scope() else {
            continue;
        };
        if let Some(cooldown) = target
            .account
            .as_ref()
            .and_then(|account| account.cooldown.as_ref())
        {
            match cooldown.remaining(cache_store).await {
                Ok(Some(_)) => continue,
                Ok(None) => {}
                Err(_) => return account_state_unavailable(PROTOCOL),
            }
        }
        let charges = gateway.charge_plan(&key, &model, target, endpoint, payload);
        let Ok(mut budget) =
            gateway.budget_reservations(&key, target, endpoint, payload, charges.as_ref())
        else {
            return PROTOCOL.error(
                StatusCode::BAD_REQUEST,
                "budget_unbounded",
                "无法确认此批任务的货币费用上限",
            );
        };
        for reservation in [&mut budget.group, &mut budget.account]
            .into_iter()
            .flatten()
        {
            let Some(atoms) = reservation.atoms.checked_mul(i64::from(count)) else {
                return invalid("批任务预算超出可用金额范围");
            };
            reservation.atoms = atoms;
        }
        let quota = match key.output_reservation(endpoint, target, payload) {
            Ok(quota) => quota,
            Err(message) => return invalid(message),
        };
        let lease = if let Some(account) = &target.account {
            match account.acquire(cache_store).await {
                Ok(Admission::Allowed(lease)) => lease,
                Ok(Admission::Busy) => continue,
                Err(_) => return account_state_unavailable(PROTOCOL),
            }
        } else {
            None
        };
        if let Some(response) = admit(&key, PROTOCOL).await? {
            return Ok(response);
        }
        let request_id = Str::ulid().to_string();
        let identity = CallIdentity {
            request_id: &request_id,
            invocation: None,
        };
        let mut call = match gateway
            .usage(
                &key,
                &model,
                target,
                (endpoint, endpoint),
                identity,
                AttemptAdmission {
                    lease,
                    quota,
                    budget,
                    count: 1,
                    charges,
                    operation: endpoint.path(),
                    hide_chat_usage: false,
                },
            )
            .await
        {
            Ok(mut calls) => calls.pop().expect("批任务应登记一次出站尝试"),
            Err(UsageAdmission::BudgetLimited(seconds)) => {
                return limited(seconds, "budget_exceeded");
            }
            Err(UsageAdmission::AccountBudgetLimited(seconds)) => {
                return limited(seconds, "upstream_budget_exceeded");
            }
            Err(UsageAdmission::Limited(seconds)) => {
                return limited(seconds, "output_quota_exceeded");
            }
            Err(UsageAdmission::Unavailable) => return storage_error(),
        };
        let batch = Batch {
            id: call.attempt_id().expect("批任务需要持久化用量").into(),
            owner: owner(&key),
            scope,
            provider: target.provider_name.clone(),
            model,
            upstream_model: target.upstream_model.clone(),
            upstream_name: None,
            endpoint,
            item_count: count,
        };
        if store.reserve_gemini_batch(&batch).await.is_err() || !call.begin_dispatch().await {
            return storage_error();
        }
        // 提交结果未确认时只保留原尝试，禁止把请求重发给另一个账号。
        call.outcome("batch_submission_unknown");
        rewrite_input_models(payload, &target.upstream_model);
        let operation = if endpoint.is_embedding() {
            GeminiBatchRequest::Embed {
                model: &target.upstream_model,
                batch: payload,
            }
        } else {
            GeminiBatchRequest::Generate {
                model: &target.upstream_model,
                batch: payload,
            }
        };
        let upstream = match provider.send_batch(operation).await {
            Ok(response) => response,
            Err(error) => return driver_error(error),
        };
        let status = upstream.status();
        call.status(status);
        call.observe_headers(upstream.headers());
        if !call.persist_observation().await {
            return storage_error();
        }
        if !status.is_success() {
            call.outcome("upstream_error");
            if let Some(cooldown) = target
                .account
                .as_ref()
                .and_then(|account| account.cooldown.as_ref())
                && matches!(status.as_u16(), 429 | 503 | 529)
                && let Some(until) = cooldown.deadline(upstream.headers())
                && cooldown.pause(cache_store, until).await.is_err()
            {
                return account_state_unavailable(PROTOCOL);
            }
            return upstream_status(status);
        }
        let Some(job) = upstream
            .json::<Value>()
            .ok()
            .filter(|job| upstream_name(job).is_some())
        else {
            return malformed();
        };
        let upstream_name = upstream_name(&job).expect("已核对上游任务名称");
        if store
            .attach_gemini_batch(&batch, upstream_name)
            .await
            .is_err()
        {
            return storage_error();
        }
        call.outcome("batch_submitted");
        call.defer_settlement();
        call.release().await;
        return match present(store, &batch, job).await {
            Ok(job) => Response::json(status, job),
            Err(error) => error,
        };
    }
    account_state_unavailable(PROTOCOL)
}

fn validate_inline(body: &Value, model: &str, endpoint: TextEndpoint) -> Result<i32, &'static str> {
    let object = body.as_object().ok_or("批任务需要 batch 对象")?;
    if object.keys().any(|key| {
        !matches!(
            key.as_str(),
            "displayName" | "display_name" | "inputConfig" | "input_config" | "model"
        )
    }) {
        return Err("批任务含有不支持的字段");
    }
    validate_model(object, model)?;
    let inputs = inputs_shape(body).ok_or("共享连接仅支持内联请求批次")?;
    if inputs.is_empty() {
        return Err("批任务至少需要一项请求");
    }
    for entry in inputs {
        let entry = entry.as_object().ok_or("批任务项须为对象")?;
        if entry
            .keys()
            .any(|key| !matches!(key.as_str(), "request" | "metadata"))
        {
            return Err("批任务项含有不支持的字段");
        }
        let request = entry
            .get("request")
            .and_then(Value::as_object)
            .ok_or("批任务项缺少 request")?;
        validate_model(request, model)?;
        if resources::gemini_file_references(request)
            || [
                "cachedContent",
                "cached_content",
                "continuationToken",
                "continuation_token",
            ]
            .iter()
            .any(|field| resources::present_field(request, field))
        {
            return Err("批任务仅支持当前请求中的资源");
        }
        if let Some(tools) = request.get("tools").filter(|tools| !tools.is_null())
            && tools.as_array().is_none_or(|tools| {
                tools.iter().any(|tool| {
                    tool.as_object().is_none_or(|tool| {
                        tool.len() != 1
                            || tool.keys().any(|name| {
                                !matches!(
                                    name.as_str(),
                                    "functionDeclarations" | "function_declarations"
                                )
                            })
                    })
                })
            })
        {
            return Err("批任务只支持客户端函数工具");
        }
        let contents = if endpoint.is_embedding() {
            "content"
        } else {
            "contents"
        };
        if !resources::present_field(request, contents) {
            return Err("批任务项缺少内容");
        }
    }
    i32::try_from(inputs.len()).map_err(|_| "批任务项数超限")
}

fn validate_model(object: &Map<String, Value>, expected: &str) -> Result<(), &'static str> {
    if object.get("model").is_some_and(|model| {
        model
            .as_str()
            .is_none_or(|model| model.strip_prefix("models/").unwrap_or(model) != expected)
    }) {
        return Err("批任务的模型须与公开模型一致");
    }
    Ok(())
}

fn inputs_shape(body: &Value) -> Option<&Vec<Value>> {
    let input = match (body.get("inputConfig"), body.get("input_config")) {
        (Some(input), None) | (None, Some(input)) => input.as_object()?,
        _ => return None,
    };
    let requests = input.get("requests")?.as_object()?;
    if input.len() != 1 || requests.len() != 1 {
        return None;
    }
    requests.get("requests")?.as_array()
}

fn rewrite_input_models(body: &mut Value, upstream: &str) {
    body.as_object_mut().expect("已验证 batch").remove("model");
    let key = if body.get("inputConfig").is_some() {
        "inputConfig"
    } else {
        "input_config"
    };
    let resource = format!(
        "models/{}",
        upstream.strip_prefix("models/").unwrap_or(upstream)
    );
    for item in body[key]["requests"]["requests"]
        .as_array_mut()
        .expect("已验证批任务请求")
    {
        if let Some(model) = item["request"].get_mut("model") {
            *model = json!(resource);
        }
    }
}

enum Action {
    Get,
    Cancel,
    Delete,
}

async fn manage(request: &mut Request, action: Action) -> HttpResult {
    let gateway = Gateway::get_facade_root()?;
    let key = match authorize(&gateway, PROTOCOL.token(request), PROTOCOL).await {
        Ok(key) => key,
        Err(error) => return error,
    };
    let Some(store) = &gateway.usage_store else {
        return storage_error();
    };
    let Some(path) = request.route("id") else {
        return not_found();
    };
    let id = match action {
        Action::Cancel => path.strip_suffix(":cancel"),
        _ => Some(path),
    };
    let Some(id) = id.filter(|id| valid_id(id)) else {
        return not_found();
    };
    let batch = match store.gemini_batch(&owner(&key), id).await {
        Ok(Some(batch)) => batch,
        Ok(None) => return not_found(),
        Err(_) => return storage_error(),
    };
    let Some(target) = target_for(&gateway, &key, &batch) else {
        return not_found();
    };
    if let Some(response) = admit(&key, PROTOCOL).await? {
        return Ok(response);
    }
    let name = batch
        .upstream_name
        .as_deref()
        .expect("只读取已分配的批任务");
    let provider = target
        .provider
        .gemini_batch_gateway()
        .expect("已验证批任务连接");
    match action {
        Action::Get => match read(store, &batch, target).await {
            Ok(job) => Response::json(StatusCode::OK, job),
            Err(error) => error,
        },
        Action::Cancel => {
            let upstream = match provider
                .send_batch(GeminiBatchRequest::Cancel { name })
                .await
            {
                Ok(upstream) => upstream,
                Err(error) => return driver_error(error),
            };
            if !upstream.status().is_success() {
                return upstream_status(upstream.status());
            }
            Response::json(StatusCode::OK, json!({}))
        }
        Action::Delete => {
            // 完成任务先收集结果证据，再执行可能移除该证据的上游操作。
            if let Err(error) = read(store, &batch, target).await {
                return error;
            }
            let upstream = match provider
                .send_batch(GeminiBatchRequest::Delete { name })
                .await
            {
                Ok(upstream) => upstream,
                Err(error) => return driver_error(error),
            };
            if !upstream.status().is_success() {
                return upstream_status(upstream.status());
            }
            let settlement = Settlement::from_context("batch_deleted", &LogContext::new(), true);
            if store
                .settle_gemini_batch(&batch, settlement, true)
                .await
                .is_err()
            {
                return storage_error();
            }
            Response::json(StatusCode::OK, json!({}))
        }
    }
}

async fn list(request: &mut Request) -> HttpResult {
    let gateway = Gateway::get_facade_root()?;
    let key = match authorize(&gateway, PROTOCOL.token(request), PROTOCOL).await {
        Ok(key) => key,
        Err(error) => return error,
    };
    let Some(store) = &gateway.usage_store else {
        return storage_error();
    };
    let limit = match request.query(Some("pageSize")) {
        Ok(None) => 20,
        Ok(Some(Value::String(limit))) => match limit.parse::<u64>() {
            Ok(limit @ 1..=100) => limit,
            _ => return invalid("pageSize 须为 1 至 100"),
        },
        _ => return invalid("pageSize 无效"),
    };
    let cursor = match request.query(Some("pageToken")) {
        Ok(None) => None,
        Ok(Some(Value::String(cursor))) if valid_id(&cursor) => Some(cursor),
        _ => return invalid("pageToken 无效"),
    };
    if let Some(response) = admit(&key, PROTOCOL).await? {
        return Ok(response);
    }
    let mut batches = match store
        .gemini_batch_page(&owner(&key), cursor.as_deref(), limit + 1)
        .await
    {
        Ok(batches) => batches,
        Err(_) => return storage_error(),
    };
    let more = if batches.len() as u64 > limit {
        batches.pop();
        batches.last().map(|batch| batch.id.clone())
    } else {
        None
    };
    let mut operations = Vec::new();
    for batch in batches {
        if let Some(target) = target_for(&gateway, &key, &batch) {
            match read(store, &batch, target).await {
                Ok(job) => operations.push(job),
                Err(error) => return error,
            }
        }
    }
    let mut page = json!({"operations":operations});
    if let Some(cursor) = more {
        page["nextPageToken"] = cursor.into();
    }
    Response::json(StatusCode::OK, page)
}

async fn read(store: &UsageStore, batch: &Batch, target: &Target) -> Result<Value, HttpResult> {
    let name = batch
        .upstream_name
        .as_deref()
        .expect("只读取已分配的批任务");
    let provider = target
        .provider
        .gemini_batch_gateway()
        .expect("已验证批任务连接");
    let response = provider
        .send_batch(GeminiBatchRequest::Get { name })
        .await
        .map_err(driver_error)?;
    if !response.status().is_success() {
        return Err(upstream_status(response.status()));
    }
    let job = response.json::<Value>().map_err(|_| malformed())?;
    if upstream_name(&job) != Some(name) {
        return Err(malformed());
    }
    present(store, batch, job).await
}

async fn present(store: &UsageStore, batch: &Batch, mut job: Value) -> Result<Value, HttpResult> {
    if let Some(settlement) = batch_settlement(batch, &job)
        && store
            .settle_gemini_batch(batch, settlement, false)
            .await
            .is_err()
    {
        return Err(storage_error());
    }
    let local = format!("batches/{}", batch.id);
    job["name"] = json!(local);
    if let Some(metadata) = job.get_mut("metadata").and_then(Value::as_object_mut) {
        metadata.remove("inputConfig");
        metadata.remove("input_config");
        if metadata.contains_key("name") {
            metadata.insert("name".into(), json!(local));
        }
        if metadata.contains_key("model") {
            metadata.insert("model".into(), json!(format!("models/{}", batch.model)));
        }
    }
    for path in ["/response", "/metadata/output"] {
        if let Some(output) = job.pointer_mut(path).and_then(Value::as_object_mut) {
            output.remove("responsesFile");
            output.remove("responses_file");
            for field in ["inlinedResponses", "inlinedEmbedContentResponses"] {
                let Some(items) = output.get_mut(field) else {
                    continue;
                };
                let items = if items.is_array() {
                    items.as_array_mut()
                } else {
                    items["inlinedResponses"].as_array_mut()
                };
                for item in items.into_iter().flatten() {
                    if let Some(response) = item.get_mut("response") {
                        rename_model(response, &batch.model);
                    }
                    if let Some(error) = item.get_mut("error") {
                        redact_error(error);
                    }
                }
            }
        }
    }
    if let Some(error) = job.get_mut("error").filter(|error| !error.is_null()) {
        redact_error(error);
    }
    Ok(job)
}

fn batch_settlement(batch: &Batch, job: &Value) -> Option<Settlement> {
    let state = job["metadata"]["state"].as_str().unwrap_or_default();
    let successful = matches!(
        state,
        "JOB_STATE_SUCCEEDED" | "JOB_STATE_PARTIALLY_SUCCEEDED" | "BATCH_STATE_SUCCEEDED"
    );
    let failed = !job["error"].is_null()
        || matches!(
            state,
            "JOB_STATE_FAILED"
                | "JOB_STATE_CANCELLED"
                | "JOB_STATE_EXPIRED"
                | "BATCH_STATE_FAILED"
                | "BATCH_STATE_CANCELLED"
                | "BATCH_STATE_EXPIRED"
        );
    if job["done"] != true && !successful && !failed {
        return None;
    }
    let mut context = LogContext::new();
    context.insert("upstream_endpoint".into(), json!(batch.endpoint.path()));
    if failed {
        return Some(Settlement::from_context("batch_failed", &context, true));
    }
    // 上游可能先回报终态，结果或最终用量随后才可读。
    let items = batch_items(job)
        .filter(|items| i32::try_from(items.len()).ok() == Some(batch.item_count))?;
    let mut successes = Vec::with_capacity(items.len());
    for item in items {
        match (item["response"].as_object(), item["error"].as_object()) {
            (Some(_), None) if item["error"].is_null() => successes.push(&item["response"]),
            (None, Some(_)) if item["response"].is_null() => {}
            _ => return None,
        }
    }
    let cost_pending = successes.len() != items.len();
    for (field, upstream, required) in [
        ("input_tokens", "promptTokenCount", true),
        (
            "output_tokens",
            "candidatesTokenCount",
            !batch.endpoint.is_embedding(),
        ),
        ("cached_input_tokens", "cachedContentTokenCount", false),
        ("reasoning_tokens", "thoughtsTokenCount", false),
    ] {
        let total = successes.iter().try_fold(0_i64, |sum, item| {
            sum.checked_add(
                item["usageMetadata"][upstream]
                    .as_i64()
                    .filter(|value| *value >= 0)?,
            )
        });
        if let Some(count) = total {
            context.insert(field.into(), json!(count));
        } else if required {
            return None;
        }
    }
    if let Some(model) = successes
        .iter()
        .filter_map(|item| item["modelVersion"].as_str())
        .filter(|model| {
            !model.is_empty()
                && model.len() <= 255
                && model.bytes().all(|byte| byte.is_ascii_graphic())
        })
        .find(|model| {
            model.strip_prefix("models/").unwrap_or(model)
                != batch
                    .upstream_model
                    .strip_prefix("models/")
                    .unwrap_or(&batch.upstream_model)
        })
    {
        context.insert("reported_model".into(), json!(model));
    }
    let mut settlement = Settlement::from_context("completed", &context, true);
    settlement.cost_pending = cost_pending;
    Some(settlement)
}

fn batch_items(job: &Value) -> Option<&Vec<Value>> {
    ["/response", "/metadata/output"]
        .into_iter()
        .filter_map(|path| job.pointer(path))
        .find_map(|output| {
            ["inlinedResponses", "inlinedEmbedContentResponses"]
                .into_iter()
                .filter_map(|field| output.get(field))
                .find_map(|items| {
                    items
                        .as_array()
                        .or_else(|| items["inlinedResponses"].as_array())
                })
        })
}

fn redact_error(error: &mut Value) {
    *error = json!({"code":error["code"].as_i64().unwrap_or(13), "message":"上游批任务处理失败"});
}

fn target_for<'a>(gateway: &'a Gateway, key: &'a Key, batch: &Batch) -> Option<&'a Target> {
    if !key.allows_model(&batch.model) {
        return None;
    }
    gateway
        .models
        .get(&batch.model)?
        .targets_for(batch.endpoint, key)
        .map(|(target, _)| target)
        .find(|target| {
            target.provider_name == batch.provider
                && target.upstream_model == batch.upstream_model
                && target
                    .provider
                    .gemini_batch_gateway()
                    .is_some_and(|provider| {
                        provider
                            .batch_scope()
                            .is_ok_and(|scope| scope == batch.scope)
                    })
        })
}

fn owner(key: &Key) -> String {
    key.usage_owner().invocation_scope("gemini-batches")
}

fn valid_id(value: &str) -> bool {
    value.len() == 26
        && value
            .bytes()
            .all(|byte| b"0123456789ABCDEFGHJKMNPQRSTVWXYZ".contains(&byte))
}

fn upstream_name(job: &Value) -> Option<&str> {
    let name = job["name"].as_str()?;
    let id = name.strip_prefix("batches/")?;
    (!id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')))
    .then_some(name)
}

fn invalid(message: &str) -> HttpResult {
    PROTOCOL.error(StatusCode::BAD_REQUEST, "invalid_request", message)
}

fn not_found() -> HttpResult {
    PROTOCOL.error(StatusCode::NOT_FOUND, "not_found", "批任务或模型不可用")
}

fn storage_error() -> HttpResult {
    PROTOCOL.error(
        StatusCode::SERVICE_UNAVAILABLE,
        "usage_unavailable",
        "批任务持久化暂不可用",
    )
}

fn malformed() -> HttpResult {
    PROTOCOL.error(
        StatusCode::BAD_GATEWAY,
        "invalid_upstream_response",
        "上游批任务响应无效",
    )
}

fn driver_error(error: AiError) -> HttpResult {
    match error {
        AiError::Timeout => PROTOCOL.error(
            StatusCode::GATEWAY_TIMEOUT,
            "upstream_timeout",
            "上游批任务请求超时",
        ),
        _ => malformed(),
    }
}

fn upstream_status(status: StatusCode) -> HttpResult {
    if status.is_client_error() || status.is_server_error() {
        PROTOCOL.error(status, "upstream_error", "上游未能处理批任务请求")
    } else {
        malformed()
    }
}

fn limited(seconds: u64, code: &str) -> HttpResult {
    PROTOCOL
        .error(StatusCode::TOO_MANY_REQUESTS, code, "批任务超出可用额度")
        .map(|response| {
            response.header(http::header::RETRY_AFTER, http::HeaderValue::from(seconds))
        })
}
