use openrust::{
    http::HttpFuture,
    support::{Str, Stringable},
};

use crate::{Agent, AiResult, PromptOptions, TextModelTier};

/// 通过已配置的经济型文本模型生成摘要。
pub struct SummarizeAgent {
    instructions: String,
}

impl SummarizeAgent {
    pub fn new(sentences: usize) -> Self {
        let sentences = sentences.max(1);
        let unit = if sentences == 1 {
            "sentence"
        } else {
            "sentences"
        };
        Self {
            instructions: format!(
                "Summarize the given text in no more than {sentences} {unit}. Respond with only the summary and nothing else."
            ),
        }
    }
}

impl Default for SummarizeAgent {
    fn default() -> Self {
        Self::new(3)
    }
}

impl Agent for SummarizeAgent {
    fn instructions(&self) -> &str {
        &self.instructions
    }

    fn model_tier(&self) -> TextModelTier {
        TextModelTier::Cheapest
    }
}

pub trait Summarizes {
    fn summarize(&self) -> HttpFuture<'_, AiResult<String>> {
        self.summarize_with(3, PromptOptions::default())
    }

    fn summarize_with<'a>(
        &'a self,
        sentences: usize,
        options: PromptOptions<'a>,
    ) -> HttpFuture<'a, AiResult<String>>;
}

impl Summarizes for str {
    fn summarize_with<'a>(
        &'a self,
        sentences: usize,
        options: PromptOptions<'a>,
    ) -> HttpFuture<'a, AiResult<String>> {
        Box::pin(async move {
            Ok(SummarizeAgent::new(sentences)
                .prompt_with(self, options)
                .await?
                .text)
        })
    }
}

impl Summarizes for Stringable {
    fn summarize_with<'a>(
        &'a self,
        sentences: usize,
        options: PromptOptions<'a>,
    ) -> HttpFuture<'a, AiResult<String>> {
        self.value().summarize_with(sentences, options)
    }
}

pub trait SummarizesText {
    fn summarize(value: &str) -> HttpFuture<'_, AiResult<String>> {
        Self::summarize_with(value, 3, PromptOptions::default())
    }

    fn summarize_with<'a>(
        value: &'a str,
        sentences: usize,
        options: PromptOptions<'a>,
    ) -> HttpFuture<'a, AiResult<String>>;
}

impl SummarizesText for Str {
    fn summarize_with<'a>(
        value: &'a str,
        sentences: usize,
        options: PromptOptions<'a>,
    ) -> HttpFuture<'a, AiResult<String>> {
        value.summarize_with(sentences, options)
    }
}
