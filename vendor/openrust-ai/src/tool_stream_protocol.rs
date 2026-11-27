use std::collections::BTreeMap;

use eventsource_stream::{EventStreamError, Eventsource};
use futures_util::StreamExt;
use openrust::http::client::ResponseStream;
use serde_json::{Value, json};

use crate::{
    AiError, AiResult, ReasoningPart, StepEvent, StepEventStream, StepResponse, UrlCitation, Usage,
    streaming::output_part,
    text::{OpenAiProtocol, transport_error},
    tool_protocol,
};

pub(crate) fn openai(
    protocol: OpenAiProtocol,
    upstream: ResponseStream,
    structured: bool,
) -> StepEventStream {
    Box::pin(async_stream::try_stream! {
        let mut source = upstream.eventsource();
        let mut chat = Chat::default();
        let mistral = matches!(protocol, OpenAiProtocol::Mistral);
        let groq = matches!(protocol, OpenAiProtocol::Groq);
        let mut thinking = String::new();
        let mut reasoning_open = false;
        let mut reasoning_id = 0;
        while let Some(frame) = source.next().await {
            let frame = frame.map_err(|error| match error {
                EventStreamError::Transport(error) => transport_error(error),
                _ => AiError::InvalidResponse,
            })?;
            if frame.data == "[DONE]" && !matches!(protocol, OpenAiProtocol::Responses | OpenAiProtocol::Xai) {
                if reasoning_open {
                    yield StepEvent::Reasoning(ReasoningPart::End { part: ("thinking".into(), reasoning_id), text: None });
                }
                let (mut response, reason) = chat.finish()?;
                if groq && structured && response.tool_calls.is_empty()
                    && serde_json::from_str::<serde_json::Map<String, Value>>(
                        response.text.as_deref().unwrap_or_default(),
                    ).is_err()
                {
                    Err(AiError::InvalidStructuredResponse)?;
                }
                response.reasoning = thinking;
                yield StepEvent::Finished { response: Box::new(response), reason };
                return;
            }
            let mut data: Value = serde_json::from_str(&frame.data).map_err(|_| AiError::InvalidResponse)?;
            if data.get("error").is_some_and(|value| !value.is_null()) || frame.event == "error" || (mistral && data["object"] == "error") {
                Err(AiError::InvalidResponse)?;
            }
            match protocol {
                OpenAiProtocol::ChatCompletions | OpenAiProtocol::Mistral | OpenAiProtocol::Groq => {
                    if groq {
                        chat.usage.update(tool_protocol::groq_usage(&data));
                    }
                    if (mistral || groq)
                        && let Some(choice) = data.get_mut("choices").and_then(|choices| choices.get_mut(0))
                    {
                        if mistral && let Some(reason) = choice.get_mut("finish_reason").filter(|reason| *reason == "model_length") {
                            *reason = "length".into();
                        }
                        let delta = choice.get_mut("delta").and_then(Value::as_object_mut);
                        let Some(delta) = delta else {
                            chat.consume(&data)?;
                            continue;
                        };
                        let (text, thought) = if mistral {
                            tool_protocol::mistral_content(delta.get("content").unwrap_or(&Value::Null))
                        } else {
                            (
                                delta.get("content").and_then(Value::as_str).unwrap_or_default().to_owned(),
                                delta.get("reasoning").and_then(Value::as_str).unwrap_or_default().to_owned(),
                            )
                        };
                        if !thought.is_empty() {
                            thinking.push_str(&thought);
                            reasoning_open = true;
                            yield StepEvent::Reasoning(ReasoningPart::Delta {
                                part: ("thinking".into(), reasoning_id),
                                delta: thought,
                            });
                        }
                        if reasoning_open && (!text.is_empty() || delta.get("tool_calls").is_some_and(Value::is_array)) {
                            yield StepEvent::Reasoning(ReasoningPart::End { part: ("thinking".into(), reasoning_id), text: None });
                            reasoning_id += 1;
                            reasoning_open = false;
                        }
                        if mistral && let Some(content) = delta.get_mut("content").filter(|content| content.is_array()) {
                            *content = text.into();
                        }
                    }
                    if let Some(delta) = chat.consume(&data)? {
                        yield StepEvent::TextDelta(delta);
                    }
                }
                OpenAiProtocol::Responses | OpenAiProtocol::Xai => match data["type"].as_str() {
                    Some("response.output_text.delta") => {
                        let delta = data["delta"].as_str().ok_or(AiError::InvalidResponse)?;
                        if !delta.is_empty() {
                            if let Some(part) = output_part(&data) {
                                yield StepEvent::TextPartDelta { part, delta: delta.into() };
                            } else {
                                yield StepEvent::TextDelta(delta.into());
                            }
                        }
                    }
                    Some("response.output_text.done") => {
                        if let Some(part) = output_part(&data) {
                            yield StepEvent::TextPartEnd { part };
                        }
                    }
                    Some("response.output_text.annotation.added") => {
                        if let Some(citation) = UrlCitation::from_annotation(&data["annotation"]) {
                            yield StepEvent::Citation { part: output_part(&data), citation };
                        }
                    }
                    Some("response.completed") => {
                        let response = tool_protocol::responses_step(data["response"].take(), matches!(protocol, OpenAiProtocol::Xai))?;
                        let reason = if response.tool_calls.is_empty() { "stop" } else { "tool_calls" };
                        yield StepEvent::Finished { response: Box::new(response), reason: reason.into() };
                        return;
                    }
                    Some("response.refusal.delta" | "response.refusal.done") => Err(AiError::Refused)?,
                    Some("response.failed" | "response.incomplete") => Err(AiError::InvalidResponse)?,
                    _ => {
                        if let Some(part) = ReasoningPart::from_openai_frame(&data) {
                            yield StepEvent::Reasoning(part);
                        }
                    }
                },
            }
        }
        // 工具执行以明确的上游终止帧为准。
        Err(AiError::InvalidResponse)?;
    })
}

