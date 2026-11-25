use crate::{
    DefaultRespondsTo, DefaultSignatureValidator, Error, ProcessEverythingWebhookProfile,
    RespondsToWebhook, Result, SignatureValidator, WebhookConfig, WebhookProfile, WebhookRequest,
    WebhookStore, models::WebhookCall,
};
use openrust::{
    contracts::events::Dispatcher,
    events::EventContext,
    http::{HttpFuture, Response},
};
use std::sync::Arc;

/// 仅由 Processor 在验签及 Profile 通过后产生；不是可反序列化的输入 DTO。
pub struct VerifiedWebhook<'a> {
    request: &'a WebhookRequest,
    config: &'a WebhookConfig,
}
impl VerifiedWebhook<'_> {
    pub fn request(&self) -> &WebhookRequest {
        self.request
    }
    pub fn config(&self) -> &WebhookConfig {
        self.config
    }
}

/// 实现负责派发处理任务；成功表示派发成功，不代表业务已完成。
pub trait WebhookJobDispatcher: Send + Sync {
    /// 对照上游任务构造：保存后、清除异常前准备，失败也记入接收记录。
    fn prepare(&self, _: &WebhookCall) -> Result<()> {
        Ok(())
    }
    fn dispatch<'a>(&'a self, call: &'a WebhookCall) -> HttpFuture<'a, Result<()>>;
}

/// 每个端点显式选择的接收依赖，不使用全局工厂或共享账号回退。
pub struct WebhookRuntime {
    pub(crate) application: Option<Arc<openrust::foundation::Application>>,
    store: Arc<dyn WebhookStore>,
    job: Arc<dyn WebhookJobDispatcher>,
    validator: Arc<dyn SignatureValidator>,
    profile: Arc<dyn WebhookProfile>,
    response: Arc<dyn RespondsToWebhook>,
    events: Arc<dyn Dispatcher>,
}
impl WebhookRuntime {
    pub fn new(
        store: Arc<dyn WebhookStore>,
        job: Arc<dyn WebhookJobDispatcher>,
        events: Arc<dyn Dispatcher>,
    ) -> Self {
        Self {
            application: None,
            store,
            job,
            events,
            validator: Arc::new(DefaultSignatureValidator),
            profile: Arc::new(ProcessEverythingWebhookProfile),
            response: Arc::new(DefaultRespondsTo),
        }
    }
    pub fn with_signature_validator(mut self, validator: Arc<dyn SignatureValidator>) -> Self {
        self.validator = validator;
        self
    }
    pub fn with_webhook_profile(mut self, profile: Arc<dyn WebhookProfile>) -> Self {
        self.profile = profile;
        self
    }
    pub fn with_webhook_response(mut self, response: Arc<dyn RespondsToWebhook>) -> Self {
        self.response = response;
        self
    }
}

/// 保留上游验签、筛选、保存、派发、响应的顺序；可由自定义 HTTP 入口调用。
pub struct WebhookProcessor<'a> {
    request: &'a WebhookRequest,
    config: &'a WebhookConfig,
    runtime: &'a WebhookRuntime,
}
impl<'a> WebhookProcessor<'a> {
    pub fn new(
        request: &'a WebhookRequest,
        config: &'a WebhookConfig,
        runtime: &'a WebhookRuntime,
    ) -> Self {
        Self {
            request,
            config,
            runtime,
        }
    }
    pub async fn process(&self) -> Result<Response> {
        if let Some(app) = &self.runtime.application {
            app.call(
                openrust::container::Callback::new(&[], |_| async {
                    Ok(self.process_inner().await)
                }),
                Default::default(),
            )
            .await
            .map_err(Error::binding)?
        } else {
            self.process_inner().await
        }
    }
    async fn process_inner(&self) -> Result<Response> {
        // 资源预算由流程入口再次检查，自定义验证器不能绕过正文上限。
        if self.request.body().len() > self.config.max_body_bytes() {
            return Err(Error::PayloadTooLarge);
        }
        if !self.runtime.validator.is_valid(self.request, self.config)? {
            self.runtime
                .events
                .dispatch(
                    Arc::new(crate::events::InvalidWebhookSignatureEvent {
                        request: Arc::new(self.request.clone()),
                        config: Arc::new(self.config.clone()),
                    }),
                    &mut EventContext::new(),
                )
                .await
                .map_err(|source| Error::Runtime {
                    source: Box::new(source),
                })?;
            return Err(Error::InvalidSignature);
        }
        if !self
            .runtime
            .profile
            .should_process(self.request, self.config)?
        {
            return self.create_response();
        }
        let verified = VerifiedWebhook {
            request: self.request,
            config: self.config,
        };
        let mut call = WebhookCall::store_webhook(self.runtime.store.as_ref(), &verified).await?;
        let dispatched = async {
            self.runtime.job.prepare(&call)?;
            call.clear_exception(self.runtime.store.as_ref()).await?;
            self.runtime.job.dispatch(&call).await
        }
        .await;
        if let Err(error) = dispatched {
            // 不持久化任意错误文本／堆栈，避免第三方错误把密钥或正文写入诊断。
            call.save_exception(
                self.runtime.store.as_ref(),
                serde_json::json!({"code":0,"message":"Webhook 处理任务派发失败"}),
            )
            .await?;
            return Err(error);
        }
        self.create_response()
    }
    fn create_response(&self) -> Result<Response> {
        self.runtime
            .response
            .respond_to_valid_webhook(self.request, self.config)
    }
}
