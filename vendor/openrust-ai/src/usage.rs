use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_read_input_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_write_input_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_tokens: Option<u64>,
}

impl Usage {
    pub(crate) fn zero() -> Self {
        Self {
            prompt_tokens: Some(0),
            completion_tokens: Some(0),
            cache_read_input_tokens: Some(0),
            cache_write_input_tokens: Some(0),
            reasoning_tokens: Some(0),
        }
    }

    pub fn new(prompt_tokens: Option<u64>, completion_tokens: Option<u64>) -> Self {
        Self {
            prompt_tokens,
            completion_tokens,
            ..Self::default()
        }
    }

    pub fn total_tokens(&self) -> Option<u64> {
        known_sum(self.prompt_tokens, self.completion_tokens)
    }

    pub fn uncached_input_tokens(&self) -> Option<u64> {
        self.prompt_tokens.map(|tokens| {
            tokens
                .saturating_sub(self.cache_read_input_tokens.unwrap_or_default())
                .saturating_sub(self.cache_write_input_tokens.unwrap_or_default())
        })
    }

    pub(crate) fn combine(self, previous: &Self) -> Self {
        Self {
            prompt_tokens: known_sum(previous.prompt_tokens, self.prompt_tokens),
            completion_tokens: known_sum(previous.completion_tokens, self.completion_tokens),
            cache_read_input_tokens: reported_sum(
                previous.cache_read_input_tokens,
                self.cache_read_input_tokens,
            ),
            cache_write_input_tokens: reported_sum(
                previous.cache_write_input_tokens,
                self.cache_write_input_tokens,
            ),
            reasoning_tokens: reported_sum(previous.reasoning_tokens, self.reasoning_tokens),
        }
    }

    /// 同一响应的事件可只更新部分累计计数。
    pub(crate) fn update(&mut self, newer: Self) {
        self.prompt_tokens = newer.prompt_tokens.or(self.prompt_tokens);
        self.completion_tokens = newer.completion_tokens.or(self.completion_tokens);
        self.cache_read_input_tokens = newer
            .cache_read_input_tokens
            .or(self.cache_read_input_tokens);
        self.cache_write_input_tokens = newer
            .cache_write_input_tokens
            .or(self.cache_write_input_tokens);
        self.reasoning_tokens = newer.reasoning_tokens.or(self.reasoning_tokens);
    }

    pub(crate) fn from_openai(usage: &Value) -> Self {
        OpenAiUsage::deserialize(usage)
            .map(Into::into)
            .unwrap_or_default()
    }

    pub(crate) fn from_xai_responses(usage: &Value) -> Self {
        OpenAiUsage::deserialize(usage)
            .map(OpenAiUsage::into_xai_responses)
            .unwrap_or_default()
    }

    pub(crate) fn from_anthropic(usage: &Value) -> Self {
        Self {
            prompt_tokens: usage["input_tokens"].as_u64(),
            completion_tokens: usage["output_tokens"].as_u64(),
            cache_read_input_tokens: usage["cache_read_input_tokens"].as_u64(),
            cache_write_input_tokens: usage["cache_creation_input_tokens"].as_u64(),
            reasoning_tokens: usage["output_tokens_details"]["thinking_tokens"].as_u64(),
        }
    }

    pub(crate) fn from_gemini(usage: &Value) -> Self {
        Self {
            prompt_tokens: usage["promptTokenCount"].as_u64(),
            completion_tokens: usage["candidatesTokenCount"].as_u64(),
            cache_read_input_tokens: usage["cachedContentTokenCount"].as_u64(),
            reasoning_tokens: usage["thoughtsTokenCount"].as_u64(),
            ..Self::default()
        }
    }

    pub(crate) fn include_cached_input(mut self) -> Self {
        self.prompt_tokens = reported_sum(
            reported_sum(self.prompt_tokens, self.cache_read_input_tokens),
            self.cache_write_input_tokens,
        );
        self
    }

    pub(crate) fn include_reasoning_output(mut self) -> Self {
        self.completion_tokens = reported_sum(self.completion_tokens, self.reasoning_tokens);
        self
    }
}

fn known_sum(first: Option<u64>, second: Option<u64>) -> Option<u64> {
    first.zip(second).map(|(a, b)| a.saturating_add(b))
}

fn reported_sum(first: Option<u64>, second: Option<u64>) -> Option<u64> {
    match (first, second) {
        (Some(a), Some(b)) => Some(a.saturating_add(b)),
        (first, second) => first.or(second),
    }
}

#[derive(Deserialize)]
pub(crate) struct OpenAiUsage {
    #[serde(alias = "input_tokens")]
    prompt_tokens: Option<u64>,
    #[serde(alias = "output_tokens")]
    completion_tokens: Option<u64>,
    total_tokens: Option<Value>,
    #[serde(alias = "input_tokens_details")]
    prompt_tokens_details: Option<InputDetails>,
    #[serde(alias = "output_tokens_details")]
    completion_tokens_details: Option<OutputDetails>,
}

#[derive(Deserialize)]
struct InputDetails {
    cached_tokens: Option<u64>,
    cache_write_tokens: Option<u64>,
}

#[derive(Deserialize)]
struct OutputDetails {
    reasoning_tokens: Option<u64>,
}

impl OpenAiUsage {
    pub(crate) fn into_xai_responses(mut self) -> Usage {
        let total = self.total_tokens.take();
        // 固定 Laravel 基线使用分开计数。报告总数时，用它区分是否已包含推理。
        let mut usage = Usage::from(self);
        if let Some((output, reasoning)) = usage.completion_tokens.zip(usage.reasoning_tokens)
            && let Some(inclusive) = output.checked_add(reasoning)
            && total.is_none_or(|total| {
                total.as_u64().is_some_and(|total| {
                    usage
                        .prompt_tokens
                        .and_then(|input| input.checked_add(inclusive))
                        == Some(total)
                })
            })
        {
            usage.completion_tokens = Some(inclusive);
        }
        usage
    }
}

impl From<OpenAiUsage> for Usage {
    fn from(usage: OpenAiUsage) -> Self {
        let input = usage.prompt_tokens_details.as_ref();
        Self {
            prompt_tokens: usage.prompt_tokens,
            completion_tokens: usage.completion_tokens,
            cache_read_input_tokens: input.and_then(|details| details.cached_tokens),
            cache_write_input_tokens: input.and_then(|details| details.cache_write_tokens),
            reasoning_tokens: usage
                .completion_tokens_details
                .and_then(|details| details.reasoning_tokens),
        }
    }
}
