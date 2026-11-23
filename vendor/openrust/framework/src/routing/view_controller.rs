use crate::{
    contracts::routing::Handler,
    http::{HeaderMap, HttpError, HttpFuture, Request, Response, StatusCode},
    support::facades::View,
    view::ViewData,
};

/// 视图路由的可选参数；Rust 不支持省略位置参数。
#[derive(Clone)]
pub struct ViewRouteOptions {
    pub data: ViewData,
    pub status: StatusCode,
    pub headers: HeaderMap,
}

impl Default for ViewRouteOptions {
    fn default() -> Self {
        Self {
            data: ViewData::new(),
            status: StatusCode::OK,
            headers: HeaderMap::new(),
        }
    }
}

/// 从当前应用的视图工厂渲染视图，路径参数覆盖同名初始数据。
pub struct ViewController {
    view: String,
    options: ViewRouteOptions,
}

impl ViewController {
    pub fn new(view: &str, options: ViewRouteOptions) -> Self {
        Self {
            view: view.to_owned(),
            options,
        }
    }
}

impl Handler for ViewController {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            let mut data = self.options.data.clone();
            for (name, value) in request.route_parameters() {
                if !matches!(name.as_str(), "view" | "data" | "status" | "headers") {
                    data.insert(name.clone(), value.clone().into());
                }
            }
            let view =
                View::make(self.view.clone(), data).map_err(|source| HttpError::Internal {
                    source: Box::new(source),
                })?;
            Response::view(
                self.options.status,
                view,
                self.options.headers.clone(),
                request,
            )
            .await
        })
    }
}
