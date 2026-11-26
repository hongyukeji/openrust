use std::{collections::BTreeMap, sync::Arc};

use openrust::{http::HttpFuture, support::Str};
use serde_json::{Map, Value, json};

use crate::{
    AgentResponse, AiError, AiResult, ImageAttachment, ImageFormat, TextPrompt, TextProvider,
    Usage,
    connection::Connection,
    text::{transport_error, validate},
};

use super::{
    Answer, ClassificationPrompt, ClassificationProvider, ClassificationResponse,
    ClassificationUsage, Question, answers, validate_prompt,
};

#[derive(Clone, Copy)]
pub(crate) enum DecisionsProtocol {
    SystemOne,
    OpenRouter,
    Gemini,
}

#[derive(Clone)]
pub(crate) struct HttpDecisionsProvider {
    pub(crate) connection: Arc<Connection>,
    pub(crate) protocol: DecisionsProtocol,
}

impl TextProvider for HttpDecisionsProvider {
    fn name(&self) -> &str {
        self.connection.name()
    }

    fn default_text_model(&self) -> AiResult<&str> {
        Err(AiError::EndpointNotSupported)
    }

    fn generate_text<'a>(&'a self, _: TextPrompt<'a>) -> HttpFuture<'a, AiResult<AgentResponse>> {
        Box::pin(async { Err(AiError::EndpointNotSupported) })
    }

    fn classification_provider(&self) -> Option<Arc<dyn ClassificationProvider>> {
        Some(Arc::new(self.clone()))
    }
}

impl ClassificationProvider for HttpDecisionsProvider {
    fn name(&self) -> &str {
        self.connection.name()
    }

    fn default_classification_model(&self) -> AiResult<&str> {
        self.connection.default_classification_model()
    }

    fn classify<'a>(
        &'a self,
        prompt: ClassificationPrompt<'a>,
    ) -> HttpFuture<'a, AiResult<ClassificationResponse>> {
        Box::pin(async move {
            validate_prompt(&prompt)?;
            let protocol = self.protocol;
            let mut body = protocol.payload(&prompt)?;
            let mut request = self.connection.request(Some(prompt.timeout))?;
            let endpoint = match protocol {
                DecisionsProtocol::SystemOne => {
                    if !prompt.attachments.is_empty() {
                        request = request.with_header(
                            "typesafe-danger-zone-images"
                                .parse()
                                .expect("valid header name"),
                            "true".parse().expect("valid header value"),
                        );
                    }
                    "systemone"
                }
                DecisionsProtocol::OpenRouter => {
                    let mut base = url::Url::parse(self.connection.base_url())
                        .map_err(|_| AiError::InvalidConfiguration("providers.<name>.url"))?;
                    let path = base.path().trim_end_matches('/');
                    if let Some(path) = path.strip_suffix("/v1").map(str::to_owned) {
                        base.set_path(&path);
                        request = request.base_url(base.as_str());
                    }
                    "alpha/decisions"
                }
                DecisionsProtocol::Gemini => "decisions",
            };
            body["model"] = prompt.model.into();
            let response = request
                .post(endpoint, &body)
                .await
                .map_err(transport_error)?;
            let data: Value = validate(Ok(response))?
                .json()
                .map_err(|_| AiError::InvalidResponse)?;
            let routed = match protocol {
                DecisionsProtocol::SystemOne => data["routing"]["model"].as_str(),
                _ => None,
            };
            Ok(ClassificationResponse {
                answers: decode_keyed(prompt.questions, &data["answers"])?,
                provider: self.connection.name().into(),
                model: routed
                    .or(data["model"].as_str())
                    .unwrap_or(prompt.model)
                    .into(),
                usage: protocol.usage(&data["usage"]),
            })
        })
    }
}

impl DecisionsProtocol {
    fn payload(self, prompt: &ClassificationPrompt<'_>) -> AiResult<Value> {
        let gemini = matches!(self, Self::Gemini);
        let questions: Map<_, _> = prompt
            .questions
            .iter()
            .map(|(name, question)| (name.clone(), keyed_question(question, gemini)))
            .collect();
        match self {
            Self::SystemOne => {
                let (state, assets) = system_one_state(prompt.state, prompt.attachments)?;
                let mut body = json!({"state":state,"questions":questions});
                if !assets.is_empty() {
                    body["assets"] = assets.into();
                }
                Ok(body)
            }
            Self::OpenRouter => {
                let state = if prompt.attachments.is_empty() {
                    prompt.state.clone()
                } else {
                    let mut state = state_parts(prompt.state);
                    state.extend(prompt.attachments.iter().map(|image| json!({
                        "type":"image_url","image_url":{"url":format!(
                            "data:{};base64,{}", image.format().mime_type(), Str::to_base64(image.content())
                        )}
                    })));
                    state.into()
                };
                Ok(json!({"state":state,"questions":questions}))
            }
            Self::Gemini => {
                let text = prompt
                    .state
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| prompt.state.to_string());
                let input: Value = if prompt.attachments.is_empty() {
                    text.into()
                } else {
                    let mut parts = vec![json!({"type":"text","text":text})];
                    parts.extend(prompt.attachments.iter().map(|image| json!({
                        "type":"image","data":Str::to_base64(image.content()),"mime_type":image.format().mime_type()
                    })));
                    parts.into()
                };
                Ok(json!({"input":input,"questions":questions}))
            }
        }
    }

    fn usage(self, usage: &Value) -> ClassificationUsage {
        let (input, output, cached) = if matches!(self, Self::Gemini) {
            (
                "total_input_tokens",
                "total_output_tokens",
                usage["total_cached_tokens"].as_u64(),
            )
        } else {
            ("input_tokens", "output_tokens", None)
        };
        ClassificationUsage {
            tokens: Usage {
                prompt_tokens: usage[input].as_u64(),
                completion_tokens: usage[output].as_u64(),
                ..Usage::default()
            },
            cached_input_tokens: cached,
            cache_write_input_tokens: None,
            reasoning_tokens: None,
        }
    }
}

