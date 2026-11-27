use openrust::http::client;
use openrust_ai::{GeminiCacheExpiration, GeminiCacheRequest, GeminiCachedContent};
use serde_json::Map;

use super::*;
use crate::usage::{
    UsageStore,
    gemini_caches::{CREATE, Cache, DELETE, UPDATE},
};

const PROTOCOL: Protocol = Protocol::Gemini;
const GENERATION: [TextEndpoint; 2] = [
    TextEndpoint::GeminiGenerateContent,
    TextEndpoint::GeminiStreamGenerateContent,
];

pub(super) fn routes(router: &mut Router) -> Result<(), RoutingError> {
    router.post("/cachedContents", create_route)?;
    router.get("/cachedContents", list_route)?;
    router.get("/cachedContents/{id}", get_route)?;
    router.patch("/cachedContents/{id}", update_route)?;
    router.delete("/cachedContents/{id}", delete_route)?;
    Ok(())
}

fn create_route(request: &mut Request) -> HttpFuture<'_, HttpResult> {
    Box::pin(create(request))
}

fn list_route(request: &mut Request) -> HttpFuture<'_, HttpResult> {
    Box::pin(list(request))
}

fn get_route(request: &mut Request) -> HttpFuture<'_, HttpResult> {
    Box::pin(manage(request, Action::Get))
}

fn update_route(request: &mut Request) -> HttpFuture<'_, HttpResult> {
    Box::pin(manage(request, Action::Update))
}

fn delete_route(request: &mut Request) -> HttpFuture<'_, HttpResult> {
    Box::pin(manage(request, Action::Delete))
}

async fn create(request: &mut Request) -> HttpResult {
    let gateway = Gateway::get_facade_root()?;
    let key = match authorize(&gateway, PROTOCOL.token(request), PROTOCOL).await {
        Ok(key) => key,
        Err(error) => return error,
    };
    let Some(store) = &gateway.usage_store else {
        return unavailable();
    };
    let mut data = match body(request).await {
        Ok(data) => data,
        Err(error) => return error,
    };
    let Some(model) = data
        .get("model")
        .and_then(Value::as_str)
        .and_then(|id| id.strip_prefix("models/"))
    else {
        return invalid("Gemini 缓存需要公开模型名");
    };
    if resources::gemini_file_references(&data)
        || !resources::gemini_function_tools(data.get("tools"))
        || data.keys().any(|key| {
            !matches!(
                key.as_str(),
                "model"
                    | "ttl"
                    | "expireTime"
                    | "expire_time"
                    | "displayName"
                    | "display_name"
                    | "contents"
                    | "systemInstruction"
                    | "system_instruction"
                    | "tools"
                    | "toolConfig"
                    | "tool_config"
            )
        })
    {
        return invalid("Gemini 缓存仅接收内联内容及客户端函数定义");
    }
    let expirations = [
        data.get("ttl"),
        data.get("expireTime"),
        data.get("expire_time"),
    ];
    if expirations.iter().flatten().count() > 1
        || expirations
            .into_iter()
            .flatten()
            .any(|value| value.as_str().is_none_or(|value| value.trim().is_empty()))
    {
        return invalid("Gemini 缓存过期时间无效");
    }
    let model = model.to_owned();
    data.remove("model");
    let Some(target) = cache_targets(&gateway, &key, &model).next() else {
        return not_found();
    };
    let Some(provider) = target.provider.gemini_cache_gateway() else {
        return not_found();
    };
    let Ok(scope) = provider.cache_scope() else {
        return unavailable();
    };
    if let Some(response) = admit(&key, PROTOCOL).await? {
        return Ok(response);
    }
    let content = Value::Object(data);
    if !can_change_expiry(&gateway, &key, target) {
        return unbounded();
    }
    let mut cache = Cache {
        id: Str::ulid().to_string(),
        owner: owner(&key),
        provider: target.provider_name.clone(),
        scope,
        model,
        upstream_model: target.upstream_model.clone(),
        upstream_name: String::new(),
    };
    if store.reserve_gemini_cache(&cache).await.is_err() {
        return unavailable();
    }
    let upstream = match send(
        &gateway,
        target,
        GeminiCacheRequest::Create {
            model: &target.upstream_model,
            content: &content,
        },
        Some((&key, &cache, CREATE)),
    )
    .await
    {
        Ok(response) => response,
        Err(error) => return error,
    };
    if !upstream.successful() {
        if upstream.status().is_client_error()
            && store
                .discard_gemini_cache_reservation(&cache.id)
                .await
                .is_err()
        {
            return unavailable();
        }
        return upstream_error(upstream.status());
    }
    let Some(resource) = decode(&upstream, None) else {
        return malformed();
    };
    cache.upstream_name = resource.name.clone();
    if store.attach_gemini_cache(&cache).await.is_err() {
        return unavailable();
    }
    json_response(upstream.status(), present(&cache, resource))
}

