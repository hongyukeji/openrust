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
    Batch, Error, Factory, PendingRequest, Pool, RecordedRequest, Response, ResponseSequence,
    Result,
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
    pub fn globalMiddleware(
        middleware: impl for<'a> Fn(super::Request, super::Next<'a>) -> HttpFuture<'a, Result<Response>>
        + Send
        + Sync
        + 'static,
    ) -> Result<()> {
        Self::getFacadeRoot()
            .map_err(Error::Unavailable)?
            .globalMiddleware(middleware);
        Ok(())
    }

    pub fn globalRequestMiddleware(
        middleware: impl Fn(&mut super::Request) -> Result<()> + Send + Sync + 'static,
    ) -> Result<()> {
        Self::getFacadeRoot()
            .map_err(Error::Unavailable)?
            .globalRequestMiddleware(middleware);
        Ok(())
    }

    pub fn globalResponseMiddleware(
        middleware: impl Fn(Response) -> Result<Response> + Send + Sync + 'static,
    ) -> Result<()> {
        Self::getFacadeRoot()
            .map_err(Error::Unavailable)?
            .globalResponseMiddleware(middleware);
        Ok(())
    }

    pub fn sink(sink: impl Into<super::Sink>) -> Result<PendingRequest> {
        Ok(Self::pendingRequest()?.sink(sink))
    }

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

    pub fn withUserAgent(user_agent: &str) -> Result<PendingRequest> {
        Ok(Self::pendingRequest()?.withUserAgent(user_agent))
    }

    pub fn withoutVerifying() -> Result<PendingRequest> {
        Ok(Self::pendingRequest()?.withoutVerifying())
    }

    pub fn withBasicAuth(username: &str, password: &str) -> Result<PendingRequest> {
        Ok(Self::pendingRequest()?.withBasicAuth(username, password))
    }

    pub fn withDigestAuth(username: &str, password: &str) -> Result<PendingRequest> {
        Ok(Self::pendingRequest()?.withDigestAuth(username, password))
    }

    pub fn baseUrl(url: impl Into<String>) -> Result<PendingRequest> {
        Ok(Self::pendingRequest()?.baseUrl(url))
    }

    pub fn withUrlParameters(
        parameters: serde_json::Map<String, serde_json::Value>,
    ) -> Result<PendingRequest> {
        Ok(Self::pendingRequest()?.withUrlParameters(parameters))
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

    pub async fn poolOrdered<'a>(
        build: impl FnOnce(&mut Pool<'a>),
    ) -> Result<indexmap::IndexMap<String, Result<Response>>> {
        Ok(Self::getFacadeRoot()
            .map_err(Error::Unavailable)?
            .poolOrdered(build)
            .await)
    }

    pub fn batch<'a>(build: impl FnOnce(&mut Batch<'a>)) -> Result<Batch<'a>> {
        Ok(Self::getFacadeRoot()
            .map_err(Error::Unavailable)?
            .batch(build))
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

    pub fn allowStrayRequests(patterns: impl IntoIterator<Item = impl Into<String>>) -> Result<()> {
        Self::getFacadeRoot()
            .map_err(Error::Unavailable)?
            .allowStrayRequests(patterns);
        Ok(())
    }

    pub fn preventingStrayRequests() -> Result<bool> {
        Ok(Self::getFacadeRoot()
            .map_err(Error::Unavailable)?
            .preventingStrayRequests())
    }

    pub fn isAllowedRequestUrl(url: &str) -> Result<bool> {
        Ok(Self::getFacadeRoot()
            .map_err(Error::Unavailable)?
            .isAllowedRequestUrl(url))
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
            app.container().singleton::<Factory, _>(|container| {
                let factory =
                    Factory::new().map_err(|source| BindingResolutionError::FactoryFailed {
                        service: "http.client",
                        source: Box::new(source),
                    })?;
                Ok(Arc::new(factory.withDispatcher(
                    container.make::<crate::events::Dispatcher>()?,
                )))
            })?;
            Ok(())
        })
    }
}