#[derive(Default)]
struct Chat {
    model: Option<String>,
    text: Option<String>,
    reason: Option<String>,
    reasoning_content: Option<String>,
    reasoning_replay: Option<String>,
    usage: Usage,
    calls: BTreeMap<u64, Call>,
}

#[derive(Default)]
struct Call {
    id: Option<String>,
    name: Option<String>,
    arguments: String,
}

impl Chat {
    fn consume(&mut self, data: &Value) -> AiResult<Option<String>> {
        if let Some(model) = data["model"].as_str() {
            self.model = Some(model.into());
        }
        if let Some(usage) = data.get("usage") {
            self.usage.update(Usage::from_openai(usage));
        }
        let Some(choice) = data["choices"].get(0) else {
            return Ok(None);
        };
        let delta = &choice["delta"];
        if let Some(reasoning) = delta["reasoning_content"].as_str() {
            self.reasoning_content
                .get_or_insert_default()
                .push_str(reasoning);
        }
        set_once(
            &mut self.reasoning_replay,
            delta[crate::CHAT_REASONING_REPLAY].as_str(),
        )?;
        if delta["refusal"]
            .as_str()
            .is_some_and(|message| !message.is_empty())
        {
            return Err(AiError::Refused);
        }
        if let Some(reason) = choice["finish_reason"].as_str() {
            if reason == "content_filter" {
                return Err(AiError::Refused);
            }
            if self.reason.replace(reason.into()).is_some() {
                return Err(AiError::InvalidResponse);
            }
        }
        if let Some(calls) = delta.get("tool_calls").filter(|calls| !calls.is_null()) {
            for item in calls.as_array().ok_or(AiError::InvalidResponse)? {
                if item["type"].as_str().is_some_and(|kind| kind != "function") {
                    return Err(AiError::InvalidResponse);
                }
                let index = item["index"].as_u64().ok_or(AiError::InvalidResponse)?;
                let call = self.calls.entry(index).or_default();
                set_once(&mut call.id, item["id"].as_str())?;
                set_once(&mut call.name, item["function"]["name"].as_str())?;
                if let Some(arguments) = item["function"]["arguments"].as_str() {
                    call.arguments.push_str(arguments);
                }
            }
        }
        let text = delta["content"].as_str().filter(|text| !text.is_empty());
        if let Some(text) = text {
            self.text.get_or_insert_default().push_str(text);
        }
        Ok(text.map(str::to_owned))
    }

    fn finish(self) -> AiResult<(StepResponse, String)> {
        let reason = self.reason.ok_or(AiError::InvalidResponse)?;
        if !matches!(reason.as_str(), "stop" | "length" | "tool_calls")
            || (reason == "tool_calls") == self.calls.is_empty()
        {
            return Err(AiError::InvalidResponse);
        }
        let calls: Vec<_> = self
            .calls
            .into_values()
            .map(|call| {
                json!({
                    "id":call.id,"type":"function",
                    "function":{"name":call.name,"arguments":call.arguments},
                })
            })
            .collect();
        let mut response = tool_protocol::chat_step(json!({
            "model":self.model,"choices":[{
                "finish_reason":reason,
                "message":{"content":self.text,"tool_calls":calls,"reasoning_content":self.reasoning_content,
                    crate::CHAT_REASONING_REPLAY:self.reasoning_replay}
            }],
        }))?;
        response.usage = self.usage;
        Ok((response, reason))
    }
}

fn set_once(target: &mut Option<String>, value: Option<&str>) -> AiResult<()> {
    if let Some(value) = value {
        match target {
            None => *target = Some(value.into()),
            Some(previous) if previous != value => return Err(AiError::InvalidResponse),
            _ => {}
        }
    }
    Ok(())
}
