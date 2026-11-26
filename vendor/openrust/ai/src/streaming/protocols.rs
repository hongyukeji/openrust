use std::convert::Infallible;

use futures_util::StreamExt;
use openrust::{
    http::{HeaderMap, Response, StatusCode},
    support::Str,
};
use serde_json::{Value, json};

use crate::{AgentEventStream, PendingApproval, StreamEventData, Usage};

/// 将 Agent 事件封装成公开 HTTP 流。
pub trait StreamProtocol: Send {
    fn response(self: Box<Self>, events: AgentEventStream) -> Response;
}

pub struct VercelDataProtocol {
    message_id: Option<String>,
}

impl VercelDataProtocol {
    pub fn new(message_id: Option<String>) -> Self {
        Self { message_id }
    }
}

impl StreamProtocol for VercelDataProtocol {
    fn response(self: Box<Self>, mut events: AgentEventStream) -> Response {
        let mut headers = headers();
        headers.insert(
            "x-vercel-ai-ui-message-stream",
            "v1".parse().expect("valid header"),
        );
        let body = async_stream::stream! {
            let mut started = false;
            let mut completion = None;
            while let Some(event) = events.next().await {
                match event {
                    Ok(event) => match event.data {
                        StreamEventData::StreamStart { .. } => {
                            if started {
                                yield payload(json!({"type":"finish-step"}));
                            } else {
                                started = true;
                                let id = self.message_id.as_deref().unwrap_or(&event.id);
                                yield payload(json!({"type":"start","messageId":id}));
                            }
                            yield payload(json!({"type":"start-step"}));
                        }
                        StreamEventData::TextStart { message_id } => {
                            yield payload(json!({"type":"text-start","id":message_id}));
                        }
                        StreamEventData::TextDelta { message_id, delta } => {
                            yield payload(json!({"type":"text-delta","id":message_id,"delta":delta}));
                        }
                        StreamEventData::TextEnd { message_id } => {
                            yield payload(json!({"type":"text-end","id":message_id}));
                        }
                        StreamEventData::ToolCall { call } => {
                            yield payload(json!({"type":"tool-input-available","toolCallId":call.id,"toolName":call.name,"input":call.arguments}));
                        }
                        StreamEventData::ToolResult { result } => {
                            let data = if result.failed {
                                json!({"type":"tool-output-error","toolCallId":result.id,"errorText":result.result})
                            } else {
                                json!({"type":"tool-output-available","toolCallId":result.id,"output":result.result})
                            };
                            yield payload(data);
                        }
                        StreamEventData::ToolApprovalRequest { conversation_id, approval_id, approvals } => {
                            yield payload(json!({
                                "type":"data-tool-approval",
                                "data":{"conversationId":conversation_id,"approvalBatchId":approval_id},
                            }));
                            for approval in approvals {
                                yield payload(json!({
                                    "type":"tool-approval-request",
                                    "toolCallId":approval.id,
                                    "approvalId":approval.id,
                                    "reason":approval.reason,
                                }));
                            }
                            if started {
                                yield payload(json!({"type":"finish-step"}));
                            }
                            yield Ok("data: [DONE]\n\n".into());
                            return;
                        }
                        StreamEventData::StreamEnd { reason, usage } => record_completion(&mut completion, reason, usage),
                    },
                    Err(_) => {
                        if !started {
                            let id = self.message_id.clone().unwrap_or_else(|| Str::ulid().to_string());
                            yield payload(json!({"type":"start","messageId":id}));
                            yield payload(json!({"type":"start-step"}));
                        }
                        yield payload(json!({"type":"error","errorText":"发生错误"}));
                        yield Ok("data: [DONE]\n\n".into());
                        return;
                    }
                }
            }
            if started {
                yield payload(json!({"type":"finish-step"}));
                if let Some((reason, usage)) = completion {
                    yield payload(json!({
                        "type":"finish","finishReason":finish_reason(&reason),
                        "messageMetadata":{"usage":usage_data(&usage)},
                    }));
                }
            }
            yield Ok("data: [DONE]\n\n".into());
        };
        Response::stream(StatusCode::OK, body).with_headers(headers)
    }
}

pub struct AgentUserInteractionProtocol {
    thread_id: String,
    run_id: Option<String>,
}

impl AgentUserInteractionProtocol {
    pub fn new(thread_id: Option<String>, run_id: Option<String>) -> Self {
        Self {
            thread_id: thread_id.unwrap_or_else(|| Str::ulid().to_string()),
            run_id,
        }
    }
}