fn keyed_question(question: &Question, gemini: bool) -> Value {
    match question {
        Question::Boolean(question) => {
            let fields = if gemini {
                ["true_description", "false_description"]
            } else {
                ["true", "false"]
            };
            let criteria: Map<_, _> = fields
                .into_iter()
                .zip(question.criteria())
                .filter_map(|(name, value)| value.map(|value| (name.into(), value.into())))
                .collect();
            let mut result = json!({"type":"noul","instructions":question.instructions()});
            if !criteria.is_empty() {
                result["criteria"] = criteria.into();
            }
            result
        }
        Question::Choice(question) => {
            let criteria: Map<_, _> = question
                .options()
                .iter()
                .map(|option| (option.value.clone(), json!(option.description)))
                .collect();
            json!({"type":"choice","instructions":question.instructions(),"criteria":criteria})
        }
        Question::Score(question) => {
            let criteria: Vec<_> = question
                .levels()
                .iter()
                .map(|level| level.description.as_deref().unwrap_or(&level.label))
                .collect();
            json!({"type":"score","instructions":question.instructions(),"criteria":criteria})
        }
    }
}

fn state_parts(state: &Value) -> Vec<Value> {
    state
        .as_array()
        .cloned()
        .unwrap_or_else(|| vec![state.clone()])
}

fn system_one_state(
    state: &Value,
    images: &[ImageAttachment],
) -> AiResult<(Value, Map<String, Value>)> {
    let mut assets = Map::new();
    if images.is_empty() {
        return Ok((state.clone(), assets));
    }
    if state.get("images").is_some() {
        return Err(AiError::InvalidClassification);
    }
    let mut ids = Vec::new();
    for image in images {
        let format = match image.format() {
            ImageFormat::Png => "png",
            ImageFormat::Jpeg => "jpg",
            _ => return Err(AiError::InvalidImage),
        };
        let id = Str::uuid()
            .map_err(|_| AiError::InvalidClassification)?
            .to_string();
        assets.insert(
            id.clone(),
            json!({
                "type":"image","format":format,"data_base64":Str::to_base64(image.content())
            }),
        );
        ids.push(Value::String(id));
    }
    let state = if state.is_object() {
        let mut state = state.clone();
        state["images"] = ids.into();
        state
    } else {
        let mut parts = state_parts(state);
        parts.extend(ids);
        parts.into()
    };
    Ok((state, assets))
}

fn decode_keyed(
    questions: &[(String, Question)],
    data: &Value,
) -> AiResult<BTreeMap<String, Answer>> {
    let questions: BTreeMap<_, _> = questions
        .iter()
        .map(|(name, question)| (name, question))
        .collect();
    data.as_object()
        .ok_or(AiError::InvalidResponse)?
        .iter()
        .map(|(name, data)| {
            let question = questions.get(name).ok_or(AiError::InvalidResponse)?;
            let mut normalized = data.clone();
            match data["type"].as_str() {
                Some("noul") => {
                    normalized["type"] = "predicate".into();
                    normalized["probability"] = data["noul"].clone();
                }
                Some(kind @ ("choice" | "score")) if !data["probabilities"].is_null() => {
                    let entries = data["probabilities"]
                        .as_object()
                        .ok_or(AiError::InvalidResponse)?;
                    let mut probabilities = Vec::new();
                    for (key, probability) in entries {
                        let value = if kind == "score" {
                            let index =
                                key.parse::<usize>().map_err(|_| AiError::InvalidResponse)?;
                            json!(index)
                        } else {
                            json!(key)
                        };
                        probabilities.push(json!({"value":value,"probability":probability}));
                    }
                    normalized["probabilities"] = probabilities.into();
                }
                _ => {}
            }
            Ok((name.clone(), answers::decode(question, &normalized)?))
        })
        .collect()
}
