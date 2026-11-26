use std::sync::Arc;

use openrust::{http::HttpFuture, support::Str};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::{
    AgentPrompt, AgentResponse, AiError, AiResult, Attachment, EmbeddingProvider, EmbeddingsPrompt,
    EmbeddingsResponse, FinishReason, MessageRole, ModelCatalog, StepResponse, StepTextGateway,
    TextModelTier, TextPrompt, TextProvider, ToolCall, ToolChoice, ToolDefinition, ToolResult,
    ToolStep, Usage, connection::Connection, embeddings::valid_vectors, models::ListedModel,
    text::validate,
};

pub(crate) struct OllamaProvider(pub(crate) Arc<Connection>);

impl OllamaProvider {
    fn chat_body(
        prompt: &AgentPrompt<'_>,
        tools: &[ToolDefinition],
        steps: &[ToolStep],
    ) -> AiResult<Value> {
        prompt.options.validate()?;
        let mut messages = Vec::new();
        if !prompt.text.instructions.trim().is_empty() {
            messages.push(json!({"role":"system", "content":prompt.text.instructions}));
        }
        for message in prompt.messages {
            match message.role {
                MessageRole::User => {
                    messages.push(user_message(&message.content, &message.attachments)?)
                }
                MessageRole::Assistant => {
                    messages.push(assistant_message(&message.content, &message.tool_calls))
                }
                MessageRole::ToolResult => {
                    messages.extend(message.tool_results.iter().map(tool_message))
                }
            }
        }
        messages.push(user_message(prompt.text.prompt, prompt.attachments)?);
        for step in steps {
            messages.push(assistant_message(
                step.text.as_deref().unwrap_or_default(),
                &step.calls,
            ));
            messages.extend(step.results.iter().map(tool_message));
        }
        let mut body = json!({"model":prompt.text.model, "messages":messages, "stream":false});
        if !tools.is_empty() {
            match prompt.tool_choice {
                None | Some(ToolChoice::Auto) => {
                    body["tools"] = Value::Array(tools.iter().map(|tool| json!({
                        "type":"function",
                        "function":{"name":tool.name,"description":tool.description,"parameters":tool.schema},
                    })).collect());
                }
                Some(ToolChoice::None) => {}
                _ => return Err(AiError::InvalidToolChoice),
            }
        }
        if let Some(output) = prompt.structured_output {
            body["format"] = output.object_schema();
        }
        let mut options = Map::new();
        for (key, value) in [
            ("temperature", json!(prompt.options.temperature)),
            ("top_p", json!(prompt.options.top_p)),
            ("num_predict", json!(prompt.options.max_tokens)),
        ] {
            if !value.is_null() {
                options.insert(key.into(), value);
            }
        }
        if let Some(overrides) = prompt.provider_options {
            for (key, value) in overrides {
                match key.as_str() {
                    "format" if body.get("format").is_some() => {}
                    "format" | "keep_alive" | "think" | "logprobs" | "top_logprobs" => {
                        body[key] = value.clone();
                    }
                    "model" | "messages" | "stream" | "tools" => {
                        return Err(AiError::InvalidProviderOptions);
                    }
                    _ => {
                        options.insert(key.clone(), value.clone());
                    }
                }
            }
        }
        if !options.is_empty() {
            body["options"] = Value::Object(options);
        }
        Ok(body)
    }

    async fn discover(&self) -> AiResult<Vec<ListedModel>> {
        #[derive(Deserialize)]
        struct Catalog {
            models: Vec<Model>,
        }
        #[derive(Deserialize)]
        struct Model {
            model: Option<String>,
            name: Option<String>,
        }
        let response = validate(self.0.request(None)?.get("api/tags").await)?;
        let payload: Catalog = response.json().map_err(|_| AiError::InvalidResponse)?;
        Ok(payload
            .models
            .into_iter()
            .filter_map(|model| {
                let id = model
                    .model
                    .or_else(|| model.name.clone())
                    .filter(|id| !id.trim().is_empty())?;
                Some(ListedModel {
                    id,
                    name: model.name,
                    input_modalities: None,
                    output_modalities: None,
                })
            })
            .collect())
    }
}

