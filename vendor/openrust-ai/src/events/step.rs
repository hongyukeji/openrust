use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use openrust::{contracts::events::Event, support::facades::Event as Events};

use crate::{AgentPrompt, AiError, Step, ToolStep, messages::OwnedPrompt};

use super::{AgentPromptSnapshot, dispatch_prompt_event};

/// 供应商一次生成步骤的请求信息。
pub struct ModelStep {
    pub invocation_id: String,
    pub step_number: usize,
    pub is_final_step: bool,
    pub prompt: AgentPromptSnapshot,
}

pub struct StartingStep {
    pub step: Arc<ModelStep>,
    pub previous_steps: Vec<Step>,
}

impl StartingStep {
    pub const NAME: &str = "openrust_ai::events::StartingStep";
}

impl Event for StartingStep {
    fn name(&self) -> &str {
        Self::NAME
    }
}

pub struct StepCompleted {
    pub step: Arc<ModelStep>,
    pub response: Step,
    pub duration: Duration,
}

impl StepCompleted {
    pub const NAME: &str = "openrust_ai::events::StepCompleted";
}

impl Event for StepCompleted {
    fn name(&self) -> &str {
        Self::NAME
    }
}

pub struct StepFailed {
    pub step: Arc<ModelStep>,
    pub error: AiError,
    pub duration: Duration,
}

impl StepFailed {
    pub const NAME: &str = "openrust_ai::events::StepFailed";
}

impl Event for StepFailed {
    fn name(&self) -> &str {
        Self::NAME
    }
}

pub(crate) struct StepObservation {
    step: Arc<ModelStep>,
    started: Instant,
    completed: bool,
    failed: bool,
}

impl StepObservation {
    pub(crate) fn wants_response(&self) -> bool {
        self.completed
    }

    pub(crate) fn wants_result(&self) -> bool {
        self.completed || self.failed
    }

    pub(crate) async fn start(
        provider: &str,
        prompt: &AgentPrompt<'_>,
        previous_steps: &[ToolStep],
        step_number: usize,
        is_final_step: bool,
    ) -> Option<Self> {
        let (agent, invocation_id) = crate::request_context::agent_invocation()?;
        let dispatcher = Events::dispatcher().ok()?;
        let starting = dispatcher.observes(StartingStep::NAME);
        let completed = dispatcher.observes(StepCompleted::NAME);
        let failed = dispatcher.observes(StepFailed::NAME);
        if !(starting || completed || failed) {
            return None;
        }
        let step = Arc::new(ModelStep {
            invocation_id,
            step_number,
            is_final_step,
            prompt: AgentPromptSnapshot {
                agent,
                provider: provider.to_owned(),
                prompt: OwnedPrompt::from(prompt),
            },
        });
        if starting {
            dispatch_prompt_event(StartingStep {
                step: step.clone(),
                previous_steps: previous_steps
                    .iter()
                    .map(|response| response.public_step(provider))
                    .collect(),
            })
            .await;
        }
        Some(Self {
            step,
            started: Instant::now(),
            completed,
            failed,
        })
    }

    pub(crate) async fn complete(self, response: impl FnOnce(&ModelStep) -> Step) {
        if self.completed {
            let duration = self.started.elapsed();
            let response = response(&self.step);
            dispatch_prompt_event(StepCompleted {
                step: self.step,
                duration,
                response,
            })
            .await;
        }
    }

    pub(crate) async fn fail(self, error: &AiError) {
        if self.failed {
            dispatch_prompt_event(StepFailed {
                step: self.step,
                duration: self.started.elapsed(),
                error: error.clone(),
            })
            .await;
        }
    }
}
