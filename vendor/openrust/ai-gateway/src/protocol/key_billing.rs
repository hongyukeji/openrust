use http::{HeaderValue, header};
use openrust::support::{
    DateTimezone,
    facades::{Date, Facade},
};
use serde_json::json;

use super::{
    Gateway, HttpFuture, HttpResult, Protocol, Request, Response, StatusCode, authorize,
    catalog_protocol,
};

pub(super) fn show(request: &mut Request) -> HttpFuture<'_, HttpResult> {
    Box::pin(async move {
        let gateway = Gateway::get_facade_root()?;
        let protocol = Protocol::OpenAi;
        let key =
            match authorize(&gateway, catalog_protocol(request).token(request), protocol).await {
                Ok(key) => key,
                Err(response) => return response,
            };
        if !gateway.billing_enabled() {
            return protocol.error(StatusCode::NOT_FOUND, "billing_disabled", "网关未启用计费");
        }
        let Some(group) = key.group_name() else {
            return protocol.error(
                StatusCode::FORBIDDEN,
                "key_group_required",
                "网关 Key 未分配授权分组",
            );
        };
        let observed = Date::now(Some(DateTimezone::UTC))
            .and_then(|date| date.format(r"Y-m-d\TH:i:s\Z"))
            .map_err(|source| openrust::http::HttpError::Internal {
                source: Box::new(source),
            })?;
        // OpenRust 的下游费率是发布模型的最终基础费率。
        let mut result = json!({
            "object": "sub2api.key_billing",
            "schema_version": 1,
            "billing_scope": "token",
            "group_rate_multiplier": 1,
            "resolved_rate_multiplier": 1,
            "peak_rate_enabled": false,
            "effective_rate_multiplier": 1,
            "observed_at": observed,
        });
        if request.uri().query().is_some_and(includes_group) {
            result["group_name"] = json!(group);
            result["group_description"] = json!("");
        }
        Response::json(StatusCode::OK, result).map(|response| {
            response.header(header::CACHE_CONTROL, HeaderValue::from_static("no-store"))
        })
    })
}

fn includes_group(query: &str) -> bool {
    url::form_urlencoded::parse(query.as_bytes()).any(|(key, value)| {
        key == "include" && value.split(',').any(|section| section.trim() == "group")
    })
}
