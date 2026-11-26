use std::collections::HashSet;

use eventsource_stream::{Event, EventStreamError, Eventsource};
use futures_util::StreamExt;
use openrust::http::client::{Error, ResponseStream};
use serde_json::{Map, Value, json};

use crate::{
    AiError, AiResult, StepEvent, StepEventStream, anthropic, gemini, text::transport_error,
};

pub(crate) fn anthropic(upstream: ResponseStream) -> StepEventStream {
    Box::pin(async_stream::try_stream! {
        let mut source = upstream.eventsource();
        let mut message = Value::Null;
        let mut contents = Vec::new();
        let mut active = None::<AnthropicBlock>;
        let mut cited_urls = HashSet::new();
        while let Some(frame) = source.next().await {
            let data = decode(frame)?;
            match data["type"].as_str() {
                Some("ping") => {}
                Some("message_start") if message.is_null() => {
                    message = data["message"].clone();
                    if !message.is_object()
                        || message["content"].as_array().is_some_and(|blocks| !blocks.is_empty())
                    {
                        Err(AiError::InvalidResponse)?;
                    }
                    if !message["usage"].is_object() {
                        message["usage"] = json!({});
                    }
                }
                Some("content_block_start")
                    if active.is_none() && !message.is_null() && message["stop_reason"].is_null() =>
                {
                    if data["index"].as_u64() != Some(contents.len() as u64)
                        || !data["content_block"].is_object()
                    {
                        Err(AiError::InvalidResponse)?;
                    }
                    let block = AnthropicBlock { value: data["content_block"].clone(), arguments: String::new() };
                    if block.value["type"] == "text"
                        && let Some(text) = block.value["text"].as_str().filter(|text| !text.is_empty())
                    {
                        yield StepEvent::TextDelta(text.into());
                    }
                    active = Some(block);
                }
                Some("content_block_delta") if data["index"].as_u64() == Some(contents.len() as u64) => {
                    let block = active.as_mut().ok_or(AiError::InvalidResponse)?;
                    if let Some(text) = block.delta(&data["delta"])? {
                        yield StepEvent::TextDelta(text);
                    }
                }
                Some("content_block_stop") if data["index"].as_u64() == Some(contents.len() as u64) => {
                    let block = active.take().ok_or(AiError::InvalidResponse)?;
                    contents.push(block.complete()?);
                }
                Some("message_delta") if active.is_none() && !message.is_null() => {
                    if let Some(usage) = data["usage"].as_object() {
                        message["usage"].as_object_mut().ok_or(AiError::InvalidResponse)?.extend(usage.clone());
                    }
                    if !data["delta"]["stop_reason"].is_null() {
                        message["stop_reason"] = data["delta"]["stop_reason"].clone();
                    }
                }
                Some("message_stop") if active.is_none() && !message.is_null() => {
                    let reason = match message["stop_reason"].as_str() {
                        Some("end_turn" | "stop_sequence") => "stop",
                        Some("max_tokens" | "model_context_window_exceeded") => "length",
                        Some("tool_use") => "tool_calls",
                        Some("refusal") => Err(AiError::Refused)?,
                        _ => Err(AiError::InvalidResponse)?,
                    };
                    message["content"] = contents.into();
                    let response = anthropic::parse_step(message)?;
                    yield StepEvent::Finished { response: Box::new(response), reason: reason.into() };
                    return;
                }
                _ => Err(AiError::InvalidResponse)?,
            }
            for citation in anthropic::stream_citations(&data) {
                if cited_urls.insert(citation.url.clone()) {
                    yield StepEvent::Citation { part: None, citation };
                }
            }
        }
        Err(AiError::InvalidResponse)?;
    })
}

struct AnthropicBlock {
    value: Value,
    arguments: String,
}

