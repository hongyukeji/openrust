use std::sync::Arc;

use openrust::{
    encryption::{Crypt, Encrypter},
    support::{Str, facades::Facade},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::{AiError, AiResult, StepTextGateway, ToolCall};

use super::{BedrockProvider, tools};

const MAX_TOKEN_BYTES: usize = 128 * 1024;
const TTL_SECONDS: u64 = 3600;

pub(super) struct ChatReplay {
    cipher: Arc<Encrypter>,
    scope: String,
}

#[derive(Serialize, Deserialize)]
struct SignedTurn {
    scope: String,
    history: String,
    assistant: String,
    expires_at: u64,
    blocks: Vec<Value>,
}

impl ChatReplay {
    pub(super) fn new(
        provider: &BedrockProvider,
        downstream_scope: Option<&str>,
        model: Option<&str>,
    ) -> Option<Self> {
        let scope = digest(json!([
            "openrust-bedrock-chat-v1",
            downstream_scope?,
            provider.approval_replay_scope().ok()??,
            model?,
        ]));
        Some(Self {
            cipher: Crypt::get_facade_root().ok()?,
            scope,
        })
    }

    pub(super) fn seal(
        &self,
        history: String,
        text: &str,
        calls: &[ToolCall],
        blocks: &[Value],
        now: u64,
    ) -> AiResult<String> {
        let turn = SignedTurn {
            scope: self.scope.clone(),
            history,
            assistant: projection(text, calls),
            expires_at: now.saturating_add(TTL_SECONDS),
            blocks: blocks.to_vec(),
        };
        let raw = serde_json::to_vec(&turn).map_err(|_| AiError::InvalidResponse)?;
        if raw.len() > MAX_TOKEN_BYTES {
            return Err(AiError::InvalidResponse);
        }
        let encrypted = self
            .cipher
            .encrypt_string(&raw)
            .map_err(|_| AiError::InvalidResponse)?;
        if encrypted.len() > MAX_TOKEN_BYTES {
            return Err(AiError::InvalidResponse);
        }
        Ok(encrypted)
    }

    pub(super) fn restore(
        &self,
        token: &Value,
        prefix: &[Value],
        text: &str,
        calls: &[ToolCall],
        now: u64,
    ) -> AiResult<super::BedrockMessage> {
        let token = token
            .as_str()
            .filter(|token| token.len() <= MAX_TOKEN_BYTES)
            .ok_or(AiError::InvalidPrompt)?;
        Str::from_base64(token).map_err(|_| AiError::InvalidPrompt)?;
        let raw = self
            .cipher
            .decrypt_string(token)
            .map_err(|_| AiError::InvalidPrompt)?;
        let turn: SignedTurn = serde_json::from_slice(&raw).map_err(|_| AiError::InvalidPrompt)?;
        if turn.scope != self.scope
            || turn.expires_at <= now
            || turn.history != history(prefix)
            || turn.assistant != projection(text, calls)
        {
            return Err(AiError::InvalidPrompt);
        }
        tools::assistant(text, calls, Some(&turn.blocks))
    }
}

pub(super) fn history(messages: &[Value]) -> String {
    digest(json!(messages))
}

fn projection(text: &str, calls: &[ToolCall]) -> String {
    digest(json!([text, calls]))
}

fn digest(mut value: Value) -> String {
    value.sort_all_objects();
    format!("{:x}", Sha256::digest(value.to_string()))
}

pub(super) fn private_blocks(content: Option<&Value>) -> Option<&[Value]> {
    let blocks = content?.get("bedrock_blocks")?.as_array()?;
    blocks
        .iter()
        .any(|block| {
            block.get("signed_reasoning").is_some() || block.get("redacted_reasoning").is_some()
        })
        .then_some(blocks.as_slice())
}