impl StreamProtocol for AgentUserInteractionProtocol {
    fn response(mut self: Box<Self>, mut events: AgentEventStream) -> Response {
        let body = async_stream::stream! {
            let mut step = 0_u64;
            let mut destination = None;
            let mut completion = None;
            while let Some(event) = events.next().await {
                match event {
                    Ok(event) => match event.data {
                        StreamEventData::StreamStart { provider, model } => {
                            destination = Some((provider, model));
                            if step == 0 {
                                let run_id = self.run_id.get_or_insert(event.invocation_id);
                                yield payload(json!({"type":"RUN_STARTED","threadId":self.thread_id,"runId":run_id}));
                            } else {
                                yield payload(json!({"type":"STEP_FINISHED","stepName":step.to_string()}));
                            }
                            step += 1;
                            yield payload(json!({"type":"STEP_STARTED","stepName":step.to_string()}));
                        }
                        StreamEventData::TextStart { message_id } => {
                            yield payload(json!({"type":"TEXT_MESSAGE_START","messageId":message_id,"role":"assistant"}));
                        }
                        StreamEventData::TextDelta { message_id, delta } => {
                            yield payload(json!({"type":"TEXT_MESSAGE_CONTENT","messageId":message_id,"delta":delta}));
                        }
                        StreamEventData::TextEnd { message_id } => {
                            yield payload(json!({"type":"TEXT_MESSAGE_END","messageId":message_id}));
                        }
                        StreamEventData::ToolCall { call } => {
                            yield payload(json!({"type":"TOOL_CALL_START","toolCallId":call.id,"toolCallName":call.name}));
                            yield payload(json!({"type":"TOOL_CALL_ARGS","toolCallId":call.id,"delta":serde_json::to_string(&call.arguments).expect("参数为 JSON 对象")}));
                            yield payload(json!({"type":"TOOL_CALL_END","toolCallId":call.id}));
                        }
                        StreamEventData::ToolResult { result } => {
                            yield payload(json!({"type":"TOOL_CALL_RESULT","messageId":event.id,"toolCallId":result.id,"content":result.result,"role":"tool"}));
                        }
                        StreamEventData::ToolApprovalRequest { conversation_id, approval_id, approvals } => {
                            let interrupts: Vec<_> = approvals.into_iter().map(|approval| {
                                approval_interrupt(approval, &conversation_id, &approval_id)
                            }).collect();
                            if step > 0 {
                                yield payload(json!({"type":"STEP_FINISHED","stepName":step.to_string()}));
                            }
                            yield payload(json!({
                                "type":"RUN_FINISHED","threadId":self.thread_id,"runId":self.run_id,
                                "outcome":{"type":"interrupt","interrupts":interrupts},
                            }));
                            return;
                        }
                        StreamEventData::StreamEnd { reason, usage } => record_completion(&mut completion, reason, usage),
                    },
                    Err(_) => {
                        if step == 0 {
                            let run_id = self.run_id.get_or_insert_with(|| Str::ulid().to_string());
                            yield payload(json!({"type":"RUN_STARTED","threadId":self.thread_id,"runId":run_id}));
                            yield payload(json!({"type":"STEP_STARTED","stepName":"1"}));
                        }
                        yield payload(json!({"type":"RUN_ERROR","message":"发生错误"}));
                        return;
                    }
                }
            }
            if step > 0 {
                yield payload(json!({"type":"STEP_FINISHED","stepName":step.to_string()}));
                let mut finished = json!({"type":"RUN_FINISHED","threadId":self.thread_id,"runId":self.run_id});
                if let Some((reason, usage)) = completion {
                    let mut usage = usage_data(&usage);
                    if let Some((provider, model)) = destination {
                        usage["provider"] = provider.into();
                        usage["model"] = model.into();
                    }
                    finished["usage"] = json!([usage]);
                    finished["metadata"] = json!({"finishReason":reason});
                }
                yield payload(finished);
            }
        };
        Response::stream(StatusCode::OK, body).with_headers(headers())
    }
}

pub(super) fn headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        "content-type",
        "text/event-stream".parse().expect("valid header"),
    );
    headers.insert(
        "cache-control",
        "no-cache, no-transform".parse().expect("valid header"),
    );
    headers
}

fn approval_interrupt(
    approval: PendingApproval,
    conversation_id: &str,
    approval_id: &str,
) -> Value {
    let mut interrupt = json!({
        "id": approval.id,
        "reason": "approval_required",
        "toolCallId": approval.id,
        "metadata": {
            "kind": "approval", "toolName": approval.tool, "input": approval.arguments,
            "conversationId": conversation_id, "approvalBatchId": approval_id,
        },
        "responseSchema": {
            "type": "object", "properties": {"approved": {"type": "boolean"}},
            "required": ["approved"],
        },
    });
    if let Some(reason) = approval.reason {
        interrupt["message"] = reason.into();
    }
    interrupt
}

fn payload(value: Value) -> Result<String, Infallible> {
    Ok(format!("data: {value}\n\n"))
}

fn record_completion(completion: &mut Option<(String, Usage)>, reason: String, mut usage: Usage) {
    if let Some((_, previous)) = completion.as_ref() {
        usage = usage.combine(previous);
    }
    *completion = Some((reason, usage));
}

fn usage_data(usage: &Usage) -> Value {
    let fields = [
        ("inputTokens", usage.prompt_tokens),
        ("outputTokens", usage.completion_tokens),
        ("totalTokens", usage.total_tokens()),
        ("cachedInputTokens", usage.cache_read_input_tokens),
        ("cacheWriteInputTokens", usage.cache_write_input_tokens),
        ("reasoningTokens", usage.reasoning_tokens),
    ];
    Value::Object(
        fields
            .into_iter()
            .filter_map(|(name, value)| Some((name.to_owned(), value?.into())))
            .collect(),
    )
}

fn finish_reason(reason: &str) -> &'static str {
    match reason {
        "stop" => "stop",
        "tool_calls" => "tool-calls",
        "length" | "max_output_tokens" => "length",
        "content_filter" => "content-filter",
        "error" => "error",
        _ => "other",
    }
}