impl TextProvider for OllamaProvider {
    fn name(&self) -> &str {
        self.0.name()
    }

    fn default_text_model(&self) -> AiResult<&str> {
        self.0.default_text_model()
    }

    fn text_model(&self, tier: TextModelTier) -> AiResult<&str> {
        self.0.text_model(tier)
    }

    fn generate_text<'a>(
        &'a self,
        prompt: TextPrompt<'a>,
    ) -> HttpFuture<'a, AiResult<AgentResponse>> {
        self.generate(prompt.into())
    }

    fn generate<'a>(&'a self, prompt: AgentPrompt<'a>) -> HttpFuture<'a, AiResult<AgentResponse>> {
        Box::pin(async move {
            let model = prompt.text.model;
            let step = self.generate_text_step(&prompt, &[], &[]).await?;
            if step.text.as_deref().is_none_or(str::is_empty) && step.tool_calls.is_empty() {
                return Err(AiError::InvalidResponse);
            }
            let public = step.public_step(TextProvider::name(self), model);
            let mut response = AgentResponse::new(
                public.text.clone(),
                step.usage,
                TextProvider::name(self),
                step.model.unwrap_or_else(|| model.into()),
            );
            response.reasoning = step.reasoning;
            response.tool_calls = step.tool_calls;
            response.steps.push(public);
            Ok(response)
        })
    }

    fn models(&self) -> HttpFuture<'_, ModelCatalog> {
        Box::pin(async { self.0.catalog(self.discover().await) })
    }

    fn embedding_provider(&self) -> Option<Arc<dyn EmbeddingProvider>> {
        Some(Arc::new(Self(self.0.clone())))
    }

    fn step_text_gateway(&self) -> Option<&dyn StepTextGateway> {
        Some(self)
    }
}

impl StepTextGateway for OllamaProvider {
    fn approval_replay_scope(&self) -> AiResult<Option<String>> {
        self.0.request_namespace("ollama-chat").map(Some)
    }

    fn generate_text_step<'a>(
        &'a self,
        prompt: &'a AgentPrompt<'a>,
        tools: &'a [ToolDefinition],
        history: &'a [ToolStep],
    ) -> HttpFuture<'a, AiResult<StepResponse>> {
        Box::pin(async move {
            let body = Self::chat_body(prompt, tools, history)?;
            let response = validate(
                self.0
                    .request(prompt.text.timeout)?
                    .distinguish_connection_setup_failure()
                    .post("api/chat", &body)
                    .await,
            )?;
            parse_step(response.json().map_err(|_| AiError::InvalidResponse)?)
        })
    }
}

impl EmbeddingProvider for OllamaProvider {
    fn name(&self) -> &str {
        self.0.name()
    }

    fn default_embeddings_model(&self) -> AiResult<&str> {
        self.0.default_embeddings_model()
    }

    fn default_embeddings_dimensions(&self) -> Option<usize> {
        self.0.default_embeddings_dimensions()
    }

    fn embeddings_cache_namespace(&self) -> AiResult<Option<String>> {
        self.0.request_namespace("ollama-embed").map(Some)
    }

    fn embeddings<'a>(
        &'a self,
        prompt: EmbeddingsPrompt<'a>,
    ) -> HttpFuture<'a, AiResult<EmbeddingsResponse>> {
        Box::pin(async move {
            #[derive(Deserialize)]
            struct Payload {
                embeddings: Vec<Vec<f32>>,
                model: Option<String>,
                prompt_eval_count: Option<u64>,
            }
            if prompt.inputs.is_empty() || prompt.inputs.iter().any(|input| input.trim().is_empty())
            {
                return Err(AiError::InvalidPrompt);
            }
            if prompt.dimensions == Some(0) {
                return Err(AiError::InvalidConfiguration("embeddings.dimensions"));
            }
            let mut body = json!({"model":prompt.model,"input":prompt.inputs});
            if let Some(dimensions) = prompt.dimensions {
                body["dimensions"] = dimensions.into();
            }
            let response = validate(
                self.0
                    .request(Some(prompt.timeout))?
                    .post("api/embed", &body)
                    .await,
            )?;
            let value: Payload = response.json().map_err(|_| AiError::InvalidResponse)?;
            if !valid_vectors(&value.embeddings, prompt.inputs.len(), prompt.dimensions) {
                return Err(AiError::InvalidResponse);
            }
            Ok(EmbeddingsResponse {
                embeddings: value.embeddings,
                usage: Usage {
                    prompt_tokens: value.prompt_eval_count,
                    ..Usage::default()
                },
                provider: EmbeddingProvider::name(self).into(),
                model: value.model.unwrap_or_else(|| prompt.model.into()),
            })
        })
    }
}

