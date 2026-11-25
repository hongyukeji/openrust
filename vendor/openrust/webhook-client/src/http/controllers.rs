use crate::{Error, Result, WebhookConfig, WebhookProcessor, WebhookRequest, WebhookRuntime};
use openrust::{
    contracts::routing::Handler,
    http::{HttpError, HttpFuture, Request, Response},
    support::facades::{App, Facade},
};
use std::sync::Arc;

/// 显式注册的接收控制器；与手动 Processor 共享流程，不自动绕过 CSRF。
pub struct WebhookController;
impl WebhookController {
    pub async fn invoke(
        &self,
        request: &mut Request,
        config: Arc<WebhookConfig>,
    ) -> Result<Response> {
        let app = App::get_facade_root().map_err(Error::binding)?;
        let bytes = request
            .body_bytes_with_limit(config.max_body_bytes())
            .await?;
        let snapshot = WebhookRequest::new(
            request.method().clone(),
            request.headers().clone(),
            bytes.to_vec(),
        )?
        .with_uri(request.uri().clone());
        // 工厂、处理流程共用同一次原生请求借用，允许合法的请求依赖在流程内使用。
        request
            .with_container(async {
                let runtime = WebhookRuntime::from_application(app, config.clone()).await?;
                WebhookProcessor::new(&snapshot, &config, &runtime)
                    .process()
                    .await
            })
            .await
    }
}
impl Handler for WebhookController {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> HttpFuture<'a, std::result::Result<Response, HttpError>> {
        Box::pin(async move {
            let app = App::get_facade_root()?;
            let config = request.with_container(app.make::<WebhookConfig>()).await?;
            match self.invoke(request, config).await {
                Err(Error::Http(error)) => Err(error),
                Err(source) => Err(HttpError::Internal {
                    source: Box::new(source),
                }),
                Ok(response) => Ok(response),
            }
        })
    }
}
