use openrust::http::{HttpFuture, Method, client::Response};
use serde_json::{Value, json};

use super::{GeminiTextProvider, validated_model_id};
use crate::{
    AiError, AiResult, GeminiCacheGateway, GeminiCacheRequest, gemini_batches::valid_cursor,
    gemini_caches::valid_name, text::transport_error,
};

impl GeminiCacheGateway for GeminiTextProvider {
    fn cache_scope(&self) -> AiResult<String> {
        self.0.request_namespace("gemini-caches")
    }

    fn send_cache<'a>(
        &'a self,
        operation: GeminiCacheRequest<'a>,
    ) -> HttpFuture<'a, AiResult<Response>> {
        Box::pin(async move {
            let request = self.0.request(None)?;
            let response = match operation {
                GeminiCacheRequest::Create { model, content } => {
                    let model = validated_model_id(model)?;
                    let mut body = content.as_object().ok_or(AiError::InvalidPrompt)?.clone();
                    if body.contains_key("name")
                        || body.get("model").is_some_and(|field| {
                            field
                                .as_str()
                                .and_then(|model| validated_model_id(model).ok())
                                != Some(model)
                        })
                    {
                        return Err(AiError::InvalidPrompt);
                    }
                    let expires = [
                        body.get("ttl"),
                        body.get("expireTime"),
                        body.get("expire_time"),
                    ];
                    if expires.iter().flatten().count() > 1
                        || expires
                            .into_iter()
                            .flatten()
                            .any(|value| value.as_str().is_none_or(str::is_empty))
                    {
                        return Err(AiError::InvalidPrompt);
                    }
                    body.insert("model".into(), format!("models/{model}").into());
                    request.post("cachedContents", &body).await
                }
                GeminiCacheRequest::Get { name } => request.get(resource_path(name)?).await,
                GeminiCacheRequest::List { limit, cursor } => {
                    let mut query = Vec::new();
                    if let Some(limit) = limit {
                        if limit == 0 {
                            return Err(AiError::InvalidPrompt);
                        }
                        query.push(("pageSize".into(), limit.to_string()));
                    }
                    if let Some(cursor) = cursor {
                        if !valid_cursor(cursor) {
                            return Err(AiError::InvalidPrompt);
                        }
                        query.push(("pageToken".into(), cursor.into()));
                    }
                    request
                        .with_query_parameters(query)
                        .get("cachedContents")
                        .await
                }
                GeminiCacheRequest::Update { name, expiration } => {
                    let (field, value) = expiration.field();
                    if value.trim().is_empty() {
                        return Err(AiError::InvalidPrompt);
                    }
                    request
                        .with_query_parameters([("updateMask".into(), field.into())])
                        .patch(resource_path(name)?, &json!({(field):value}))
                        .await
                }
                GeminiCacheRequest::Delete { name } => {
                    request.send(Method::DELETE, resource_path(name)?).await
                }
            };
            response.map_err(transport_error)
        })
    }
}

fn resource_path(name: &str) -> AiResult<&str> {
    valid_name(name)
        .then_some(name)
        .ok_or(AiError::InvalidPrompt)
}

pub(super) fn normalize_reference(body: &mut Value) -> AiResult<()> {
    let data = body
        .as_object_mut()
        .ok_or(AiError::InvalidProviderOptions)?;
    let snake = data
        .remove("cached_content")
        .filter(|value| !value.is_null());
    if data.get("cachedContent").is_some_and(Value::is_null) {
        data.remove("cachedContent");
    }
    match (data.get("cachedContent"), snake) {
        (Some(_), Some(_)) => Err(AiError::InvalidProviderOptions),
        (Some(value), None) if value.as_str().is_none_or(|value| !valid_name(value)) => {
            Err(AiError::InvalidProviderOptions)
        }
        (None, Some(value)) => {
            if value.as_str().is_none_or(|value| !valid_name(value)) {
                return Err(AiError::InvalidProviderOptions);
            }
            data.insert("cachedContent".into(), value);
            Ok(())
        }
        _ => Ok(()),
    }
}