fn user_message(text: &str, attachments: &[Attachment]) -> AiResult<Value> {
    let mut message = json!({"role":"user","content":text});
    if !attachments.is_empty() {
        let images = attachments
            .iter()
            .map(|attachment| match attachment {
                Attachment::Image(image) => Ok(Str::to_base64(image.content())),
                Attachment::Document(_) => Err(AiError::AttachmentsNotSupported),
            })
            .collect::<AiResult<Vec<_>>>()?;
        message["images"] = json!(images);
    }
    Ok(message)
}

fn assistant_message(text: &str, calls: &[ToolCall]) -> Value {
    let mut message = json!({"role":"assistant","content":text});
    if !calls.is_empty() {
        message["tool_calls"] = Value::Array(
            calls
                .iter()
                .map(|call| {
                    json!({
                        "function":{"name":call.name,"arguments":call.arguments},
                    })
                })
                .collect(),
        );
    }
    message
}

fn tool_message(result: &ToolResult) -> Value {
    json!({"role":"tool","tool_name":result.name,"content":result.text()})
}

fn parse_step(value: Value) -> AiResult<StepResponse> {
    if value["done"] != true || value.get("error").is_some_and(|error| !error.is_null()) {
        return Err(AiError::InvalidResponse);
    }
    let message = &value["message"];
    if !message.is_object() {
        return Err(AiError::InvalidResponse);
    }
    let calls = match message.get("tool_calls") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(calls)) => calls
            .iter()
            .map(|call| {
                let function = &call["function"];
                let name = function["name"]
                    .as_str()
                    .filter(|name| !name.trim().is_empty())
                    .ok_or(AiError::InvalidResponse)?
                    .to_owned();
                let arguments = match function.get("arguments") {
                    None | Some(Value::Null) => Map::new(),
                    Some(Value::Object(arguments)) => arguments.clone(),
                    Some(Value::String(arguments)) => serde_json::from_str(arguments)
                        .map_err(|_| AiError::InvalidToolArguments)?,
                    _ => return Err(AiError::InvalidToolArguments),
                };
                let result_id = call["id"]
                    .as_str()
                    .filter(|id| !id.trim().is_empty())
                    .map(str::to_owned);
                let id = result_id
                    .clone()
                    .unwrap_or_else(|| Str::uuid7().to_string());
                Ok(ToolCall {
                    id,
                    name,
                    arguments,
                    result_id,
                })
            })
            .collect::<AiResult<Vec<_>>>()?,
        _ => return Err(AiError::InvalidResponse),
    };
    let finish_reason = if calls.is_empty() {
        FinishReason::from_wire(value["done_reason"].as_str())
    } else {
        FinishReason::ToolCalls
    };
    Ok(StepResponse {
        model: value["model"].as_str().map(str::to_owned),
        text: message["content"].as_str().map(str::to_owned),
        finish_reason,
        reasoning: message["thinking"].as_str().unwrap_or_default().into(),
        usage: Usage {
            prompt_tokens: value["prompt_eval_count"].as_u64(),
            completion_tokens: value["eval_count"].as_u64(),
            cache_read_input_tokens: value["prompt_eval_cached_count"].as_u64(),
            ..Usage::default()
        },
        citations: Vec::new(),
        tool_calls: calls,
        provider_content: None,
    })
}
