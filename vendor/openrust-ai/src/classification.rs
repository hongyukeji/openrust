use std::{
    collections::{BTreeMap, HashSet},
    sync::Arc,
    time::Duration,
};

use openrust::{http::HttpFuture, support::Str};
use serde::Serialize;
use serde_json::{Value, json};

use crate::{
    Ai, AiError, AiResult, ImageAttachment, Usage,
    connection::Connection,
    text::{transport_error, validate},
};

mod answers;
mod decide;
mod decisions;
mod fake;
mod questions;

pub use answers::{Answer, BooleanAnswer, ChoiceAnswer, ScoreAnswer};
pub use decide::{DecideOptions, Decides, DecidesBetween, DecidesText};
pub(crate) use decisions::{DecisionsProtocol, HttpDecisionsProvider};
pub use fake::{FakeClassificationGateway, FakeClassificationResult, RecordedClassification};
pub use questions::{Boolean, Choice, ChoiceOption, Question, Score, ScoreLevel};

pub struct Classification;

impl Classification {
    pub fn of(state: impl Into<Value>) -> AiResult<PendingClassification> {
        let state = state.into();
        if !valid_state(&state) {
            return Err(AiError::InvalidClassification);
        }
        Ok(PendingClassification {
            state,
            questions: Vec::new(),
            attachments: Vec::new(),
            timeout: Duration::from_secs(30),
        })
    }

    pub fn fake(
        responses: impl IntoIterator<Item = FakeClassificationResult>,
    ) -> AiResult<Arc<FakeClassificationGateway>> {
        Ai::fake_classification(responses)
    }

    pub fn fake_with(
        resolve: impl Fn(&RecordedClassification) -> AiResult<FakeClassificationResult>
        + Send
        + Sync
        + 'static,
    ) -> AiResult<Arc<FakeClassificationGateway>> {
        Ok(Ai::manager()?.fake_classification_with(resolve))
    }

    pub fn is_faked() -> AiResult<bool> {
        Ai::classification_is_faked()
    }

    #[track_caller]
    pub fn assert_classified(predicate: impl Fn(&RecordedClassification) -> bool) -> AiResult<()> {
        Self::fake_gateway()?.assert_classified(predicate);
        Ok(())
    }

    #[track_caller]
    pub fn assert_not_classified(
        predicate: impl Fn(&RecordedClassification) -> bool,
    ) -> AiResult<()> {
        Self::fake_gateway()?.assert_not_classified(predicate);
        Ok(())
    }

    #[track_caller]
    pub fn assert_nothing_classified() -> AiResult<()> {
        Self::fake_gateway()?.assert_nothing_classified();
        Ok(())
    }

    fn fake_gateway() -> AiResult<Arc<FakeClassificationGateway>> {
        Ai::manager()?
            .classification_fake()
            .ok_or(AiError::Unavailable)
    }
}

pub struct PendingClassification {
    state: Value,
    questions: Vec<(String, Question)>,
    attachments: Vec<ImageAttachment>,
    timeout: Duration,
}

impl PendingClassification {
    pub fn question(mut self, key: impl Into<String>, question: impl Into<Question>) -> Self {
        let key = key.into();
        let question = question.into();
        if let Some((_, active)) = self.questions.iter_mut().find(|(name, _)| *name == key) {
            *active = question;
        } else {
            self.questions.push((key, question));
        }
        self
    }

    pub fn questions<K: Into<String>, Q: Into<Question>>(
        mut self,
        questions: impl IntoIterator<Item = (K, Q)>,
    ) -> Self {
        for (key, question) in questions {
            self = self.question(key, question);
        }
        self
    }

    pub fn attachments(mut self, images: impl IntoIterator<Item = ImageAttachment>) -> Self {
        self.attachments = images.into_iter().collect();
        self
    }

    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub async fn classify(self) -> AiResult<ClassificationResponse> {
        self.classify_with(None, None).await
    }

    pub async fn classify_with(
        self,
        provider: Option<&str>,
        model: Option<&str>,
    ) -> AiResult<ClassificationResponse> {
        if self.questions.is_empty() || self.timeout.is_zero() {
            return Err(AiError::InvalidClassification);
        }
        if model.is_some_and(|model| model.trim().is_empty()) {
            return Err(AiError::MissingModel);
        }
        validate_questions(&self.questions)?;
        let manager = Ai::manager()?;
        if let Some(fake) = manager.classification_fake() {
            let model = model.map(str::to_owned).or_else(|| {
                manager
                    .classification_provider(provider)
                    .ok()
                    .and_then(|provider| {
                        provider
                            .default_classification_model()
                            .ok()
                            .map(str::to_owned)
                    })
            });
            return fake.respond(RecordedClassification {
                state: self.state,
                questions: self.questions,
                attachments: self.attachments,
                timeout: self.timeout,
                provider: manager.classification_provider_name(provider).ok(),
                model,
            });
        }
        let provider = manager.classification_provider(provider)?;
        let model = match model {
            Some(model) => model,
            None => provider.default_classification_model()?,
        };
        provider
            .classify(ClassificationPrompt {
                state: &self.state,
                questions: &self.questions,
                attachments: &self.attachments,
                model,
                timeout: self.timeout,
            })
            .await
    }
}

