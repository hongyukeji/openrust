use std::collections::HashSet;

use http::{HeaderValue, StatusCode, header};
use openrust::http::{HttpError, Request, Response};
use openrust_ai::TextEndpoint;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::gateway::{Gateway, Key};

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CodexMetadata {
    display_name: Option<String>,
    description: Option<String>,
    context_window: Option<u64>,
    #[serde(default = "text_input")]
    input_modalities: Vec<InputModality>,
    #[serde(default)]
    supported_reasoning_levels: Vec<ReasoningLevel>,
    default_reasoning_level: Option<Effort>,
}

#[derive(Clone, Copy, Eq, Hash, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum InputModality {
    Text,
    Image,
}

fn text_input() -> Vec<InputModality> {
    vec![InputModality::Text]
}

#[derive(Clone, Copy, Eq, Hash, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum Effort {
    None,
    Minimal,
    Low,
    Medium,
    High,
    Xhigh,
    Max,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ReasoningLevel {
    effort: Effort,
    description: String,
}

impl CodexMetadata {
    pub(crate) fn is_valid(&self) -> bool {
        let mut efforts = HashSet::new();
        let modalities: HashSet<_> = self.input_modalities.iter().copied().collect();
        self.display_name
            .as_ref()
            .is_none_or(|name| !name.trim().is_empty())
            && self
                .context_window
                .is_none_or(|window| (1..=i64::MAX as u64).contains(&window))
            && modalities.contains(&InputModality::Text)
            && modalities.len() == self.input_modalities.len()
            && self
                .supported_reasoning_levels
                .iter()
                .all(|level| !level.description.trim().is_empty() && efforts.insert(level.effort))
            && self
                .default_reasoning_level
                .is_none_or(|effort| efforts.contains(&effort))
    }

    fn descriptor(&self, name: &str) -> Value {
        let mut value = json!({
            "slug": name,
            "display_name": self.display_name.as_deref().unwrap_or(name),
            "description": self.description,
            "supported_in_api": true,
            "visibility": "list",
            "priority": 99,
            "base_instructions": "You are a coding assistant.",
            "shell_type": "shell_command",
            "truncation_policy": {"mode":"tokens","limit":10000},
            "supported_reasoning_levels": self.supported_reasoning_levels,
            "supports_reasoning_summary_parameter": false,
            "default_reasoning_summary": "none",
            "support_verbosity": false,
            "include_apps_usage_instructions": false,
            "experimental_supported_tools": [],
            "input_modalities": self.input_modalities,
        });
        if let Some(effort) = self.default_reasoning_level {
            value["default_reasoning_level"] = json!(effort);
        }
        if let Some(context_window) = self.context_window {
            value["context_window"] = json!(context_window);
        }
        value
    }
}

pub(crate) fn selected(request: &Request) -> Result<Option<bool>, HttpError> {
    match request.query("client_version")? {
        None | Some(Value::Null) => Ok(Some(false)),
        Some(Value::String(value)) if value.is_empty() => Ok(Some(false)),
        Some(Value::String(value))
            if value.len() <= 64
                && value.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_' | b'+')
                }) =>
        {
            Ok(Some(true))
        }
        _ => Ok(None),
    }
}

pub(crate) fn respond(
    request: &Request,
    gateway: &Gateway,
    key: &Key,
) -> Result<Response, HttpError> {
    let default = CodexMetadata {
        input_modalities: text_input(),
        ..Default::default()
    };
    let models: Vec<_> = gateway
        .models
        .iter()
        .filter(|(name, model)| {
            key.allows_model(name) && model.supports_for(TextEndpoint::Responses, key)
        })
        .map(|(name, model)| model.codex.as_ref().unwrap_or(&default).descriptor(name))
        .collect();
    let payload = json!({"models":models});
    let tag = format!("\"{:x}\"", Sha256::digest(payload.to_string().as_bytes()));
    let fresh = request
        .headers()
        .get_all(header::IF_NONE_MATCH)
        .iter()
        .any(|header| {
            header.to_str().is_ok_and(|value| {
                value.split(',').map(str::trim).any(|candidate| {
                    candidate == "*" || candidate.strip_prefix("W/").unwrap_or(candidate) == tag
                })
            })
        });
    let response = if fresh {
        Response::empty(StatusCode::NOT_MODIFIED)
    } else {
        Response::json(StatusCode::OK, payload)?
    };
    Ok(response
        .header(
            header::CACHE_CONTROL,
            HeaderValue::from_static("private, no-cache"),
        )
        .header(
            header::ETAG,
            tag.parse::<HeaderValue>().expect("valid digest header"),
        ))
}
