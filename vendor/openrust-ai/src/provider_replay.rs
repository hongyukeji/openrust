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
    ResponsesItems(Vec<Value>),
    AnthropicBlocks(Vec<Value>),
    GeminiContent(Value),
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
        let scope = scope?;
        if model.is_empty() {
            return None;
        }
        let source = content?;
        let content =
            if let Some(reasoning) = source.get("reasoning_content").and_then(Value::as_str) {
                if reasoning.is_empty() {
                    return None;
                }
                ReplayContent::ChatReasoning(reasoning.to_owned())
            } else if let Some(items) = source.get("responses_items").and_then(Value::as_array) {
                ReplayContent::ResponsesItems(items.clone())
            } else if let Some(blocks) = source
                .as_array()
                .filter(|blocks| anthropic_replayable(blocks))
            {
                ReplayContent::AnthropicBlocks(blocks.clone())
            } else if let Some(parts) = source
                .get("parts")
                .and_then(Value::as_array)
                .filter(|parts| gemini_replayable(parts))
            {
                ReplayContent::GeminiContent(serde_json::json!({"role":"model","parts":parts}))
            } else {
                return None;
            };
        Some(Self {
            scope: scope.into(),
            model: model.into(),
            content,
        })
    }

    fn for_scope(&self, scope: &str, model: &str) -> Option<&ReplayContent> {
        (self.scope == scope && self.model == model).then_some(&self.content)
    }

    pub(crate) fn chat_reasoning(&self, scope: &str, model: &str) -> Option<&str> {
        match self.for_scope(scope, model)? {
            ReplayContent::ChatReasoning(reasoning) => Some(reasoning),
            _ => None,
        }
    }

    pub(crate) fn responses_items(&self, scope: &str, model: &str) -> Option<&[Value]> {
        match self.for_scope(scope, model)? {
            ReplayContent::ResponsesItems(items) => Some(items),
            _ => None,
        }
    }

    pub(crate) fn anthropic_blocks(&self, scope: &str, model: &str) -> Option<&[Value]> {
        match self.for_scope(scope, model)? {
            ReplayContent::AnthropicBlocks(blocks) => Some(blocks),
            _ => None,
        }
    }

    pub(crate) fn gemini_content(&self, scope: &str, model: &str) -> Option<&Value> {
        match self.for_scope(scope, model)? {
            ReplayContent::GeminiContent(content) => Some(content),
            _ => None,
        }
    }
}

fn anthropic_replayable(blocks: &[Value]) -> bool {
    let kinds = || blocks.iter().map(|block| block["type"].as_str());
    kinds().any(|kind| matches!(kind, Some("thinking" | "redacted_thinking")))
        && kinds().all(|kind| {
            matches!(
                kind,
                Some("text" | "tool_use" | "thinking" | "redacted_thinking")
            )
        })
}

fn gemini_replayable(parts: &[Value]) -> bool {
    parts.iter().any(|part| part.get("functionCall").is_some())
        && parts.iter().all(|part| {
            part.as_object().is_some_and(|fields| {
                fields.keys().all(|key| {
                    matches!(
                        key.as_str(),
                        "text" | "functionCall" | "thought" | "thoughtSignature"
                    )
                })
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn provider_replay_omits_native_media_blocks() {
        let anthropic = json!([
            {"type":"thinking","thinking":"example","signature":"fixture"},
            {"type":"tool_use","id":"call","name":"lookup","input":{}},
            {"type":"image","source":{"type":"base64","data":"AA=="}}
        ]);
        let gemini = json!({"parts":[
            {"functionCall":{"name":"lookup","args":{}},"thoughtSignature":"fixture"},
            {"inlineData":{"mimeType":"image/png","data":"AA=="}}
        ]});
        for content in [&anthropic, &gemini] {
            assert!(ProviderReplay::capture(Some("scope"), "model", Some(content)).is_none());
        }
    }
}
