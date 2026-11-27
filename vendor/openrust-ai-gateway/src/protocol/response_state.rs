use std::sync::Arc;

use openrust::{encryption::Encrypter, support::facades::Crypt};
use serde::{Deserialize, Serialize};
use serde_json::Map;

use super::{
    Admission, AiError, Facade as _, Gateway, HttpFuture, HttpResult, Key, Protocol, Request,
    Response, StatusCode, Target, TextEndpoint, Value, account_state_unavailable, admit, authorize,
    compact, downstream_headers, rename_model,
};

const PREFIX: &str = "resp_or_";
const PROTOCOL: Protocol = Protocol::OpenAi;

#[derive(Serialize, Deserialize)]
struct Binding {
    model: String,
    upstream: String,
    scope: String,
}

pub(super) struct State {
    pub(super) store: bool,
    previous: Option<String>,
}

pub(super) struct Context {
    crypt: Arc<Encrypter>,
    model: String,
    scope: String,
    store: bool,
    previous: Option<String>,
    issued: Option<(String, String)>,
}

pub(super) fn prepare(
    data: &Map<String, Value>,
    enabled: bool,
) -> Result<Option<State>, &'static str> {
    let store = match data.get("store") {
        None | Some(Value::Null | Value::Bool(false)) => false,
        Some(Value::Bool(true)) => true,
        _ => return Err("store 须为布尔值"),
    };
    let previous = match data.get("previous_response_id") {
        None | Some(Value::Null) => None,
        Some(Value::String(id)) if token(id).is_some() => Some(id.clone()),
        _ => return Err("previous_response_id 须使用当前网关签发的标识"),
    };
    if !store && previous.is_none() {
        return Ok(None);
    }
    if !enabled {
        return Err("此公开模型未启用 Responses 持久状态");
    }
    Ok(Some(State { store, previous }))
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
            store: self.store,
            previous: self.previous.clone(),
            issued: None,
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
    Box::pin(resource(request, false))
}

pub(super) fn delete(request: &mut Request) -> HttpFuture<'_, HttpResult> {
    Box::pin(resource(request, true))
}

async fn resource(request: &mut Request, delete: bool) -> HttpResult {
    let gateway = Gateway::get_facade_root()?;
    let key = match authorize(&gateway, PROTOCOL.token(request), PROTOCOL).await {
        Ok(key) => key,
        Err(response) => return response,
    };
    if request.uri().query().is_some_and(|query| !query.is_empty()) {
        return PROTOCOL.error(
            StatusCode::BAD_REQUEST,
            "unsupported_parameter",
            "持久响应读取仅支持 JSON",
        );
    }
    let Some((id, binding)) = request.route("response_id").and_then(|id| {
        let crypt = Crypt::get_facade_root().ok()?;
        Some((id, read(&crypt, id)?))
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
    let result = target
        .provider
        .response_resource(&binding.upstream, delete)
        .await;
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
    if data["id"].as_str() != Some(binding.upstream.as_str())
        || !compact::protect_output(&mut data, context.as_mut(), true)
    {
        return PROTOCOL.error(
            StatusCode::BAD_GATEWAY,
            "invalid_upstream_response",
            "上游未返回匹配的持久响应",
        );
    }
    data["id"] = id.into();
    if let Some(previous) = data.get_mut("previous_response_id") {
        *previous = Value::Null;
    }
    rename_model(&mut data, &binding.model);
    Response::json(reply.status(), data)
        .map(|response| response.with_headers(downstream_headers(reply.headers())))
}

fn not_found() -> HttpResult {
    PROTOCOL.error(StatusCode::NOT_FOUND, "response_not_found", "响应不可用")
}
