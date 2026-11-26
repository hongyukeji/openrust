use std::{
    collections::{BTreeMap, VecDeque},
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use serde_json::Value;

use crate::{AiError, AiResult, ImageAttachment};

use super::{Answer, BooleanAnswer, ChoiceAnswer, ClassificationResponse, Question, ScoreAnswer};

#[derive(Clone)]
pub struct RecordedClassification {
    pub state: Value,
    pub questions: Vec<(String, Question)>,
    pub attachments: Vec<ImageAttachment>,
    pub timeout: Duration,
    pub provider: Option<String>,
    pub model: Option<String>,
}

pub enum FakeClassificationResult {
    Answers(BTreeMap<String, Answer>),
    Response(ClassificationResponse),
}

impl From<BTreeMap<String, Answer>> for FakeClassificationResult {
    fn from(answers: BTreeMap<String, Answer>) -> Self {
        Self::Answers(answers)
    }
}

impl From<ClassificationResponse> for FakeClassificationResult {
    fn from(response: ClassificationResponse) -> Self {
        Self::Response(response)
    }
}

type Resolver = dyn Fn(&RecordedClassification) -> AiResult<FakeClassificationResult> + Send + Sync;

enum Responses {
    Sequence(Mutex<VecDeque<FakeClassificationResult>>),
    Callback(Box<Resolver>),
}

/// 当前应用中的分类替身与分类请求记录。
pub struct FakeClassificationGateway {
    responses: Responses,
    prompts: Mutex<Vec<RecordedClassification>>,
    prevent_stray: AtomicBool,
}

impl FakeClassificationGateway {
    pub(crate) fn new(responses: impl IntoIterator<Item = FakeClassificationResult>) -> Self {
        Self::from_responses(Responses::Sequence(Mutex::new(
            responses.into_iter().collect(),
        )))
    }

    pub(crate) fn with(
        resolve: impl Fn(&RecordedClassification) -> AiResult<FakeClassificationResult>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        Self::from_responses(Responses::Callback(Box::new(resolve)))
    }

    fn from_responses(responses: Responses) -> Self {
        Self {
            responses,
            prompts: Mutex::default(),
            prevent_stray: AtomicBool::new(false),
        }
    }

    pub fn prevent_stray_classifications(&self, prevent: bool) -> &Self {
        self.prevent_stray.store(prevent, Ordering::Relaxed);
        self
    }

    pub fn recorded(&self) -> Vec<RecordedClassification> {
        self.prompts.lock().unwrap().clone()
    }

    #[track_caller]
    pub fn assert_classified(&self, predicate: impl Fn(&RecordedClassification) -> bool) {
        assert!(
            self.recorded().iter().any(predicate),
            "未发现匹配的分类请求"
        );
    }

    #[track_caller]
    pub fn assert_not_classified(&self, predicate: impl Fn(&RecordedClassification) -> bool) {
        assert!(
            !self.recorded().iter().any(predicate),
            "发现了不应发生的分类请求"
        );
    }

    #[track_caller]
    pub fn assert_nothing_classified(&self) {
        assert!(self.prompts.lock().unwrap().is_empty(), "发现了分类请求");
    }

    pub(super) fn respond(
        &self,
        prompt: RecordedClassification,
    ) -> AiResult<ClassificationResponse> {
        let response = match &self.responses {
            Responses::Sequence(responses) => responses.lock().unwrap().pop_front(),
            Responses::Callback(resolve) => Some(resolve(&prompt)?),
        };
        let response = match response {
            Some(FakeClassificationResult::Response(response)) => response,
            Some(FakeClassificationResult::Answers(answers)) => make_response(&prompt, answers),
            None if self.prevent_stray.load(Ordering::Relaxed) => {
                return Err(AiError::FakeExhausted);
            }
            None => make_response(&prompt, BTreeMap::new()),
        };
        self.prompts.lock().unwrap().push(prompt);
        Ok(response)
    }
}

fn make_response(
    prompt: &RecordedClassification,
    mut answers: BTreeMap<String, Answer>,
) -> ClassificationResponse {
    let answers = prompt
        .questions
        .iter()
        .map(|(key, question)| {
            let answer = answers
                .remove(key)
                .unwrap_or_else(|| default_answer(question));
            (key.clone(), answer)
        })
        .collect();
    ClassificationResponse {
        answers,
        model: prompt.model.clone().unwrap_or_default(),
        provider: prompt.provider.clone().unwrap_or_default(),
        usage: Default::default(),
    }
}

fn default_answer(question: &Question) -> Answer {
    match question {
        Question::Boolean(_) => Answer::Boolean(BooleanAnswer { probability: 0.5 }),
        Question::Choice(question) => {
            let options = question.options();
            let probability = 1.0 / options.len() as f64;
            Answer::Choice(ChoiceAnswer {
                choice: options[0].value.clone(),
                probabilities: options
                    .iter()
                    .map(|option| (option.value.clone(), probability))
                    .collect(),
                confidence: Some(probability),
            })
        }
        Question::Score(question) => {
            let levels = question.levels();
            let probability = 1.0 / levels.len() as f64;
            Answer::Score(ScoreAnswer {
                score: (levels.len() - 1) as f64 / 2.0,
                probabilities: (0..levels.len())
                    .map(|level| (level, probability))
                    .collect(),
                legend: levels.to_vec(),
                confidence: Some(probability),
            })
        }
    }
}