enum Action {
    Get,
    Update,
    Delete,
}

async fn manage(request: &mut Request, action: Action) -> HttpResult {
    let gateway = Gateway::get_facade_root()?;
    let key = match authorize(&gateway, PROTOCOL.token(request), PROTOCOL).await {
        Ok(key) => key,
        Err(error) => return error,
    };
    let Some(id) = request.route("id").filter(|id| valid_id(id)) else {
        return not_found();
    };
    let Some(store) = &gateway.usage_store else {
        return unavailable();
    };
    let cache = match load(store, &key, id).await {
        Ok(cache) => cache,
        Err(error) => return error,
    };
    let Some(target) =
        cache_targets(&gateway, &key, &cache.model).find(|target| cache_matches(&cache, target))
    else {
        return not_found();
    };
    if let Some(response) = admit(&key, PROTOCOL).await? {
        return Ok(response);
    }
    let name = &cache.upstream_name;
    let upstream = match action {
        Action::Get => send(&gateway, target, GeminiCacheRequest::Get { name }, None).await,
        Action::Update => {
            if !can_change_expiry(&gateway, &key, target) {
                return unbounded();
            }
            let data = match body(request).await {
                Ok(data) => data,
                Err(error) => return error,
            };
            let Some((field, value)) = data.iter().next().filter(|_| data.len() == 1) else {
                return invalid("Gemini 缓存只更新过期时间");
            };
            let Some(value) = value.as_str().filter(|value| !value.trim().is_empty()) else {
                return invalid("Gemini 缓存过期时间无效");
            };
            let expiration = match field.as_str() {
                "ttl" => GeminiCacheExpiration::Ttl(value),
                "expireTime" | "expire_time" => GeminiCacheExpiration::ExpireTime(value),
                _ => return invalid("Gemini 缓存只更新过期时间"),
            };
            if !matches!(request.query(Some("updateMask")), Ok(None))
                && !matches!(request.query(Some("updateMask")), Ok(Some(Value::String(mask))) if mask == *field
                    || (matches!(expiration, GeminiCacheExpiration::ExpireTime(_)) && matches!(mask.as_str(),"expireTime" | "expire_time")))
            {
                return invalid("Gemini 缓存更新掩码不匹配");
            }
            send(
                &gateway,
                target,
                GeminiCacheRequest::Update { name, expiration },
                Some((&key, &cache, UPDATE)),
            )
            .await
        }
        Action::Delete => {
            send(
                &gateway,
                target,
                GeminiCacheRequest::Delete { name },
                Some((&key, &cache, DELETE)),
            )
            .await
        }
    };
    let upstream = match upstream {
        Ok(response) => response,
        Err(error) => return error,
    };
    if matches!(action, Action::Delete) && (upstream.successful() || upstream.not_found()) {
        if store.delete_gemini_cache(&cache).await.is_err() {
            return unavailable();
        }
        return json_response(StatusCode::OK, json!({}));
    }
    if !upstream.successful() {
        return upstream_error(upstream.status());
    }
    match decode(&upstream, Some(name)) {
        Some(resource) => json_response(upstream.status(), present(&cache, resource)),
        None => malformed(),
    }
}