impl AnthropicBlock {
    fn delta(&mut self, delta: &Value) -> AiResult<Option<String>> {
        match (self.value["type"].as_str(), delta["type"].as_str()) {
            (Some("text"), Some("text_delta")) => {
                let text = delta["text"].as_str().ok_or(AiError::InvalidResponse)?;
                self.append("text", text)?;
                Ok(Some(text.into()))
            }
            (Some("text"), Some("citations_delta")) => {
                if self.value["citations"].is_null() {
                    self.value["citations"] = Vec::<Value>::new().into();
                }
                self.value["citations"]
                    .as_array_mut()
                    .ok_or(AiError::InvalidResponse)?
                    .push(
                        delta
                            .get("citation")
                            .ok_or(AiError::InvalidResponse)?
                            .clone(),
                    );
                Ok(None)
            }
            (Some("tool_use"), Some("input_json_delta")) => {
                if !self.value["input"].as_object().is_some_and(Map::is_empty) {
                    return Err(AiError::InvalidToolArguments);
                }
                self.arguments.push_str(
                    delta["partial_json"]
                        .as_str()
                        .ok_or(AiError::InvalidToolArguments)?,
                );
                Ok(None)
            }
            (Some("thinking"), Some("thinking_delta" | "signature_delta")) => {
                let field = if delta["type"] == "thinking_delta" {
                    "thinking"
                } else {
                    "signature"
                };
                let text = delta[field].as_str().ok_or(AiError::InvalidResponse)?;
                self.append(field, text)?;
                Ok(None)
            }
            _ => Err(AiError::InvalidResponse),
        }
    }

    fn append(&mut self, field: &str, value: &str) -> AiResult<()> {
        match &mut self.value[field] {
            Value::String(output) => output.push_str(value),
            empty @ Value::Null => *empty = value.into(),
            _ => return Err(AiError::InvalidResponse),
        }
        Ok(())
    }

    fn complete(mut self) -> AiResult<Value> {
        if !self.arguments.is_empty() {
            let arguments: Map<String, Value> =
                serde_json::from_str(&self.arguments).map_err(|_| AiError::InvalidToolArguments)?;
            self.value["input"] = Value::Object(arguments);
        }
        Ok(self.value)
    }
}

pub(crate) fn gemini(upstream: ResponseStream) -> StepEventStream {
    Box::pin(async_stream::try_stream! {
        let mut source = upstream.eventsource();
        let mut parts = Vec::new();
        let mut usage = Map::new();
        let mut model = Value::Null;
        let mut finish = Value::Null;
        let mut cited_urls = HashSet::new();
        while let Some(frame) = source.next().await {
            let frame = frame.map_err(frame_error)?;
            if frame.data == "[DONE]" {
                break;
            }
            let mut data = decode(Ok(frame))?;
            gemini::finish_reason(&data, &Value::Null)?;
            if let Some(candidate) = data.get_mut("candidates")
                .and_then(Value::as_array_mut)
                .and_then(|candidates| candidates.first_mut())
            {
                if !finish.is_null() {
                    Err(AiError::InvalidResponse)?;
                }
                if gemini::finish_reason(&Value::Null, candidate)?.is_some() {
                    finish = candidate["finishReason"].clone();
                }
                if let Some(Value::Array(chunk)) = candidate.get_mut("content")
                    .and_then(|content| content.get_mut("parts"))
                    .map(Value::take)
                {
                    for part in chunk {
                        if part["thought"] != true && let Some(text) = part["text"].as_str().filter(|text| !text.is_empty()) {
                            yield StepEvent::TextDelta(text.into());
                        }
                        parts.push(part);
                    }
                }
                for citation in gemini::citations(candidate) {
                    if cited_urls.insert(citation.url.clone()) {
                        yield StepEvent::Citation { part: None, citation };
                    }
                }
            }
            if !data["modelVersion"].is_null() {
                model = data["modelVersion"].take();
            }
            if let Value::Object(chunk) = data["usageMetadata"].take() {
                usage.extend(chunk);
            }
        }
        let response = json!({
            "modelVersion":model, "usageMetadata":usage,
            "candidates":[{"finishReason":finish,"content":{"role":"model","parts":parts}}],
        });
        let mut reason = gemini::finish_reason(&response, &response["candidates"][0])?
            .ok_or(AiError::InvalidResponse)?;
        let response = gemini::parse_step(response)?;
        if !response.tool_calls.is_empty() {
            reason = "tool_calls".into();
        }
        yield StepEvent::Finished {response: Box::new(response),reason};
    })
}

fn decode(frame: Result<Event, EventStreamError<Error>>) -> AiResult<Value> {
    let frame = frame.map_err(frame_error)?;
    let data: Value = serde_json::from_str(&frame.data).map_err(|_| AiError::InvalidResponse)?;
    if frame.event == "error" || data.get("error").is_some_and(|error| !error.is_null()) {
        return Err(AiError::InvalidResponse);
    }
    Ok(data)
}

fn frame_error(error: EventStreamError<Error>) -> AiError {
    match error {
        EventStreamError::Transport(error) => transport_error(error),
        _ => AiError::InvalidResponse,
    }
}
