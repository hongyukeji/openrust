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
        let (keys, requests): (Vec<_>, Vec<_>) = self.requests.into_iter().unzip();
        let mut responses = stream::iter(requests.into_iter().enumerate())
            .map(|(index, request)| async move { (index, request.await) })
            .buffer_unordered(concurrency.get())
            .collect::<Vec<_>>()
            .await;
        responses.sort_unstable_by_key(|(index, _)| *index);
        keys.into_iter()
            .zip(responses.into_iter().map(|(_, response)| response))
            .collect()
    }
}