async fn list(request: &mut Request) -> HttpResult {
    let gateway = Gateway::get_facade_root()?;
    let key = match authorize(&gateway, PROTOCOL.token(request), PROTOCOL).await {
        Ok(key) => key,
        Err(error) => return error,
    };
    let Some(store) = &gateway.usage_store else {
        return unavailable();
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
    let mut caches = match store
        .gemini_cache_page(&owner(&key), cursor.as_deref(), limit + 1)
        .await
    {
        Ok(caches) => caches,
        Err(_) => return unavailable(),
    };
    let next = if caches.len() as u64 > limit {
        caches.pop();
        caches.last().map(|cache| cache.id.clone())
    } else {
        None
    };
    let mut items = Vec::new();
    for cache in caches {
        let Some(target) = cache_targets(&gateway, &key, &cache.model)
            .find(|target| cache_matches(&cache, target))
        else {
            continue;
        };
        let name = &cache.upstream_name;
        let upstream = match send(&gateway, target, GeminiCacheRequest::Get { name }, None).await {
            Ok(response) => response,
            Err(error) => return error,
        };
        if upstream.not_found() {
            continue;
        }
        if !upstream.successful() {
            return upstream_error(upstream.status());
        }
        let Some(resource) = decode(&upstream, Some(name)) else {
            return malformed();
        };
        items.push(present(&cache, resource));
    }
    let mut result = json!({"cachedContents":items});
    if let Some(next) = next {
        result["nextPageToken"] = next.into();
    }
    json_response(StatusCode::OK, result)
}

pub(super) async fn resolve(
    gateway: &Gateway,
    key: &Key,
    model: &str,
    endpoint: TextEndpoint,
    data: &mut Map<String, Value>,
) -> Result<Option<Cache>, HttpResult> {
    let camel = data
        .remove("cachedContent")
        .filter(|value| !value.is_null());
    let snake = data
        .remove("cached_content")
        .filter(|value| !value.is_null());
    if camel.is_none() && snake.is_none() {
        return Ok(None);
    }
    if !(GENERATION.contains(&endpoint) || endpoint == TextEndpoint::GeminiCountTokens)
        || (camel.is_some() && snake.is_some())
    {
        return Err(invalid("此 Gemini 端点不支持缓存引用"));
    }
    let value = camel.or(snake).expect("已经核对缓存参数");
    let Some(id) = value
        .as_str()
        .and_then(|value| value.strip_prefix("cachedContents/"))
        .filter(|id| valid_id(id))
    else {
        return Err(invalid("请使用网关返回的 Gemini 缓存名"));
    };
    let Some(store) = &gateway.usage_store else {
        return Err(unavailable());
    };
    let cache = load(store, key, id).await?;
    if cache.model != model
        || !cache_targets(gateway, key, model).any(|target| cache_matches(&cache, target))
    {
        return Err(not_found());
    }
    Ok(Some(cache))
}

pub(super) fn cache_matches(cache: &Cache, target: &Target) -> bool {
    target.provider_name == cache.provider
        && target.upstream_model == cache.upstream_model
        && target
            .provider
            .gemini_cache_gateway()
            .is_some_and(|provider| {
                provider
                    .cache_scope()
                    .is_ok_and(|scope| scope == cache.scope)
            })
}

fn cache_targets<'a>(
    gateway: &'a Gateway,
    key: &'a Key,
    name: &str,
) -> impl Iterator<Item = &'a Target> {
    gateway
        .models
        .get(name)
        .filter(|model| model.gemini_cached_contents && key.allows_model(name))
        .into_iter()
        .flat_map(|model| {
            GENERATION
                .into_iter()
                .flat_map(|endpoint| model.targets_for(endpoint, key))
        })
        .map(|(target, _)| target)
        .filter(|target| target.provider.gemini_cache_gateway().is_some())
}

fn can_change_expiry(gateway: &Gateway, key: &Key, target: &Target) -> bool {
    // 没有可信的创建及存储费用上界，沿用未知费用的硬预算策略。
    gateway
        .budget_reservations(
            key,
            target,
            TextEndpoint::GeminiGenerateContent,
            &mut Value::Null,
            None,
        )
        .is_ok()
}

