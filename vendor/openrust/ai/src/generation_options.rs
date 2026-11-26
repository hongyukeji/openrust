use serde::{Deserialize, Serialize};

use crate::{AiError, AiResult};

/// 由 Agent 声明、按当前连接编译的文本生成选项。
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TextGenerationOptions {
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
}

impl TextGenerationOptions {
    pub(crate) fn validate(self) -> AiResult<()> {
        if self.max_tokens == Some(0)
            || self
                .temperature
                .is_some_and(|value| !(0.0..=2.0).contains(&value))
            || self
                .top_p
                .is_some_and(|value| !(0.0..=1.0).contains(&value))
        {
            return Err(AiError::InvalidGenerationOptions);
        }
        Ok(())
    }
}
