use std::{num::NonZeroUsize, sync::Arc, time::Duration};

use http::HeaderMap;

use serde::Serialize;

use crate::{
    container::ServiceKey,
    contracts::{
        container::{BindingResolutionError, Container as _},
        foundation::Application as _,
    },
    foundation::Application,
    http::HttpFuture,
    support::{ProviderResult, ServiceProvider, facades::Facade},
};

use super::{
    Error, Factory, PendingRequest, Pool, RecordedRequest, Response, ResponseSequence, Result,
};

/// Resolves the HTTP factory from the current application.
pub struct Http;

impl Facade for Http {
    type Service = Factory;

    fn getFacadeAccessor() -> ServiceKey {
        ServiceKey::typed::<Factory>()
    }
}

#[allow(non_snake_case, reason = "Matches Laravel's HTTP facade API")]
impl Http {
    pub fn pendingRequest() -> Result<PendingRequest> {
        Ok(Self::getFacadeRoot()
            .map_err(Error::Unavailable)?
            .pendingRequest())
    }

    pub async fn get(url: &str) -> Result<Response> {
        Self::pendingRequest()?.get(url).await
    }

    pub async fn head(url: &str) -> Result<Response> {
        Self::pendingRequest()?.head(url).await
    }

    pub async fn post(url: &str, data: &(impl Serialize + ?Sized)) -> Result<Response> {
        Self::pendingRequest()?.post(url, data).await
    }

    pub async fn put(url: &str, data: &(impl Serialize + ?Sized)) -> Result<Response> {
        Self::pendingRequest()?.put(url, data).await
    }

    pub async fn patch(url: &str, data: &(impl Serialize + ?Sized)) -> Result<Response> {
        Self::pendingRequest()?.patch(url, data).await
    }

    pub async fn delete(url: &str, data: &(impl Serialize + ?Sized)) -> Result<Response> {
        Self::pendingRequest()?.delete(url, data).await
    }

    pub fn acceptJson() -> Result<PendingRequest> {
        Ok(Self::pendingRequest()?.acceptJson())
    }

    pub fn withHeaders(headers: HeaderMap) -> Result<PendingRequest> {
        Ok(Self::pendingRequest()?.withHeaders(headers))
    }

    pub fn withToken(token: &str) -> Result<PendingRequest> {
        Ok(Self::pendingRequest()?.withToken(token))
    }

    pub fn baseUrl(url: impl Into<String>) -> Result<PendingRequest> {
        Ok(Self::pendingRequest()?.baseUrl(url))
    }

    pub fn asForm() -> Result<PendingRequest> {
        Ok(Self::pendingRequest()?.asForm())
    }

    pub fn timeout(duration: Duration) -> Result<PendingRequest> {
        Ok(Self::pendingRequest()?.timeout(duration))
    }

    pub fn retry(attempts: usize, delay: Duration) -> Result<PendingRequest> {
        Ok(Self::pendingRequest()?.retry(attempts, delay))
    }

    pub async fn pool<'a>(
        concurrency: NonZeroUsize,
        build: impl FnOnce(&mut Pool<'a>),
    ) -> Result<indexmap::IndexMap<String, Result<Response>>> {
        Ok(Self::getFacadeRoot()
            .map_err(Error::Unavailable)?
            .pool(concurrency, build)
            .await)
    }

    pub fn fake(
        callback: impl Fn(&reqwest::Request) -> Option<Result<Response>> + Send + Sync + 'static,
    ) -> Result<()> {
        Self::getFacadeRoot()
            .map_err(Error::Unavailable)?
            .fake(callback);
        Ok(())
    }

    pub fn fakeSequence(pattern: impl Into<String>) -> Result<ResponseSequence> {
        Ok(Self::getFacadeRoot()
            .map_err(Error::Unavailable)?
            .fakeSequence(pattern))
    }

    pub fn preventStrayRequests(prevent: bool) -> Result<()> {
        Self::getFacadeRoot()
            .map_err(Error::Unavailable)?
            .preventStrayRequests(prevent);
        Ok(())
    }

    #[track_caller]
    pub fn assertSent(
        predicate: impl Fn(&RecordedRequest, Option<&Response>) -> bool,
    ) -> Result<()> {
        Self::getFacadeRoot()
            .map_err(Error::Unavailable)?
            .assertSent(predicate);
        Ok(())
    }

    #[track_caller]
    pub fn assertNotSent(
        predicate: impl Fn(&RecordedRequest, Option<&Response>) -> bool,
    ) -> Result<()> {
        Self::getFacadeRoot()
            .map_err(Error::Unavailable)?
            .assertNotSent(predicate);
        Ok(())
    }

    #[track_caller]
    pub fn assertSentCount(expected: usize) -> Result<()> {
        Self::getFacadeRoot()
            .map_err(Error::Unavailable)?
            .assertSentCount(expected);
        Ok(())
    }

    #[track_caller]
    pub fn assertNothingSent() -> Result<()> {
        Self::assertSentCount(0)
    }

    pub fn recorded() -> Result<Vec<(RecordedRequest, Option<Response>)>> {
        Ok(Self::getFacadeRoot()
            .map_err(Error::Unavailable)?
            .recorded())
    }
}

pub struct HttpClientServiceProvider;

impl ServiceProvider for HttpClientServiceProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            app.container().singleton::<Factory, _>(|_| {
                Factory::new().map(Arc::new).map_err(|source| {
                    BindingResolutionError::FactoryFailed {
                        service: "http.client",
                        source: Box::new(source),
                    }
                })
            })?;
            Ok(())
        })
    }
}
