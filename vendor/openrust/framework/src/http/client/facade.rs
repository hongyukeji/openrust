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

    fn get_facade_accessor() -> ServiceKey {
        ServiceKey::typed::<Factory>()
    }
}

impl Http {
    pub fn global_middleware(
        middleware: impl for<'a> Fn(super::Request, super::Next<'a>) -> HttpFuture<'a, Result<Response>>
        + Send
        + Sync
        + 'static,
    ) -> Result<()> {
        Self::get_facade_root()
            .map_err(Error::Unavailable)?
            .global_middleware(middleware);
        Ok(())
    }

    pub fn global_request_middleware(
        middleware: impl Fn(&mut super::Request) -> Result<()> + Send + Sync + 'static,
    ) -> Result<()> {
        Self::get_facade_root()
            .map_err(Error::Unavailable)?
            .global_request_middleware(middleware);
        Ok(())
    }

    pub fn global_response_middleware(
        middleware: impl Fn(Response) -> Result<Response> + Send + Sync + 'static,
    ) -> Result<()> {
        Self::get_facade_root()
            .map_err(Error::Unavailable)?
            .global_response_middleware(middleware);
        Ok(())
    }

    pub fn sink(sink: impl Into<super::Sink>) -> Result<PendingRequest> {
        Ok(Self::pending_request()?.sink(sink))
    }

    pub fn pending_request() -> Result<PendingRequest> {
        Ok(Self::get_facade_root()
            .map_err(Error::Unavailable)?
            .pending_request())
    }

    pub async fn get(url: &str) -> Result<Response> {
        Self::pending_request()?.get(url).await
    }

    pub async fn head(url: &str) -> Result<Response> {
        Self::pending_request()?.head(url).await
    }

    pub async fn post(url: &str, data: &(impl Serialize + ?Sized)) -> Result<Response> {
        Self::pending_request()?.post(url, data).await
    }

    pub async fn put(url: &str, data: &(impl Serialize + ?Sized)) -> Result<Response> {
        Self::pending_request()?.put(url, data).await
    }

    pub async fn patch(url: &str, data: &(impl Serialize + ?Sized)) -> Result<Response> {
        Self::pending_request()?.patch(url, data).await
    }

    pub async fn delete(url: &str, data: &(impl Serialize + ?Sized)) -> Result<Response> {
        Self::pending_request()?.delete(url, data).await
    }

    pub fn accept_json() -> Result<PendingRequest> {
        Ok(Self::pending_request()?.accept_json())
    }

    pub fn with_headers(headers: HeaderMap) -> Result<PendingRequest> {
        Ok(Self::pending_request()?.with_headers(headers))
    }

    pub fn with_token(token: &str) -> Result<PendingRequest> {
        Ok(Self::pending_request()?.with_token(token))
    }

    pub fn with_user_agent(user_agent: &str) -> Result<PendingRequest> {
        Ok(Self::pending_request()?.with_user_agent(user_agent))
    }

    pub fn without_verifying() -> Result<PendingRequest> {
        Ok(Self::pending_request()?.without_verifying())
    }

    pub fn with_basic_auth(username: &str, password: &str) -> Result<PendingRequest> {
        Ok(Self::pending_request()?.with_basic_auth(username, password))
    }

    pub fn with_digest_auth(username: &str, password: &str) -> Result<PendingRequest> {
        Ok(Self::pending_request()?.with_digest_auth(username, password))
    }

    pub fn base_url(url: impl Into<String>) -> Result<PendingRequest> {
        Ok(Self::pending_request()?.base_url(url))
    }

    pub fn with_url_parameters(
        parameters: serde_json::Map<String, serde_json::Value>,
    ) -> Result<PendingRequest> {
        Ok(Self::pending_request()?.with_url_parameters(parameters))
    }

    pub fn as_form() -> Result<PendingRequest> {
        Ok(Self::pending_request()?.as_form())
    }

    pub fn timeout(duration: Duration) -> Result<PendingRequest> {
        Ok(Self::pending_request()?.timeout(duration))
    }

    pub fn retry(attempts: usize, delay: Duration) -> Result<PendingRequest> {
        Ok(Self::pending_request()?.retry(attempts, delay))
    }

    pub async fn pool<'a>(
        concurrency: NonZeroUsize,
        build: impl FnOnce(&mut Pool<'a>),
    ) -> Result<indexmap::IndexMap<String, Result<Response>>> {
        Ok(Self::get_facade_root()
            .map_err(Error::Unavailable)?
            .pool(concurrency, build)
            .await)
    }

    pub async fn pool_ordered<'a>(
        build: impl FnOnce(&mut Pool<'a>),
    ) -> Result<indexmap::IndexMap<String, Result<Response>>> {
        Ok(Self::get_facade_root()
            .map_err(Error::Unavailable)?
            .pool_ordered(build)
            .await)
    }

    pub fn batch<'a>(build: impl FnOnce(&mut Batch<'a>)) -> Result<Batch<'a>> {
        Ok(Self::get_facade_root()
            .map_err(Error::Unavailable)?
            .batch(build))
    }

    pub fn fake(
        callback: impl Fn(&reqwest::Request) -> Option<Result<Response>> + Send + Sync + 'static,
    ) -> Result<()> {
        Self::get_facade_root()
            .map_err(Error::Unavailable)?
            .fake(callback);
        Ok(())
    }

    pub fn fake_sequence(pattern: impl Into<String>) -> Result<ResponseSequence> {
        Ok(Self::get_facade_root()
            .map_err(Error::Unavailable)?
            .fake_sequence(pattern))
    }

    pub fn prevent_stray_requests(prevent: bool) -> Result<()> {
        Self::get_facade_root()
            .map_err(Error::Unavailable)?
            .prevent_stray_requests(prevent);
        Ok(())
    }

    pub fn allow_stray_requests(
        patterns: impl IntoIterator<Item = impl Into<String>>,
    ) -> Result<()> {
        Self::get_facade_root()
            .map_err(Error::Unavailable)?
            .allow_stray_requests(patterns);
        Ok(())
    }

    pub fn preventing_stray_requests() -> Result<bool> {
        Ok(Self::get_facade_root()
            .map_err(Error::Unavailable)?
            .preventing_stray_requests())
    }

    pub fn is_allowed_request_url(url: &str) -> Result<bool> {
        Ok(Self::get_facade_root()
            .map_err(Error::Unavailable)?
            .is_allowed_request_url(url))
    }

    #[track_caller]
    pub fn assert_sent(
        predicate: impl Fn(&RecordedRequest, Option<&Response>) -> bool,
    ) -> Result<()> {
        Self::get_facade_root()
            .map_err(Error::Unavailable)?
            .assert_sent(predicate);
        Ok(())
    }

    #[track_caller]
    pub fn assert_not_sent(
        predicate: impl Fn(&RecordedRequest, Option<&Response>) -> bool,
    ) -> Result<()> {
        Self::get_facade_root()
            .map_err(Error::Unavailable)?
            .assert_not_sent(predicate);
        Ok(())
    }

    #[track_caller]
    pub fn assert_sent_count(expected: usize) -> Result<()> {
        Self::get_facade_root()
            .map_err(Error::Unavailable)?
            .assert_sent_count(expected);
        Ok(())
    }

    #[track_caller]
    pub fn assert_nothing_sent() -> Result<()> {
        Self::assert_sent_count(0)
    }

    pub fn recorded() -> Result<Vec<(RecordedRequest, Option<Response>)>> {
        Ok(Self::get_facade_root()
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
                Ok(Arc::new(factory.with_dispatcher(
                    container.make::<crate::events::Dispatcher>()?,
                )))
            })?;
            Ok(())
        })
    }
}
