use futures_util::StreamExt;
use openrust::{
    broadcasting::{BroadcastChannel, BroadcastPayload},
    log::{Level, LogContext},
    support::{
        Str,
        facades::{Broadcast, Log},
    },
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    Agent, AgentResponse, AiError, AiResult, StreamEvent, StreamEventData,
    streaming::StreamResponse,
};

pub(crate) struct StreamBroadcast {
    pub(crate) channels: Vec<BroadcastChannel>,
    pub(crate) excluded: &'static [&'static str],
    pub(crate) immediate: bool,
}

impl StreamBroadcast {
    pub(crate) fn validate(&self) -> AiResult<()> {
        validate_channels(&self.channels)
    }

    pub(crate) async fn publish(&self, event: &StreamEvent) {
        let kind = match &event.data {
            StreamEventData::StreamStart { .. } => "stream_start",
            StreamEventData::TextStart { .. } => "text_start",
            StreamEventData::TextDelta { .. } => "text_delta",
            StreamEventData::TextEnd { .. } => "text_end",
            StreamEventData::Citation { .. } => "citation",
            StreamEventData::ReasoningStart { .. } => "reasoning_start",
            StreamEventData::ReasoningDelta { .. } => "reasoning_delta",
            StreamEventData::ReasoningEnd { .. } => "reasoning_end",
            StreamEventData::ToolCall { .. } => "tool_call",
            StreamEventData::ToolResult { .. } => "tool_result",
            StreamEventData::ToolApprovalRequest { .. } => "tool_approval_request",
            StreamEventData::StreamEnd { .. } => "stream_end",
        };
        if !self.excluded.contains(&kind) {
            send(
                &self.channels,
                kind,
                serde_json::to_value(event),
                self.immediate,
            )
            .await;
        }
    }
}

fn validate_channels(channels: &[BroadcastChannel]) -> AiResult<()> {
    if channels.is_empty()
        || channels.iter().any(|channel| {
            let name = match channel {
                BroadcastChannel::Public(channel) => &channel.name,
                BroadcastChannel::Private(channel) => &channel.name,
                BroadcastChannel::Presence(channel) => &channel.name,
            };
            name.trim().is_empty() || name.chars().any(char::is_control)
        })
    {
        return Err(AiError::InvalidConfiguration("AI 广播频道"));
    }
    Ok(())
}

async fn send(
    channels: &[BroadcastChannel],
    kind: &str,
    payload: serde_json::Result<Value>,
    now: bool,
) {
    let Ok(Value::Object(payload)) = payload else {
        return report_failure();
    };
    if dispatch(channels, kind, payload, now).await.is_err() {
        report_failure();
    }
}

async fn dispatch(
    channels: &[BroadcastChannel],
    kind: &str,
    payload: BroadcastPayload,
    now: bool,
) -> openrust::events::EventResult<openrust::events::DispatchResult> {
    let event = Broadcast::on(channels.to_vec()).r#as(kind).with(payload);
    if now {
        event.send_now().await
    } else {
        event.send().await
    }
}

fn report_failure() {
    let _ = Log::log(Level::Warning, "AI 流事件广播失败", &LogContext::new());
}

#[derive(Deserialize, Serialize)]
pub(crate) struct QueuedBroadcast {
    channels: Vec<BroadcastChannel>,
    invocation_id: String,
}

impl QueuedBroadcast {
    pub(crate) fn new(channels: Vec<BroadcastChannel>) -> Self {
        Self {
            channels,
            invocation_id: Str::uuid7().to_string(),
        }
    }

    pub(crate) fn invocation_id(&self) -> &str {
        &self.invocation_id
    }

    pub(crate) fn validate(&self) -> AiResult<()> {
        validate_channels(&self.channels)
    }

    pub(crate) async fn run(
        &self,
        agent: &impl Agent,
        text: &str,
        options: crate::PromptOptions<'_>,
    ) -> AiResult<AgentResponse> {
        self.validate()?;
        let broadcast = StreamBroadcast {
            channels: self.channels.clone(),
            excluded: agent.without_broadcasting(),
            immediate: true,
        };
        let stream = agent.stream_with(text, options).await?;
        let metadata = stream.step_metadata();
        let mut events = stream.into_stream();
        let mut response = StreamResponse::default();
        let mut completed = false;
        while let Some(event) = events.next().await {
            let mut event = event?;
            event.invocation_id.clone_from(&self.invocation_id);
            broadcast.publish(&event).await;
            completed = matches!(event.data, StreamEventData::StreamEnd { .. });
            response.record(&event.data);
        }
        completed
            .then(|| response.finish(metadata.as_ref()))
            .ok_or(AiError::InvalidResponse)
    }

    pub(crate) async fn failed(&self) {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let event = json!({
            "id": Str::ulid().to_string(),
            "invocation_id": self.invocation_id,
            "type": "stream_failed",
            "message": "Agent 流生成失败",
            "recoverable": false,
            "timestamp": timestamp,
        });
        send(&self.channels, "stream_failed", Ok(event), true).await;
    }
}
