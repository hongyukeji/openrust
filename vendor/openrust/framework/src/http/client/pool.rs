use std::num::NonZeroUsize;

use futures_util::{StreamExt as _, stream};
use indexmap::IndexMap;

use crate::http::HttpFuture;

use super::{Factory, PendingRequest, Response, Result};

/// Builds named requests which run together with a bounded concurrency.
pub struct Pool<'a> {
    factory: Factory,
    requests: IndexMap<String, HttpFuture<'a, Result<Response>>>,
}

impl<'a> Pool<'a> {
    pub(super) fn new(factory: Factory) -> Self {
        Self {
            factory,
            requests: IndexMap::new(),
        }
    }

    pub fn r#as<F>(
        &mut self,
        key: impl Into<String>,
        build: impl FnOnce(PendingRequest) -> F,
    ) -> &mut Self
    where
        F: Future<Output = Result<Response>> + Send + 'a,
    {
        self.requests
            .insert(key.into(), Box::pin(build(self.factory.pendingRequest())));
        self
    }

    pub(super) async fn run(self, concurrency: NonZeroUsize) -> IndexMap<String, Result<Response>> {
        self.run_with(concurrency, |_, _| {}).await
    }

    pub(super) fn len(&self) -> usize {
        self.requests.len()
    }

    pub(super) async fn run_with(
        self,
        concurrency: NonZeroUsize,
        mut completed: impl FnMut(&str, &Result<Response>),
    ) -> IndexMap<String, Result<Response>> {
        let mut responses = stream::iter(self.requests.into_iter().enumerate())
            .map(|(index, (key, request))| async move { (index, key, request.await) })
            .buffer_unordered(concurrency.get());
        let mut ordered = Vec::new();
        while let Some((index, key, result)) = responses.next().await {
            completed(&key, &result);
            ordered.push((index, key, result));
        }
        ordered.sort_unstable_by_key(|(index, _, _)| *index);
        ordered
            .into_iter()
            .map(|(_, key, result)| (key, result))
            .collect()
    }
}
