use std::time::Duration;

use http::{HeaderMap, HeaderValue, StatusCode, header::CONTENT_TYPE};
use openrust::{http::client::Response, support::Str};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{AiError, AiResult, embeddings::valid_vectors};

use super::{BedrockProvider, invoke::InvokeFailure};

fn is_titan(model: &str) -> bool {
    model
        .rsplit('/')
        .next()
        .is_some_and(|id| id.starts_with("amazon.titan-embed-text-v2:"))
}

pub(super) fn split_titan(body: &Value) -> Result<Option<Vec<Value>>, &'static str> {
    let Some(model) = body["model"].as_str().filter(|model| is_titan(model)) else {
        return Ok(None);
    };
    let Some(inputs) = body["input"].as_array().filter(|items| items.len() > 1) else {
        return Ok(None);
    };
    if inputs.len() > 2048 {
        return Err("Bedrock Titan 批量请求最多接收 2048 条文本");
    }
    let mut template = json!({
        "model": model,
        "dimensions": body["dimensions"],
        "encoding_format": body["encoding_format"],
        "input": inputs[0],
    });
    EmbeddingRequest::parse(&template)?;
    template["input"] = Value::Null;
    Ok(Some(
        inputs
            .iter()
            .map(|input| {
                let mut request = template.clone();
                request["input"] = input.clone();
                request
            })
            .collect(),
    ))
}

enum Input<'a> {
    Titan { text: &'a str, dimensions: usize },
    Cohere(Vec<&'a str>),
}

pub(super) struct EmbeddingRequest<'a> {
    model: &'a str,
    input: Input<'a>,
    base64: bool,
}

impl<'a> EmbeddingRequest<'a> {
    pub(super) fn parse(body: &'a Value) -> Result<Self, &'static str> {
        let model = body["model"].as_str().ok_or("必须指定 Bedrock 向量模型")?;
        let encoding = match &body["encoding_format"] {
            Value::Null => false,
            Value::String(value) if value == "float" => false,
            Value::String(value) if value == "base64" => true,
            _ => return Err("Bedrock 向量编码须为 float 或 base64"),
        };
        let texts = match &body["input"] {
            Value::String(text) => Some(vec![text.as_str()]),
            Value::Array(items) => items.iter().map(Value::as_str).collect(),
            _ => None,
        }
        .filter(|texts| !texts.is_empty() && texts.iter().all(|text| !text.trim().is_empty()))
        .ok_or("Bedrock 向量入口需要非空文本或文本数组")?;
        let input = if model.contains("cohere.embed-") {
            if !body["dimensions"].is_null() {
                return Err("此 Cohere 向量入口使用模型的原生维数");
            }
            Input::Cohere(texts)
        } else if is_titan(model) {
            if texts.len() != 1 {
                return Err("Bedrock Titan 向量入口每次接收一段文本");
            }
            let dimensions = match &body["dimensions"] {
                Value::Null => 1024,
                value => match value.as_u64() {
                    Some(256) => 256,
                    Some(512) => 512,
                    Some(1024) => 1024,
                    _ => return Err("Bedrock Titan V2 的向量维数须为 256、512 或 1024"),
                },
            };
            Input::Titan {
                text: texts[0],
                dimensions,
            }
        } else {
            return Err("此 Bedrock 模型没有可用的向量协议映射");
        };
        Ok(Self {
            model,
            input,
            base64: encoding,
        })
    }
}

impl BedrockProvider {
    pub(super) async fn relay_embeddings(&self, body: Value) -> AiResult<Response> {
        let request = EmbeddingRequest::parse(&body).map_err(|_| AiError::InvalidPrompt)?;
        let native = match &request.input {
            Input::Titan { text, dimensions } => json!({"inputText":text,"dimensions":dimensions}),
            Input::Cohere(texts) => json!({"texts":texts,"input_type":"search_document"}),
        };
        let output = match self
            .invoke_with_evidence(request.model, Duration::from_secs(30), native)
            .await
        {
            Ok(output) => output,
            Err(failure) => return error_response(failure),
        };
        let payload = output.body;
        let (vectors, count, dimensions, tokens) = match &request.input {
            Input::Titan { dimensions, .. } => (
                Vec::<f32>::deserialize(&payload["embedding"]).map(|vector| vec![vector]),
                1,
                Some(*dimensions),
                payload["inputTextTokenCount"].as_u64(),
            ),
            Input::Cohere(texts) => {
                let vectors = &payload["embeddings"];
                let values = if vectors.is_object() {
                    &vectors["float"]
                } else {
                    vectors
                };
                (
                    Vec::<Vec<f32>>::deserialize(values),
                    texts.len(),
                    None,
                    output.input_tokens,
                )
            }
        };
        let Ok(vectors) = vectors else {
            return relay_response(
                StatusCode::BAD_GATEWAY,
                &json!({}),
                output.request_id.as_deref(),
            );
        };
        if !valid_vectors(&vectors, count, dimensions) {
            return relay_response(
                StatusCode::BAD_GATEWAY,
                &json!({}),
                output.request_id.as_deref(),
            );
        }
        let items: Vec<_> = vectors
            .into_iter()
            .enumerate()
            .map(|(index, vector)| {
                let embedding = if request.base64 {
                    let mut bytes = Vec::with_capacity(vector.len() * size_of::<f32>());
                    bytes.extend(vector.into_iter().flat_map(f32::to_le_bytes));
                    Value::String(Str::to_base64(bytes))
                } else {
                    json!(vector)
                };
                json!({"object":"embedding","embedding":embedding,"index":index})
            })
            .collect();
        let mut response = json!({"object":"list","data":items});
        if let Some(tokens) = tokens {
            response["usage"] = json!({"prompt_tokens":tokens,"total_tokens":tokens});
        }
        relay_response(StatusCode::OK, &response, output.request_id.as_deref())
    }
}

pub(super) fn error_response(failure: InvokeFailure) -> AiResult<Response> {
    let status = match failure.error {
        AiError::ProviderStatus(status)
        | AiError::ProviderOverloaded(status)
        | AiError::ProviderInsufficientCredits(status) => {
            StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY)
        }
        AiError::InvalidResponse => StatusCode::BAD_GATEWAY,
        other => return Err(other),
    };
    relay_response(status, &json!({}), failure.request_id.as_deref())
}

pub(super) fn relay_response(
    status: StatusCode,
    body: &Value,
    request_id: Option<&str>,
) -> AiResult<Response> {
    let headers = relay_headers("application/json", request_id);
    let body = serde_json::to_vec(body).map_err(|_| AiError::InvalidResponse)?;
    Ok(Response::new(status, headers, body))
}

pub(super) fn relay_headers(content_type: &'static str, request_id: Option<&str>) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static(content_type));
    if let Some(id) = request_id.and_then(|id| HeaderValue::from_str(id).ok()) {
        headers.insert("x-request-id", id);
    }
    headers
}
