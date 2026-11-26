use super::{Gateway, HttpFuture, HttpResult, Protocol, Request, Response, StatusCode, authorize};
use http::{HeaderValue, header};
use openrust::support::facades::Facade;
use serde_json::{Value, json};

use crate::usage::UsageStore;

pub(super) fn show(request: &mut Request) -> HttpFuture<'_, HttpResult> {
    Box::pin(async move {
        let protocol = if request.has_header("x-api-key") {
            Protocol::Anthropic
        } else {
            Protocol::OpenAi
        };
        let gateway = Gateway::get_facade_root()?;
        let key = match authorize(&gateway, protocol.token(request), protocol).await {
            Ok(key) => key,
            Err(response) => return response,
        };
        let Some(store) = &gateway.usage_store else {
            return protocol.error(
                StatusCode::NOT_FOUND,
                "usage_disabled",
                "网关未启用用量账本",
            );
        };
        let days = match request.query("days") {
            Ok(None) => Some(7),
            Ok(Some(Value::String(days))) => days.parse::<u8>().ok(),
            Ok(Some(_)) | Err(_) => None,
        };
        let Some(days) = days.filter(|days| (1..=90).contains(days)) else {
            return protocol.error(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                "days 必须为 1 至 90 的整数",
            );
        };
        let Ok((total, daily)) = store.key_summary(&key.usage_owner(), days).await else {
            return protocol.error(
                StatusCode::SERVICE_UNAVAILABLE,
                "usage_unavailable",
                "网关用量存储暂不可用",
            );
        };
        let quota = key.output_quota();
        let mode = if key.requests_per_minute.is_some() || quota.is_some() {
            "quota_limited"
        } else {
            "unrestricted"
        };
        Response::json(
            StatusCode::OK,
            json!({
                "mode": mode,
                "isValid": true,
                "usage": {"today": UsageStore::today(&daily), "total": total},
                "daily_usage": daily,
                "requests_per_minute": key.requests_per_minute,
                "group_output_quota": quota.map(|quota| json!({
                    "output_tokens_per_day": quota.output_tokens_per_day,
                    "max_output_tokens": quota.max_output_tokens,
                })),
            }),
        )
        .map(|response| {
            response.header(header::CACHE_CONTROL, HeaderValue::from_static("no-store"))
        })
    })
}