pub struct ClassificationPrompt<'a> {
    pub state: &'a Value,
    pub questions: &'a [(String, Question)],
    pub attachments: &'a [ImageAttachment],
    pub model: &'a str,
    pub timeout: Duration,
}

pub trait ClassificationProvider: Send + Sync {
    fn name(&self) -> &str;
    fn default_classification_model(&self) -> AiResult<&str>;
    fn classify<'a>(
        &'a self,
        prompt: ClassificationPrompt<'a>,
    ) -> HttpFuture<'a, AiResult<ClassificationResponse>>;
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct ClassificationUsage {
    #[serde(flatten)]
    pub tokens: Usage,
    pub cached_input_tokens: Option<u64>,
    pub cache_write_input_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ClassificationResponse {
    pub answers: BTreeMap<String, Answer>,
    pub usage: ClassificationUsage,
    pub provider: String,
    pub model: String,
}

impl ClassificationResponse {
    pub fn answer(&self, key: &str) -> Option<&Answer> {
        self.answers.get(key)
    }
}

pub(crate) struct OpenAiClassificationProvider(pub(crate) Arc<Connection>);

impl ClassificationProvider for OpenAiClassificationProvider {
    fn name(&self) -> &str {
        self.0.name()
    }

    fn default_classification_model(&self) -> AiResult<&str> {
        self.0.default_classification_model()
    }

    fn classify<'a>(
        &'a self,
        prompt: ClassificationPrompt<'a>,
    ) -> HttpFuture<'a, AiResult<ClassificationResponse>> {
        Box::pin(async move {
            validate_prompt(&prompt)?;
            let definitions: Vec<_> = prompt
                .questions
                .iter()
                .map(|(name, question)| question.definition(name))
                .collect();
            let input = decision_input(prompt.state, prompt.attachments);
            let body = json!({"model":prompt.model,"input":input,"questions":definitions});
            let response = self
                .0
                .request(Some(prompt.timeout))?
                .post("decisions", &body)
                .await
                .map_err(transport_error)?;
            let data: Value = validate(Ok(response))?
                .json()
                .map_err(|_| AiError::InvalidResponse)?;
            let answers = decode_answers(prompt.questions, &data["answers"])?;
            let usage = &data["usage"];
            Ok(ClassificationResponse {
                answers,
                provider: self.name().into(),
                model: data["model"].as_str().unwrap_or(prompt.model).into(),
                usage: ClassificationUsage {
                    tokens: Usage {
                        prompt_tokens: usage["input_tokens"].as_u64(),
                        completion_tokens: usage["output_tokens"].as_u64(),
                        ..Usage::default()
                    },
                    cached_input_tokens: usage["input_tokens_details"]["cached_tokens"].as_u64(),
                    cache_write_input_tokens: usage["input_tokens_details"]["cache_write_tokens"]
                        .as_u64(),
                    reasoning_tokens: usage["output_tokens_details"]["reasoning_tokens"].as_u64(),
                },
            })
        })
    }
}

fn validate_prompt(prompt: &ClassificationPrompt<'_>) -> AiResult<()> {
    if prompt.model.trim().is_empty() {
        return Err(AiError::MissingModel);
    }
    if !valid_state(prompt.state) || prompt.questions.is_empty() || prompt.timeout.is_zero() {
        return Err(AiError::InvalidClassification);
    }
    validate_questions(prompt.questions)
}

fn validate_questions(questions: &[(String, Question)]) -> AiResult<()> {
    let mut keys = HashSet::new();
    for (name, question) in questions {
        if !keys.insert(name) {
            return Err(AiError::InvalidClassification);
        }
        question.validate(name)?;
    }
    Ok(())
}

fn valid_state(state: &Value) -> bool {
    match state {
        Value::String(text) => !text.trim().is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(fields) => !fields.is_empty(),
        _ => false,
    }
}

fn decision_input(state: &Value, attachments: &[ImageAttachment]) -> Value {
    let text = state
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| state.to_string());
    if attachments.is_empty() {
        return text.into();
    }
    let mut content = vec![json!({"type":"input_text","text":text})];
    content.extend(attachments.iter().map(|image| json!({
        "type":"input_image","image_url":format!("data:{};base64,{}",image.format().mime_type(),Str::to_base64(image.content())),
    })));
    json!([{"role":"user","content":content}])
}

fn decode_answers(
    questions: &[(String, Question)],
    data: &Value,
) -> AiResult<BTreeMap<String, Answer>> {
    let mut answers = BTreeMap::new();
    let questions: BTreeMap<_, _> = questions
        .iter()
        .map(|(name, question)| (name.as_str(), question))
        .collect();
    for result in data.as_array().ok_or(AiError::InvalidResponse)? {
        let name = result["name"].as_str().ok_or(AiError::InvalidResponse)?;
        let question = questions.get(name).ok_or(AiError::InvalidResponse)?;
        let answer = answers::decode(question, result)?;
        if answers.insert(name.into(), answer).is_some() {
            return Err(AiError::InvalidResponse);
        }
    }
    Ok(answers)
}
