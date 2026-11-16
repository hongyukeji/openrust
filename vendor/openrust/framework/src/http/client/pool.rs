use std::num::NonZeroUsize;

use futures_util::{StreamExt as _, stream::FuturesUnordered};
use indexmap::IndexMap;

use crate::http::HttpFuture;

use super::{Factory, PendingRequest, Response, Result};

/// Builds requests which run together with a bounded concurrency.
pub struct Pool<'a> {
    factory: Factory,
    next_index: u128,
    requests: IndexMap<String, HttpFuture<'a, Result<Response>>>,
}

#[allow(non_snake_case, reason = "Matches Laravel's HTTP pool API")]
impl<'a> Pool<'a> {
    pub(super) fn new(factory: Factory) -> Self {
        Self {
            factory,
            next_index: 0,
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
        let key = key.into();
        if let Ok(index) = key.parse::<i64>()
            && index >= 0
            && index.to_string() == key
        {
            self.next_index = self.next_index.max(index as u128 + 1);
        }
        self.requests
            .insert(key, Box::pin(build(self.factory.pendingRequest())));
        self
    }

    pub fn newRequest<F>(&mut self, build: impl FnOnce(PendingRequest) -> F) -> &mut Self
    where
        F: Future<Output = Result<Response>> + Send + 'a,
    {
        let key = self.next_index.to_string();
        self.next_index += 1;
        self.r#as(key, build)
    }

    pub(super) fn len(&self) -> usize {
        self.requests.len()
    }

    pub(super) async fn run_with(
        self,
        concurrency: NonZeroUsize,
        registration_order: bool,
        mut completed: impl FnMut(&str, &Result<Response>),
    ) -> IndexMap<String, Result<Response>> {
        let mut requests = self.requests.into_iter().enumerate();
        let mut active = FuturesUnordered::new();
        let mut responses = Vec::new();
        loop {
            while active.len() < concurrency.get() {
                let Some((index, (key, request))) = requests.next() else {
                    break;
                };
                active.push(async move { (index, key, request.await) });
            }
            let Some((index, key, result)) = active.next().await else {
                break;
            };
            completed(&key, &result);
            responses.push((index, key, result));
        }
        if registration_order {
            responses.sort_unstable_by_key(|(index, _, _)| *index);
        }
        responses
            .into_iter()
            .map(|(_, key, result)| (key, result))
            .collect()
    }
}
