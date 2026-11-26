use std::{
    collections::VecDeque,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use openrust::support::Str;

use crate::{AiError, AiResult, EmbeddingsResponse, Usage, embeddings::valid_vectors};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordedEmbeddings {
    pub inputs: Vec<String>,
    pub dimensions: Option<usize>,
    pub timeout: Duration,
    pub provider: Option<String>,
    pub model: Option<String>,
}

type VectorBatch = Vec<Vec<f32>>;
type Resolver = dyn Fn(&RecordedEmbeddings) -> AiResult<VectorBatch> + Send + Sync;

enum Responses {
    Sequence(Mutex<VecDeque<VectorBatch>>),
    Callback(Box<Resolver>),
}

/// 应用内的向量生成替身与调用记录。
pub struct FakeEmbeddingGateway {
    responses: Responses,
    prompts: Mutex<Vec<RecordedEmbeddings>>,
    prevent_stray: AtomicBool,
}

impl FakeEmbeddingGateway {
    pub(crate) fn new(responses: impl IntoIterator<Item = VectorBatch>) -> Self {
        Self::from_responses(Responses::Sequence(Mutex::new(
            responses.into_iter().collect(),
        )))
    }

    pub(crate) fn with(
        resolve: impl Fn(&RecordedEmbeddings) -> AiResult<VectorBatch> + Send + Sync + 'static,
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

    pub fn prevent_stray_embeddings(&self, prevent: bool) -> &Self {
        self.prevent_stray.store(prevent, Ordering::Relaxed);
        self
    }

    pub fn recorded(&self) -> Vec<RecordedEmbeddings> {
        self.prompts.lock().unwrap().clone()
    }

    #[track_caller]
    pub fn assert_generated(&self, predicate: impl Fn(&RecordedEmbeddings) -> bool) {
        assert!(
            self.recorded().iter().any(predicate),
            "未发现匹配的向量生成请求"
        );
    }

    #[track_caller]
    pub fn assert_not_generated(&self, predicate: impl Fn(&RecordedEmbeddings) -> bool) {
        assert!(
            !self.recorded().iter().any(predicate),
            "发现了不应发生的向量生成请求"
        );
    }

    #[track_caller]
    pub fn assert_nothing_generated(&self) {
        assert!(
            self.prompts.lock().unwrap().is_empty(),
            "发现了向量生成请求"
        );
    }

    pub(crate) fn respond(&self, request: RecordedEmbeddings) -> AiResult<EmbeddingsResponse> {
        let embeddings = match &self.responses {
            Responses::Sequence(responses) => responses.lock().unwrap().pop_front(),
            Responses::Callback(resolve) => Some(resolve(&request)?),
        };
        let embeddings = match embeddings {
            Some(embeddings) => embeddings,
            None if self.prevent_stray.load(Ordering::Relaxed) => {
                return Err(AiError::FakeExhausted);
            }
            None => {
                let dimensions = request
                    .dimensions
                    .ok_or(AiError::InvalidConfiguration("Embeddings Fake 需要维度"))?;
                (0..request.inputs.len())
                    .map(|_| fake_embedding(dimensions))
                    .collect::<AiResult<_>>()?
            }
        };
        if !valid_vectors(&embeddings, request.inputs.len(), request.dimensions) {
            return Err(AiError::InvalidResponse);
        }
        let provider = request.provider.clone().unwrap_or_default();
        let model = request.model.clone().unwrap_or_default();
        self.prompts.lock().unwrap().push(request);
        Ok(EmbeddingsResponse {
            embeddings,
            usage: Usage::default(),
            provider,
            model,
        })
    }
}

pub(crate) fn fake_embedding(dimensions: usize) -> AiResult<Vec<f32>> {
    if dimensions == 0 {
        return Err(AiError::InvalidConfiguration("embeddings.dimensions"));
    }
    let mut values: Vec<_> = Str::random(dimensions)
        .bytes()
        .map(|byte| f32::from(byte) - 77.5)
        .collect();
    let length = values.iter().map(|value| value * value).sum::<f32>().sqrt();
    for value in &mut values {
        *value /= length;
    }
    Ok(values)
}
