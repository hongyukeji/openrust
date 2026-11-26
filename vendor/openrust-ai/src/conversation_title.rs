use openrust::support::Str;

use crate::{Agent, Ai, PromptOptions, TextModelTier};

/// 以会话已选连接的经济型文本模型，从首条消息生成标题。
#[derive(Clone, Copy, Debug, Default)]
pub struct ConversationTitleAgent;

impl Agent for ConversationTitleAgent {
    fn instructions(&self) -> &str {
        "Generate a concise 3-5 word title for a conversation that starts with the following message. Use the same language as the message. Respond with only the title, no quotes or punctuation."
    }

    fn model_tier(&self) -> TextModelTier {
        TextModelTier::Cheapest
    }
}

pub(crate) async fn title(prompt: &str, provider: &str, generate: bool, faked: bool) -> String {
    if !generate {
        return Str::limit(prompt, 50, "...", true);
    }
    let fallback = || Str::limit(prompt, 100, "...", true);
    if prompt.trim().is_empty() {
        return fallback();
    }
    let Ok(manager) = Ai::manager() else {
        return fallback();
    };
    let fake_title = manager.text_fake::<ConversationTitleAgent>().is_some();
    if faked && !fake_title {
        return fallback();
    }
    let response = manager
        .prompt(
            &ConversationTitleAgent,
            &Str::limit(prompt, 500, "...", false),
            PromptOptions {
                provider: (!faked).then_some(provider),
                ..Default::default()
            },
        )
        .await;
    match response {
        Ok(response) if !response.text.trim().is_empty() => {
            Str::limit(&response.text, 100, "...", false)
        }
        _ => fallback(),
    }
}
