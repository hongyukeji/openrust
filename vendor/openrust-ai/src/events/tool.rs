use std::{
    future::Future,
    sync::Arc,
    time::{Duration, Instant},
};

use openrust::{contracts::events::Event, support::facades::Event as Events};

use crate::{AiError, AiResult, ToolCall, ToolRequest};

use super::dispatch_prompt_event;

/// 工具处理器实际收到的调用。审批编辑后的参数保留在 `call` 中。
pub struct ToolInvocation {
    pub invocation_id: String,
    pub tool_invocation_id: String,
    pub agent: &'static str,
    pub call: ToolCall,
}

pub struct InvokingTool {
    pub invocation: Arc<ToolInvocation>,
}

impl InvokingTool {
    pub const NAME: &str = "openrust_ai::events::InvokingTool";
}

impl Event for InvokingTool {
    fn name(&self) -> &str {
        Self::NAME
    }
}

pub struct ToolInvoked {
    pub invocation: Arc<ToolInvocation>,
    pub result: String,
    pub duration: Duration,
}

impl ToolInvoked {
    pub const NAME: &str = "openrust_ai::events::ToolInvoked";
}

impl Event for ToolInvoked {
    fn name(&self) -> &str {
        Self::NAME
    }
}

pub struct ToolFailed {
    pub invocation: Arc<ToolInvocation>,
    pub error: AiError,
    pub duration: Duration,
}

impl ToolFailed {
    pub const NAME: &str = "openrust_ai::events::ToolFailed";
}

impl Event for ToolFailed {
    fn name(&self) -> &str {
        Self::NAME
    }
}

pub(crate) async fn observe_tool<'a, Fut>(
    request: ToolRequest<'a>,
    invoke: impl FnOnce(ToolRequest<'a>) -> Fut,
) -> AiResult<String>
where
    Fut: Future<Output = AiResult<String>>,
{
    let (Some((agent, invocation_id)), Ok(dispatcher)) = (
        crate::request_context::agent_invocation(),
        Events::dispatcher(),
    ) else {
        return invoke(request).await;
    };
    let starting = dispatcher.observes(InvokingTool::NAME);
    let completed = dispatcher.observes(ToolInvoked::NAME);
    let failed = dispatcher.observes(ToolFailed::NAME);
    if !(starting || completed || failed) {
        return invoke(request).await;
    }
    let invocation = Arc::new(ToolInvocation {
        invocation_id,
        tool_invocation_id: request.tool_invocation_id().into(),
        agent,
        call: request.call.clone(),
    });
    if starting {
        dispatch_prompt_event(InvokingTool {
            invocation: invocation.clone(),
        })
        .await;
    }
    let started = Instant::now();
    let result = invoke(request).await;
    match &result {
        Ok(result) if completed => {
            dispatch_prompt_event(ToolInvoked {
                invocation,
                result: result.clone(),
                duration: started.elapsed(),
            })
            .await;
        }
        Err(error) if failed => {
            dispatch_prompt_event(ToolFailed {
                invocation,
                error: error.clone(),
                duration: started.elapsed(),
            })
            .await;
        }
        _ => {}
    }
    result
}
