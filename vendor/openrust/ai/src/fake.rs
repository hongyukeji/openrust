use std::{
    collections::VecDeque,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use crate::{AgentResponse, AiError, AiResult, Message, Usage};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordedPrompt {
    pub text: String,
    pub messages: Vec<Message>,
    pub provider: Option<String>,
    pub model: Option<String>,
}

/// 应用内的文本替身及调用记录。
pub struct FakeTextGateway {
    responses: Mutex<VecDeque<String>>,
    prompts: Mutex<Vec<RecordedPrompt>>,
    prevent_stray: AtomicBool,
}

impl FakeTextGateway {
    pub(super) fn new(responses: impl IntoIterator<Item = String>) -> Self {
        Self {
            responses: Mutex::new(responses.into_iter().collect()),
            prompts: Mutex::default(),
            prevent_stray: AtomicBool::new(false),
        }
    }

    pub fn prevent_stray_prompts(&self, prevent: bool) -> &Self {
        self.prevent_stray.store(prevent, Ordering::Relaxed);
        self
    }

    pub fn recorded(&self) -> Vec<RecordedPrompt> {
        self.prompts.lock().unwrap().clone()
    }

    #[track_caller]
    pub fn assert_prompted(&self, predicate: impl Fn(&RecordedPrompt) -> bool) {
        assert!(
            self.recorded().iter().any(predicate),
            "未发现匹配的 AI 提示"
        );
    }

    #[track_caller]
    pub fn assert_not_prompted(&self, predicate: impl Fn(&RecordedPrompt) -> bool) {
        assert!(
            !self.recorded().iter().any(predicate),
            "发现了不应发送的 AI 提示"
        );
    }

    #[track_caller]
    pub fn assert_prompted_times(&self, count: usize) {
        assert_eq!(
            self.prompts.lock().unwrap().len(),
            count,
            "AI 提示次数不匹配"
        );
    }

    #[track_caller]
    pub fn assert_never_prompted(&self) {
        self.assert_prompted_times(0);
    }

    pub(super) fn respond(
        &self,
        prompt: &str,
        messages: &[Message],
        provider: Option<String>,
        model: Option<String>,
    ) -> AiResult<AgentResponse> {
        let text = match self.responses.lock().unwrap().pop_front() {
            Some(text) => text,
            None if self.prevent_stray.load(Ordering::Relaxed) => {
                return Err(AiError::FakeExhausted);
            }
            None => format!(
                "Fake response for prompt: {}",
                prompt
                    .split_whitespace()
                    .take(10)
                    .collect::<Vec<_>>()
                    .join(" ")
            ),
        };
        self.prompts.lock().unwrap().push(RecordedPrompt {
            text: prompt.into(),
            messages: messages.to_vec(),
            provider: provider.clone(),
            model: model.clone(),
        });
        Ok(AgentResponse {
            text,
            usage: Usage::default(),
            provider: provider.unwrap_or_default(),
            model: model.unwrap_or_default(),
        })
    }
}
