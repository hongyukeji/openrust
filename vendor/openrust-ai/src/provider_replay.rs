use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct ProviderReplay {
    scope: String,
    model: String,
    content: ReplayContent,
}

#[derive(Clone, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum ReplayContent {
    ChatReasoning(String),
}

impl fmt::Debug for ProviderReplay {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderReplay")
            .finish_non_exhaustive()
    }
}

impl ProviderReplay {
    pub(crate) fn capture(
        scope: Option<&str>,
        model: &str,
        content: Option<&Value>,
    ) -> Option<Self> {
        let reasoning = content?.get("reasoning_content")?.as_str()?;
        let scope = scope?;
        if reasoning.is_empty() || model.is_empty() {
            return None;
        }
        Some(Self {
            scope: scope.into(),
            model: model.into(),
            content: ReplayContent::ChatReasoning(reasoning.into()),
        })
    }

    pub(crate) fn chat_reasoning(&self, scope: &str, model: &str) -> Option<&str> {
        let ReplayContent::ChatReasoning(reasoning) = &self.content;
        (self.scope == scope && self.model == model).then_some(reasoning)
    }
}
