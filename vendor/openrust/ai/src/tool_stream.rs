use std::{collections::HashSet, sync::Arc, time::Duration};

use futures_util::StreamExt;
use openrust::support::{Str, facades::App};
use serde_json::Value;

use crate::{
    AgentPrompt, AiError, AiResult, Message, StepEvent, StreamEventData, StreamableAgentResponse,
    StructuredOutput, TextPrompt, TextProvider, Tool, ToolStep, streaming::event, tools,
};

pub(crate) async fn stream(
    provider: Arc<dyn TextProvider>,
    prompt: AgentPrompt<'_>,
    registered: Vec<Arc<dyn Tool>>,
    max_steps: Option<usize>,
) -> AiResult<StreamableAgentResponse> {
    let application = App::get_instance().ok();
    let max_steps = tools::step_limit(registered.len(), max_steps);
    let (definitions, registered) = tools::prepare(registered)?;
    if let Some(choice) = prompt.tool_choice {
        choice.validate(&definitions)?;
    }
    let gateway = provider
        .step_text_gateway()
        .ok_or(AiError::ToolsNotSupported)?;
    let mut upstream = gateway.stream_text_step(&prompt, &definitions, &[]).await?;
    let prompt = OwnedPrompt::from(prompt);
    let stream = async_stream::try_stream! {
        let invocation_id = Str::ulid().to_string();
        let mut history = Vec::new();
        let mut seen = HashSet::new();
        for step in 1..=max_steps {
            yield event(&invocation_id, StreamEventData::StreamStart {
                provider: provider.name().into(),
                model: prompt.model.clone(),
            });
            let mut message_id = None::<String>;
            let (completed, reason) = loop {
                let part = upstream.next().await.ok_or(AiError::InvalidResponse)??;
                match part {
                    StepEvent::TextDelta(delta) if !delta.is_empty() => {
                        let id = match &message_id {
                            Some(id) => id.clone(),
                            None => {
                                let id = Str::ulid().to_string();
                                message_id = Some(id.clone());
                                yield event(&invocation_id, StreamEventData::TextStart { message_id: id.clone() });
                                id
                            }
                        };
                        yield event(&invocation_id, StreamEventData::TextDelta { message_id: id, delta });
                    }
                    StepEvent::TextDelta(_) => {}
                    StepEvent::Finished { response, reason } => break (response, reason),
                }
            };
            drop(upstream);
            let message_id = match (message_id, completed.text.as_ref().filter(|text| !text.is_empty())) {
                (None, Some(text)) => {
                    let id = Str::ulid().to_string();
                    yield event(&invocation_id, StreamEventData::TextStart { message_id: id.clone() });
                    yield event(&invocation_id, StreamEventData::TextDelta { message_id: id.clone(), delta: text.clone() });
                    Some(id)
                }
                (existing, _) => existing,
            };
            if let Some(message_id) = message_id {
                yield event(&invocation_id, StreamEventData::TextEnd { message_id });
            } else if completed.tool_calls.is_empty() {
                Err(AiError::InvalidResponse)?;
            }
            if let Some(choice) = prompt.choice_for(step - 1) {
                choice.validate_calls(&completed.tool_calls)?;
            }
            tools::validate_calls(&completed.tool_calls, &mut seen)?;
            for call in &completed.tool_calls {
                yield event(&invocation_id, StreamEventData::ToolCall { call: call.clone() });
            }
            let execution = tools::execute(&completed.tool_calls, &registered, None, step == max_steps);
            let results = match &application {
                Some(application) => application.scope(execution).await,
                None => execution.await,
            };
            for result in &results {
                yield event(&invocation_id, StreamEventData::ToolResult { result: result.clone() });
            }
            yield event(&invocation_id, StreamEventData::StreamEnd { reason, usage: completed.usage });
            if completed.tool_calls.is_empty() || step == max_steps {
                return;
            }
            history.push(ToolStep {
                text: completed.text,
                calls: completed.tool_calls,
                results,
                provider_content: completed.provider_content,
            });
            upstream = provider.step_text_gateway().ok_or(AiError::ToolsNotSupported)?
                .stream_text_step(&prompt.as_prompt(step), &definitions, &history).await?;
        }
    };
    Ok(StreamableAgentResponse::from_events(Box::pin(stream)))
}

struct OwnedPrompt {
    instructions: String,
    text: String,
    model: String,
    timeout: Option<Duration>,
    options: crate::TextGenerationOptions,
    provider_options: Option<crate::ProviderOptions>,
    tool_choice: Option<crate::ToolChoice>,
    messages: Vec<Message>,
    attachments: Vec<crate::Attachment>,
    schema: Option<(Value, bool)>,
}

impl From<AgentPrompt<'_>> for OwnedPrompt {
    fn from(value: AgentPrompt<'_>) -> Self {
        Self {
            instructions: value.text.instructions.into(),
            text: value.text.prompt.into(),
            model: value.text.model.into(),
            timeout: value.text.timeout,
            options: value.options,
            provider_options: value.provider_options.cloned(),
            tool_choice: value.tool_choice.cloned(),
            messages: value.messages.to_vec(),
            attachments: value.attachments.to_vec(),
            schema: value
                .structured_output
                .map(|output| (output.schema.clone(), output.strict)),
        }
    }
}

impl OwnedPrompt {
    fn choice_for(&self, step: usize) -> Option<&crate::ToolChoice> {
        self.tool_choice
            .as_ref()
            .and_then(|choice| choice.for_step(step))
    }

    fn as_prompt(&self, step: usize) -> AgentPrompt<'_> {
        AgentPrompt {
            text: TextPrompt {
                instructions: &self.instructions,
                prompt: &self.text,
                model: &self.model,
                timeout: self.timeout,
            },
            options: self.options,
            provider_options: self.provider_options.as_ref(),
            tool_choice: self.choice_for(step),
            messages: &self.messages,
            attachments: &self.attachments,
            structured_output: self
                .schema
                .as_ref()
                .map(|(schema, strict)| StructuredOutput {
                    schema,
                    strict: *strict,
                }),
        }
    }
}
