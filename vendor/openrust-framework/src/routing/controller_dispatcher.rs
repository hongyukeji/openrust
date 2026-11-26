use std::{any::type_name, io};

use crate::{
    contracts::{container::Container as _, routing::Handler, support::Responsable},
    foundation::http::{FormRequest, ValidatedFormRequest},
    http::{HttpError, HttpFuture, Request, Response},
    support::facades::current_container,
};

pub(super) type Action<C, T = Response> =
    for<'a> fn(&'a C, &'a mut Request) -> HttpFuture<'a, Result<T, HttpError>>;

/// 可仅通过控制器类型注册的单动作控制器。
pub trait InvokableController {
    type Output: Responsable + 'static;

    fn invoke<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> HttpFuture<'a, Result<Self::Output, HttpError>>;
}

type Dispatch<C, T> =
    dyn for<'a> Fn(&'a C, &'a mut Request) -> HttpFuture<'a, Result<T, HttpError>> + Send + Sync;

use super::{HasMiddleware, MiddlewareEntry, controller_middleware::for_action};

/// Dispatches a typed controller action through the current application's container.
pub struct ControllerDispatcher<C, T = Response> {
    action: Box<Dispatch<C, T>>,
    form_request: bool,
    middleware: Vec<MiddlewareEntry>,
    action_name: Option<&'static str>,
}

impl<C: 'static, T: 'static> ControllerDispatcher<C, T> {
    pub fn new(action: Action<C, T>) -> Self {
        Self {
            action: Box::new(action),
            form_request: false,
            middleware: Vec::new(),
            action_name: None,
        }
    }

    /// 为指定动作加载控制器的静态中间件声明。
    pub fn with_middleware(mut self, action: &str) -> Self
    where
        C: HasMiddleware,
    {
        self.middleware = for_action(C::middleware(), action);
        self
    }
}

impl<C: InvokableController + 'static> ControllerDispatcher<C> {
    pub fn invokable() -> ControllerDispatcher<C, C::Output> {
        let mut dispatcher = ControllerDispatcher::new(C::invoke);
        dispatcher.action_name = Some(type_name::<C>());
        dispatcher
    }
}

pub(super) type FormAction<C, F, T = Response> = for<'a> fn(
    &'a C,
    &'a mut Request,
    ValidatedFormRequest<F>,
)
    -> HttpFuture<'a, Result<T, HttpError>>;

impl<C: Sync + 'static, T: Send + 'static> ControllerDispatcher<C, T> {
    /// 不注入可变请求的控制器动作自动借出当前请求，供门面与服务使用。
    pub fn without_request(
        action: for<'a> fn(&'a C) -> HttpFuture<'a, Result<T, HttpError>>,
    ) -> Self {
        Self {
            action: Box::new(move |controller, request| {
                Box::pin(async move {
                    request
                        .with_container(async { action(controller).await })
                        .await
                })
            }),
            form_request: false,
            middleware: Vec::new(),
            action_name: None,
        }
    }

    pub fn with_form_request<F: FormRequest + Default + 'static>(
        action: FormAction<C, F, T>,
    ) -> Self {
        Self {
            action: Box::new(move |controller, request| {
                Box::pin(async move {
                    let form = ValidatedFormRequest::resolve(request).await?;
                    crate::foundation::Precognition::abort(request)?;
                    action(controller, request, form).await
                })
            }),
            form_request: true,
            middleware: Vec::new(),
            action_name: None,
        }
    }
}

impl<C: Send + Sync + 'static, T: Responsable + 'static> Handler for ControllerDispatcher<C, T> {
    fn controller_action(&self) -> Option<&str> {
        self.action_name
    }

    fn controller_middleware(&self) -> &[MiddlewareEntry] {
        &self.middleware
    }

    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            let container = current_container().ok_or_else(|| HttpError::Internal {
                source: Box::new(io::Error::other(
                    "Controller dispatch requires an application scope.",
                )),
            })?;
            let controller = request.with_container(container.make_async::<C>()).await?;
            if !self.form_request {
                crate::foundation::Precognition::abort(request)?;
            }
            (self.action)(&controller, request)
                .await?
                .to_response(request)
                .await
        })
    }

    fn handle_precognitive<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        self.handle(request)
    }
}
