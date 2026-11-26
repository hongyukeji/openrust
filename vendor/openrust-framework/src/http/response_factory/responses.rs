use std::{error::Error, path::Path};

use axum::body::Bytes;
use futures_util::Stream;
use serde::Serialize;

use super::ResponseFactory;
use crate::{
    contracts::{
        container::{BindingResolutionError, Container as _},
        routing::UrlGenerator as _,
    },
    http::{
        HeaderMap, HttpError, RedirectResponse, Request, Response, ResponseOutput, StatusCode,
        StreamedEvent, StreamedJson, request_context,
    },
    routing::{RedirectOptions, Redirector, RouteParameter, UrlGenerator, UrlResult},
    support::facades::{Facade, Request as RequestFacade},
    view::ViewData,
};

impl ResponseFactory {
    /// 按返回值类型创建响应；状态和响应头在同一个响应对象上应用。
    pub fn make(
        &self,
        content: impl ResponseOutput,
        status: StatusCode,
        headers: HeaderMap,
    ) -> Result<Response, HttpError> {
        let mut response = content.into_response()?;
        response.parts_mut().status = status;
        Ok(response.with_headers(headers))
    }

    pub fn no_content(&self, status: StatusCode, headers: HeaderMap) -> Response {
        Response::empty(status).with_headers(headers)
    }

    pub fn json(
        &self,
        data: impl Serialize,
        status: StatusCode,
        headers: HeaderMap,
    ) -> Result<Response, HttpError> {
        Ok(Response::json(status, data)?.with_headers(headers))
    }

    pub fn jsonp(
        &self,
        callback: &str,
        data: impl Serialize,
        status: StatusCode,
        headers: HeaderMap,
    ) -> Result<Response, HttpError> {
        Ok(Response::jsonp(status, Some(callback), data)?.with_headers(headers))
    }

    pub fn markdown(
        &self,
        content: impl Into<String>,
        status: StatusCode,
        headers: HeaderMap,
    ) -> Response {
        Response::markdown(status, content).with_headers(headers)
    }

    /// 在当前请求上渲染；无请求上下文时返回错误，不另建请求或 Session。
    pub async fn view(
        &self,
        name: &str,
        data: ViewData,
        status: StatusCode,
        headers: HeaderMap,
    ) -> Result<Response, HttpError> {
        self.view_first(&[name], data, status, headers).await
    }

    /// 对应视图数组：选择第一个存在的视图。
    pub async fn view_first(
        &self,
        names: &[&str],
        data: ViewData,
        status: StatusCode,
        headers: HeaderMap,
    ) -> Result<Response, HttpError> {
        let view = self
            .container()?
            .make::<crate::view::Factory>()?
            .first(names, data)?;
        RequestFacade::get_facade_root()?
            .with_async(move |request| {
                Box::pin(async move { Response::view(status, view, headers, request).await })
            })
            .await
    }

    /// 显式请求借用入口，供已经持有可变请求的处理器使用。
    pub async fn view_for_request(
        &self,
        name: &str,
        data: ViewData,
        status: StatusCode,
        headers: HeaderMap,
        request: &mut Request,
    ) -> Result<Response, HttpError> {
        let view = self
            .container()?
            .make::<crate::view::Factory>()?
            .make(name.to_owned(), data)?;
        Response::view(status, view, headers, request).await
    }

    pub fn stream<T, E>(
        &self,
        source: impl Stream<Item = Result<T, E>> + Send + 'static,
        status: StatusCode,
        headers: HeaderMap,
    ) -> Response
    where
        T: Into<Bytes> + 'static,
        E: Into<Box<dyn Error + Send + Sync>> + 'static,
    {
        Response::stream(status, source).with_headers(headers)
    }

    pub fn stream_json(
        &self,
        data: StreamedJson,
        status: StatusCode,
        headers: HeaderMap,
    ) -> Response {
        Response::stream_json(status, data).with_headers(headers)
    }

    pub fn event_stream<E>(
        &self,
        source: impl Stream<Item = Result<StreamedEvent, E>> + Send + 'static,
        headers: HeaderMap,
    ) -> Response
    where
        E: Into<Box<dyn Error + Send + Sync>> + 'static,
    {
        Response::event_stream(source).with_headers(headers)
    }