async fn send(
    gateway: &Gateway,
    target: &Target,
    operation: GeminiCacheRequest<'_>,
    usage: Option<(&Key, &Cache, &'static str)>,
) -> Result<client::Response, HttpResult> {
    let cache_store = gateway.account_cache_store.as_deref();
    let lease = if let Some(account) = &target.account {
        if let Some(cooldown) = &account.cooldown
            && !matches!(cooldown.remaining(cache_store).await, Ok(None))
        {
            return Err(account_state_unavailable(PROTOCOL));
        }
        match account.acquire(cache_store).await {
            Ok(Admission::Allowed(lease)) => lease,
            _ => return Err(account_state_unavailable(PROTOCOL)),
        }
    } else {
        None
    };
    let provider = target
        .provider
        .gemini_cache_gateway()
        .expect("已核对 Gemini 缓存能力");
    let mut lease = lease;
    let mut call = if let Some((key, cache, action)) = usage {
        let mut call = gateway
            .cache_usage(key, cache, target, action, lease.take())
            .await
            .map_err(|_| unavailable())?;
        if !call.begin_dispatch().await {
            return Err(unavailable());
        }
        Some(call)
    } else {
        None
    };
    let upstream = provider.send_cache(operation).await;
    if let Some(call) = &mut call {
        match &upstream {
            Ok(response) => {
                call.status(response.status());
                call.observe_headers(response.headers());
                if response.successful() {
                    if !matches!(usage, Some((_, _, DELETE)))
                        && let Ok(data) = response.json::<Value>()
                    {
                        call.observe_cache_resource(&data, matches!(usage, Some((_, _, CREATE))));
                    }
                    call.outcome("completed");
                } else {
                    call.outcome("upstream_error");
                }
            }
            Err(_) => call.outcome("transport_error"),
        }
        let settled = call.settle().await;
        call.release().await;
        if !settled {
            return Err(unavailable());
        }
    }
    upstream.map_err(|error| match error {
        AiError::InvalidPrompt | AiError::InvalidProviderOptions => invalid("Gemini 缓存参数无效"),
        AiError::Timeout => PROTOCOL.error(
            StatusCode::GATEWAY_TIMEOUT,
            "upstream_timeout",
            "上游 Gemini 缓存请求超时",
        ),
        _ => malformed(),
    })
}

async fn load(store: &UsageStore, key: &Key, id: &str) -> Result<Cache, HttpResult> {
    match store.gemini_cache(&owner(key), id).await {
        Ok(Some(cache)) => Ok(cache),
        Ok(None) => Err(not_found()),
        Err(_) => Err(unavailable()),
    }
}

async fn body(request: &mut Request) -> Result<Map<String, Value>, HttpResult> {
    let bytes = match request.body_bytes().await {
        Ok(bytes) => bytes,
        Err(HttpError::PayloadTooLarge) => return Err(super::request_too_large(PROTOCOL)),
        Err(_) => return Err(invalid("无法读取 Gemini 缓存请求")),
    };
    serde_json::from_slice(&bytes).map_err(|_| invalid("Gemini 缓存请求须为 JSON 对象"))
}

fn decode(response: &client::Response, expected: Option<&str>) -> Option<GeminiCachedContent> {
    let resource: GeminiCachedContent = response.json().ok()?;
    let id = resource.name.strip_prefix("cachedContents/")?;
    if id.is_empty()
        || id.len() > 128
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        || expected.is_some_and(|expected| expected != resource.name)
        || resource.expire_time.is_empty()
    {
        return None;
    }
    Some(resource)
}

fn present(cache: &Cache, mut resource: GeminiCachedContent) -> GeminiCachedContent {
    resource.name = format!("cachedContents/{}", cache.id);
    resource.model = format!("models/{}", cache.model);
    resource
}

fn owner(key: &Key) -> String {
    key.usage_owner().invocation_scope("gemini-caches")
}

fn valid_id(id: &str) -> bool {
    !id.bytes().any(|byte| byte.is_ascii_lowercase()) && Str::is_ulid(id)
}

fn json_response(status: StatusCode, data: impl serde::Serialize) -> HttpResult {
    Response::json(status, data).map(|response| {
        response.header(
            header::CACHE_CONTROL,
            HeaderValue::from_static("private, no-store"),
        )
    })
}

fn invalid(message: &str) -> HttpResult {
    PROTOCOL.error(StatusCode::BAD_REQUEST, "invalid_request", message)
}

fn unavailable() -> HttpResult {
    PROTOCOL.error(
        StatusCode::SERVICE_UNAVAILABLE,
        "usage_unavailable",
        "Gemini 缓存持久化暂不可用",
    )
}

pub(super) fn not_found() -> HttpResult {
    PROTOCOL.error(
        StatusCode::NOT_FOUND,
        "not_found",
        "Gemini 缓存或模型不可用",
    )
}

fn malformed() -> HttpResult {
    PROTOCOL.error(
        StatusCode::BAD_GATEWAY,
        "invalid_upstream_response",
        "上游 Gemini 缓存响应无效",
    )
}

fn unbounded() -> HttpResult {
    PROTOCOL.error(
        StatusCode::BAD_REQUEST,
        "budget_unbounded",
        "无法确认 Gemini 缓存存储费用上限",
    )
}

fn upstream_error(status: StatusCode) -> HttpResult {
    if status.is_client_error() || status.is_server_error() {
        PROTOCOL.error(status, "upstream_error", "上游未能处理 Gemini 缓存请求")
    } else {
        malformed()
    }
}
