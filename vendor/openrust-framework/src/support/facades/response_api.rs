use std::{error::Error, path::Path};

use axum::body::Bytes;
use futures_util::Stream;
use serde::Serialize;

use super::{Facade, Response};
use crate::{
    http::{
        HeaderMap, HttpError, RedirectResponse, Request, Response as HttpResponse, ResponseOutput,
        StatusCode, StreamedEvent, StreamedJson,
    },
    routing::{RedirectOptions, RouteParameter, UrlResult},
    view::ViewData,
};

impl Response {
    pub fn make(
        content: impl ResponseOutput,
        status: StatusCode,
        headers: HeaderMap,
    ) -> Result<HttpResponse, HttpError> {
        Self::get_facade_root()?.make(content, status, headers)
    }

    pub fn no_content(status: StatusCode, headers: HeaderMap) -> Result<HttpResponse, HttpError> {
        Ok(Self::get_facade_root()?.no_content(status, headers))
    }

    pub fn json(
        data: impl Serialize,
        status: StatusCode,
        headers: HeaderMap,
    ) -> Result<HttpResponse, HttpError> {
        Self::get_facade_root()?.json(data, status, headers)
    }

    pub fn jsonp(
        callback: &str,
        data: impl Serialize,
        status: StatusCode,
        headers: HeaderMap,
    ) -> Result<HttpResponse, HttpError> {
        Self::get_facade_root()?.jsonp(callback, data, status, headers)
    }

    pub fn markdown(
        content: impl Into<String>,
        status: StatusCode,
        headers: HeaderMap,
    ) -> Result<HttpResponse, HttpError> {
        Ok(Self::get_facade_root()?.markdown(content, status, headers))
    }

    pub async fn view(
        name: &str,
        data: ViewData,
        status: StatusCode,
        headers: HeaderMap,
    ) -> Result<HttpResponse, HttpError> {
        Self::get_facade_root()?
            .view(name, data, status, headers)
            .await
    }

    pub async fn view_first(
        names: &[&str],
        data: ViewData,
        status: StatusCode,
        headers: HeaderMap,
    ) -> Result<HttpResponse, HttpError> {
        Self::get_facade_root()?
            .view_first(names, data, status, headers)
            .await
    }

    pub async fn view_for_request(
        name: &str,
        data: ViewData,
        status: StatusCode,
        headers: HeaderMap,
        request: &mut Request,
    ) -> Result<HttpResponse, HttpError> {
        Self::get_facade_root()?
            .view_for_request(name, data, status, headers, request)
            .await
    }

    pub fn stream<T, E>(
        source: impl Stream<Item = Result<T, E>> + Send + 'static,
        status: StatusCode,
        headers: HeaderMap,
    ) -> Result<HttpResponse, HttpError>
    where
        T: Into<Bytes> + 'static,
        E: Into<Box<dyn Error + Send + Sync>> + 'static,
    {
        Ok(Self::get_facade_root()?.stream(source, status, headers))
    }

    pub fn stream_json(
        data: StreamedJson,
        status: StatusCode,
        headers: HeaderMap,
    ) -> Result<HttpResponse, HttpError> {
        Ok(Self::get_facade_root()?.stream_json(data, status, headers))
    }

    pub fn event_stream<E>(
        source: impl Stream<Item = Result<StreamedEvent, E>> + Send + 'static,
        headers: HeaderMap,
    ) -> Result<HttpResponse, HttpError>
    where
        E: Into<Box<dyn Error + Send + Sync>> + 'static,
    {
        Ok(Self::get_facade_root()?.event_stream(source, headers))
    }

    pub fn event_stream_with_end<E>(
        source: impl Stream<Item = Result<StreamedEvent, E>> + Send + 'static,
        headers: HeaderMap,
        end: Option<StreamedEvent>,
    ) -> Result<HttpResponse, HttpError>
    where
        E: Into<Box<dyn Error + Send + Sync>> + 'static,
    {
        Ok(Self::get_facade_root()?.event_stream_with_end(source, headers, end))
    }

    pub fn stream_download<T, E>(
        source: impl Stream<Item = Result<T, E>> + Send + 'static,
        name: Option<&str>,
        headers: HeaderMap,
        disposition: &str,
    ) -> Result<HttpResponse, HttpError>
    where
        T: Into<Bytes> + 'static,
        E: Into<Box<dyn Error + Send + Sync>> + 'static,
    {
        Self::get_facade_root()?.stream_download(source, name, headers, disposition)
    }

    pub async fn download(
        path: impl AsRef<Path>,
        name: Option<&str>,
        headers: HeaderMap,
        disposition: &str,
    ) -> Result<HttpResponse, HttpError> {
        Self::get_facade_root()?
            .download(path, name, headers, disposition)
            .await
    }

    pub async fn file(
        path: impl AsRef<Path>,
        headers: HeaderMap,
    ) -> Result<HttpResponse, HttpError> {
        Self::get_facade_root()?.file(path, headers).await
    }

    pub fn redirect_to(
        path: &str,
        options: RedirectOptions,
        secure: Option<bool>,
    ) -> UrlResult<RedirectResponse> {
        Self::get_facade_root()
            .map_err(|_| crate::routing::UrlError::Unavailable)?
            .redirect_to(path, options, secure)
    }

    pub fn redirect_to_route(
        route: &str,
        parameters: &[RouteParameter],
        options: RedirectOptions,
    ) -> UrlResult<RedirectResponse> {
        Self::get_facade_root()
            .map_err(|_| crate::routing::UrlError::Unavailable)?
            .redirect_to_route(route, parameters, options)
    }

    pub fn redirect_to_action(
        action: &str,
        parameters: &[RouteParameter],
        options: RedirectOptions,
    ) -> UrlResult<RedirectResponse> {
        Self::get_facade_root()
            .map_err(|_| crate::routing::UrlError::Unavailable)?
            .redirect_to_action(action, parameters, options)
    }

    pub fn redirect_guest(
        path: &str,
        options: RedirectOptions,
        secure: Option<bool>,
    ) -> Result<RedirectResponse, HttpError> {
        Self::get_facade_root()?.redirect_guest(path, options, secure)
    }

    pub fn redirect_to_intended(
        fallback: &str,
        options: RedirectOptions,
        secure: Option<bool>,
    ) -> Result<RedirectResponse, HttpError> {
        Self::get_facade_root()?.redirect_to_intended(fallback, options, secure)
    }
}