    pub fn event_stream_with_end<E>(
        &self,
        source: impl Stream<Item = Result<StreamedEvent, E>> + Send + 'static,
        headers: HeaderMap,
        end: Option<StreamedEvent>,
    ) -> Response
    where
        E: Into<Box<dyn Error + Send + Sync>> + 'static,
    {
        Response::event_stream_with_end(source, end).with_headers(headers)
    }

    pub fn stream_download<T, E>(
        &self,
        source: impl Stream<Item = Result<T, E>> + Send + 'static,
        name: Option<&str>,
        headers: HeaderMap,
        disposition: &str,
    ) -> Result<Response, HttpError>
    where
        T: Into<Bytes> + 'static,
        E: Into<Box<dyn Error + Send + Sync>> + 'static,
    {
        Response::stream_download(source, name, headers, disposition)
    }

    pub async fn download(
        &self,
        path: impl AsRef<Path>,
        name: Option<&str>,
        headers: HeaderMap,
        disposition: &str,
    ) -> Result<Response, HttpError> {
        Response::download(path, name, headers, disposition).await
    }

    pub async fn file(
        &self,
        path: impl AsRef<Path>,
        headers: HeaderMap,
    ) -> Result<Response, HttpError> {
        Response::file(path, headers).await
    }

    fn container(
        &self,
    ) -> Result<std::sync::Arc<crate::container::Container>, BindingResolutionError> {
        let owner = self
            .owner
            .upgrade()
            .ok_or(BindingResolutionError::FacadeNotSet)?;
        if crate::support::facades::current_container()
            .is_some_and(|current| !std::sync::Arc::ptr_eq(&owner, &current))
        {
            return Err(BindingResolutionError::FacadeNotSet);
        }
        Ok(owner)
    }

    fn urls(&self) -> UrlResult<UrlGenerator> {
        let container = self
            .container()
            .map_err(|_| crate::routing::UrlError::Unavailable)?;
        let mut urls = container
            .make::<UrlGenerator>()
            .map_err(|_| crate::routing::UrlError::Unavailable)?
            .with_uri(request_context::with_url_uri())
            .with_base_url(request_context::base_url().unwrap_or_default());
        request_context::with_attributes(|attributes| urls.merge_defaults(attributes));
        Ok(urls)
    }

    fn redirector(&self) -> UrlResult<Redirector> {
        Ok(Redirector::new(self.urls()?))
    }

    pub fn redirect_to(
        &self,
        path: &str,
        options: RedirectOptions,
        secure: Option<bool>,
    ) -> UrlResult<RedirectResponse> {
        let target = self.urls()?.to(path, &[], secure)?;
        self.redirector()?.away(&target, options)
    }

    pub fn redirect_to_route(
        &self,
        route: &str,
        parameters: &[RouteParameter],
        options: RedirectOptions,
    ) -> UrlResult<RedirectResponse> {
        self.redirector()?.route(route, parameters, options)
    }

    pub fn redirect_to_action(
        &self,
        action: &str,
        parameters: &[RouteParameter],
        options: RedirectOptions,
    ) -> UrlResult<RedirectResponse> {
        self.redirector()?.action(action, parameters, options)
    }

    pub fn redirect_guest(
        &self,
        path: &str,
        options: RedirectOptions,
        secure: Option<bool>,
    ) -> Result<RedirectResponse, HttpError> {
        let target = self
            .urls()
            .map_err(url_error)?
            .to(path, &[], secure)
            .map_err(url_error)?;
        let redirector = self.redirector().map_err(url_error)?;
        RequestFacade::get_facade_root()?
            .with_mut(|request| redirector.guest(request, &target, options))?
            .map_err(url_error)
    }

    pub fn redirect_to_intended(
        &self,
        fallback: &str,
        options: RedirectOptions,
        secure: Option<bool>,
    ) -> Result<RedirectResponse, HttpError> {
        let mut urls = self.urls().map_err(url_error)?;
        if let Some(secure) = secure {
            urls.force_scheme(Some(if secure { "https" } else { "http" }));
        }
        let redirector = Redirector::new(urls);
        RequestFacade::get_facade_root()?
            .with_mut(|request| redirector.intended(request, fallback, options))?
            .map_err(url_error)
    }
}

fn url_error(source: crate::routing::UrlError) -> HttpError {
    HttpError::Internal {
        source: Box::new(source),
    }
}
