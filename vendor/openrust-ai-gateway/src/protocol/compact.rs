use std::sync::Arc;

use openrust::{encryption::Encrypter, support::facades::Crypt};
use serde::{Deserialize, Serialize};
use serde_json::Map;

use super::{
    Call, Facade as _, HttpFuture, HttpResult, Key, Protocol, Request, Response, StatusCode,
    Target, TextEndpoint, Value, client, downstream_headers, relay, rename_model,
    usage_storage_failure,
};

const PREFIX: &str = "openrust-compact-v1.";

pub(super) fn route(request: &mut Request) -> HttpFuture<'_, HttpResult> {
    Box::pin(relay(request, TextEndpoint::CompactResponses, None))
}

pub(super) fn has_input(data: &Map<String, Value>) -> bool {
    data.get("input")
        .and_then(Value::as_array)
        .is_some_and(|items| items.iter().any(is_compaction))
}

fn is_compaction(item: &Value) -> bool {
    item["type"] == "compaction"
}

pub(super) struct Context {
    crypt: Arc<Encrypter>,
    scope: String,
}

#[derive(Serialize, Deserialize)]
struct Payload {
    scope: String,
    id: Value,
    token: String,
}

impl Context {
    pub(super) fn new(key: &Key, name: &str, target: &Target) -> Option<Self> {
        let namespace = target
            .provider
            .step_text_gateway()?
            .approval_replay_scope()
            .ok()??;
        let scope = crate::session_affinity::key(&[
            "response-compaction",
            &key.relay_scope(name, target),
            &namespace,
        ]);
        Some(Self {
            scope,
            crypt: Crypt::get_facade_root().ok()?,
        })
    }

    pub(super) fn restore(&self, body: &mut Map<String, Value>) -> bool {
        body.get_mut("input")
            .and_then(Value::as_array_mut)
            .is_some_and(|items| {
                items
                    .iter_mut()
                    .filter(|item| is_compaction(item))
                    .all(|item| {
                        let Some(token) = self.open(item) else {
                            return false;
                        };
                        item["encrypted_content"] = token.into();
                        true
                    })
            })
    }

    fn open(&self, item: &Value) -> Option<String> {
        let token = item["encrypted_content"].as_str()?.strip_prefix(PREFIX)?;
        let data = self.crypt.decrypt_string(token).ok()?;
        let payload: Payload = serde_json::from_slice(&data).ok()?;
        (payload.scope == self.scope && payload.id == item["id"]).then_some(payload.token)
    }

    fn wrap(&self, item: &Value) -> Option<String> {
        let token = item["encrypted_content"].as_str()?;
        if token.trim().is_empty() {
            return None;
        }
        let data = serde_json::to_vec(&Payload {
            scope: self.scope.clone(),
            id: item["id"].clone(),
            token: token.into(),
        })
        .ok()?;
        let token = self.crypt.encrypt_string(&data).ok()?;
        Some(format!("{PREFIX}{token}"))
    }

    fn protect(&self, response: &mut Value) -> bool {
        if response["object"] != "response.compaction" {
            return false;
        }
        let Some(output) = response["output"].as_array_mut() else {
            return false;
        };
        let mut protected = false;
        for item in output.iter_mut().filter(|item| is_compaction(item)) {
            let Some(token) = self.wrap(item) else {
                return false;
            };
            item["encrypted_content"] = token.into();
            protected = true;
        }
        protected
    }
}

pub(super) async fn respond(
    upstream: client::Response,
    model: &str,
    context: Context,
    mut call: Call,
) -> HttpResult {
    let status = upstream.status();
    let headers = downstream_headers(upstream.headers());
    call.release().await;
    let mut data = upstream.json::<Value>().unwrap_or(Value::Null);
    call.observe_usage(&data);
    call.observe_final_cost(&data);
    if !context.protect(&mut data) {
        call.outcome("invalid_upstream_response");
        if !call.settle().await {
            return usage_storage_failure(Protocol::OpenAi, &mut call).response;
        }
        return Protocol::OpenAi.error(
            StatusCode::BAD_GATEWAY,
            "invalid_upstream_response",
            "上游未返回有效的压缩上下文",
        );
    }
    if !call.complete().await {
        return usage_storage_failure(Protocol::OpenAi, &mut call).response;
    }
    rename_model(&mut data, model);
    Response::json(status, data).map(|response| response.with_headers(headers))
}
