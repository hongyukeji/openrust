use openrust::http::{Response, StatusCode};
use openrust_ai::TextEndpoint;
use serde_json::{Value, json};

use super::{Call, HttpResult, Protocol, Target, account_state_unavailable, request_upstream};

pub(super) async fn relay(
    target: &Target,
    mut calls: Vec<Call>,
    parts: Vec<Value>,
    public_model: String,
    protocol: Protocol,
    cache_store: Option<&str>,
) -> HttpResult {
    let mut embeddings = Vec::with_capacity(parts.len());
    let mut total = Some(0_u64);
    for (index, (part, call)) in parts.into_iter().zip(&mut calls).enumerate() {
        let upstream =
            request_upstream(target, TextEndpoint::Embeddings, part, None, protocol, call).await;
        let upstream = match upstream {
            Ok(upstream) => upstream,
            Err(failure) => {
                if !Call::finish_batch(&mut calls, false).await {
                    return settlement_error(protocol);
                }
                let cooldown = target
                    .account
                    .as_ref()
                    .and_then(|account| account.cooldown.as_ref());
                if let (Some(cooldown), Some(until)) = (cooldown, failure.pause_until)
                    && cooldown.pause(cache_store, until).await.is_err()
                {
                    return account_state_unavailable(protocol);
                }
                return failure.response;
            }
        };
        let item = upstream.json::<Value>().ok().and_then(|mut response| {
            let items = response.get_mut("data")?.as_array_mut()?;
            if items.len() != 1
                || !items[0]["embedding"].is_array() && !items[0]["embedding"].is_string()
            {
                return None;
            }
            let mut item = items.pop()?;
            item["index"] = json!(index);
            Some((response, item))
        });
        let Some((response, item)) = item else {
            call.outcome("invalid_upstream_response");
            if !Call::finish_batch(&mut calls, false).await {
                return settlement_error(protocol);
            }
            return protocol.error(
                StatusCode::BAD_GATEWAY,
                "invalid_upstream_response",
                "上游未返回单条有效向量",
            );
        };

        call.observe_usage(&response);
        // 整批交付前只确认上游成本证据，Drop 也沿用这一处置。
        call.outcome("batch_aborted");
        if !call.persist_observation().await {
            return settlement_error(protocol);
        }
        total = total
            .zip(response["usage"]["prompt_tokens"].as_u64())
            .and_then(|(sum, tokens)| sum.checked_add(tokens));
        embeddings.push(item);
    }
    let mut response = json!({"object":"list","data":embeddings,"model":public_model});
    if let Some(tokens) = total {
        response["usage"] = json!({"prompt_tokens":tokens,"total_tokens":tokens});
    }
    if !Call::finish_batch(&mut calls, true).await {
        return settlement_error(protocol);
    }
    Response::json(StatusCode::OK, response)
}

fn settlement_error(protocol: Protocol) -> HttpResult {
    protocol.error(
        StatusCode::SERVICE_UNAVAILABLE,
        "usage_unavailable",
        "AI 网关无法持久化批量向量用量",
    )
}
