use std::{
    collections::VecDeque,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use openrust::support::Collection;

use crate::{
    AiError, AiResult, RankedDocument, RerankingResponse, Usage, reranking::validate_results,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordedReranking {
    pub documents: Vec<String>,
    pub query: String,
    pub limit: Option<usize>,
    pub timeout: Duration,
    pub provider: Option<String>,
    pub model: Option<String>,
}

pub enum FakeRerankingResult {
    Rankings(Vec<RankedDocument>),
    Response(RerankingResponse),
}

impl From<Vec<RankedDocument>> for FakeRerankingResult {
    fn from(value: Vec<RankedDocument>) -> Self {
        Self::Rankings(value)
    }
}

impl From<RerankingResponse> for FakeRerankingResult {
    fn from(value: RerankingResponse) -> Self {
        Self::Response(value)
    }
}

type Resolver = dyn Fn(&RecordedReranking) -> AiResult<FakeRerankingResult> + Send + Sync;

enum Responses {
    Sequence(Mutex<VecDeque<FakeRerankingResult>>),
    Callback(Box<Resolver>),
}

/// 应用内的文本重排序替身与调用记录。
pub struct FakeRerankingGateway {
    responses: Responses,
    prompts: Mutex<Vec<RecordedReranking>>,
    prevent_stray: AtomicBool,
}

impl FakeRerankingGateway {
    pub(crate) fn new(responses: impl IntoIterator<Item = FakeRerankingResult>) -> Self {
        Self::from_responses(Responses::Sequence(Mutex::new(
            responses.into_iter().collect(),
        )))
    }

    pub(crate) fn with(
        resolve: impl Fn(&RecordedReranking) -> AiResult<FakeRerankingResult> + Send + Sync + 'static,
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

    pub fn prevent_stray_rerankings(&self, prevent: bool) -> &Self {
        self.prevent_stray.store(prevent, Ordering::Relaxed);
        self
    }

    pub fn recorded(&self) -> Vec<RecordedReranking> {
        self.prompts.lock().unwrap().clone()
    }

    #[track_caller]
    pub fn assert_reranked(&self, predicate: impl Fn(&RecordedReranking) -> bool) {
        assert!(
            self.recorded().iter().any(predicate),
            "未发现匹配的重排序请求"
        );
    }

    #[track_caller]
    pub fn assert_not_reranked(&self, predicate: impl Fn(&RecordedReranking) -> bool) {
        assert!(
            !self.recorded().iter().any(predicate),
            "发现了不应发生的重排序请求"
        );
    }

    #[track_caller]
    pub fn assert_nothing_reranked(&self) {
        assert!(self.prompts.lock().unwrap().is_empty(), "发现了重排序请求");
    }

    pub(crate) fn respond(&self, request: RecordedReranking) -> AiResult<RerankingResponse> {
        let response = match &self.responses {
            Responses::Sequence(responses) => responses.lock().unwrap().pop_front(),
            Responses::Callback(resolve) => Some(resolve(&request)?),
        };
        let response = match response {
            Some(FakeRerankingResult::Response(response)) => response,
            Some(FakeRerankingResult::Rankings(results)) => make_response(&request, results),
            None if self.prevent_stray.load(Ordering::Relaxed) => {
                return Err(AiError::FakeExhausted);
            }
            None => make_response(
                &request,
                fake_results(request.documents.len(), request.limit)?,
            ),
        };
        validate_results(&response.results, request.documents.len(), request.limit)?;
        self.prompts.lock().unwrap().push(request);
        Ok(response)
    }
}

fn make_response(request: &RecordedReranking, results: Vec<RankedDocument>) -> RerankingResponse {
    RerankingResponse {
        results,
        usage: Usage::default(),
        provider: request.provider.clone().unwrap_or_default(),
        model: request.model.clone().unwrap_or_default(),
    }
}

fn fake_results(count: usize, limit: Option<usize>) -> AiResult<Vec<RankedDocument>> {
    let maximum = limit.unwrap_or(count).min(count);
    let indices = Collection::make(0..count)
        .shuffle()
        .map_err(|_| AiError::Unavailable)?;
    Ok(indices
        .all()
        .values()
        .take(maximum)
        .enumerate()
        .map(|(position, &index)| RankedDocument {
            index,
            relevance_score: 1.0 - position as f64 / maximum as f64,
        })
        .collect())
}
