use crate::{
    DefaultRespondsTo, DefaultSignatureValidator, Error, ProcessEverythingWebhookProfile,
    RespondsToWebhook, Result, SignatureValidator, WebhookConfig, WebhookProfile, WebhookRequest,
    WebhookStore,
    models::WebhookCall,
    verification::{DatabaseVerificationAudit, VerificationDecision},
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
    event_id: Option<&'a str>,
    body_deduplication_window: Option<u32>,
}
impl VerifiedWebhook<'_> {
    pub fn request(&self) -> &WebhookRequest {
        self.request
    }
    pub fn config(&self) -> &WebhookConfig {
        self.config
    }
    pub fn event_id(&self) -> Option<&str> {
        self.event_id
    }
    pub fn body_deduplication_window(&self) -> Option<u32> {
        self.body_deduplication_window
    }
}

/// 实现负责派发处理任务；成功表示派发成功，不代表业务已完成。
pub trait WebhookJobDispatcher: Send + Sync {
    /// 对照上游任务构造：保存后、清除异常前准备，失败也记入接收记录。
    fn prepare(&self, _: &WebhookCall) -> Result<()> {
        Ok(())
    }
    fn dispatch_in_transaction<'a>(
        &'a self,
        _call: &'a WebhookCall,
        _transaction: &'a mut openrust::database::Transaction,
    ) -> HttpFuture<'a, Result<()>> {
        Box::pin(async { Err(Error::InvalidConfig("atomic_database_queue")) })
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
    hint: Option<Arc<dyn crate::hints::ReceivesUnverifiedHint>>,
    verification_audit: Option<(DatabaseVerificationAudit, String)>,
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
            hint: None,
            verification_audit: None,
            validator: Arc::new(DefaultSignatureValidator),
            profile: Arc::new(ProcessEverythingWebhookProfile),
            response: Arc::new(DefaultRespondsTo),
        }
    }
    pub fn with_unverified_hint_receiver(
        mut self,
        receiver: Arc<dyn crate::hints::ReceivesUnverifiedHint>,
    ) -> Self {
        self.hint = Some(receiver);
        self
    }

    pub fn with_verification_audit(
        mut self,
        audit: DatabaseVerificationAudit,
        validator: &str,
    ) -> Result<Self> {
        crate::factories::validate_reference("signature_validator", validator)?;
        self.verification_audit = Some((audit, validator.to_owned()));
        Ok(self)
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
        if let Some(receiver) = &self.runtime.hint
            && !self
                .runtime
                .validator
                .has_signature(self.request, self.config)
        {
            self.record_verification(VerificationDecision::UnverifiedHint)
                .await?;
            receiver
                .receive(crate::hints::UnverifiedHint::new(self.request, self.config))
                .await?;
            return Ok(Response::json(
                openrust::http::StatusCode::ACCEPTED,
                serde_json::json!({"message":"accepted"}),
            )?);
        }
        let valid = match self.runtime.validator.is_valid(self.request, self.config) {
            Ok(valid) => valid,
            Err(error) => {
                self.record_verification(VerificationDecision::ValidatorError)
                    .await?;
                return Err(error);
            }
        };
        if !valid {
            let audit = if self.runtime.verification_audit.is_some() {
                let decision = if self
                    .runtime
                    .validator
                    .has_signature(self.request, self.config)
                {
                    VerificationDecision::InvalidSignature
                } else {
                    VerificationDecision::MissingSignature
                };
                self.record_verification(decision).await
            } else {
                Ok(())
            };

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
            audit?;
            return Err(Error::InvalidSignature);
        }
        self.record_verification(VerificationDecision::Verified)
            .await?;
        if !self
            .runtime
            .profile
            .should_process(self.request, self.config)?
        {
            return self.create_response();
        }
        let event_id = if self.config.deduplicate_verified_events()? {
            let id = self
                .runtime
                .validator
                .verified_event_id(self.request, self.config)?
                .filter(|id| {
                    !id.is_empty()
                        && id.len() <= 256
                        && id.trim() == id.as_ref()
                        && !id.chars().any(char::is_control)
                })
                .ok_or(Error::MissingVerifiedEventId)?;
            Some(id)
        } else {
            None
        };
        let verified = VerifiedWebhook {
            request: self.request,
            config: self.config,
            event_id: event_id.as_deref(),
            body_deduplication_window: self.config.body_deduplication_window()?,
        };
        if self.config.atomic_database_queue()? {
            self.runtime
                .store
                .store_and_dispatch_atomic(&verified, self.runtime.job.as_ref())
                .await?;
            return self.create_response();
        }
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
    async fn record_verification(&self, decision: VerificationDecision) -> Result<()> {
        let Some((audit, validator)) = &self.runtime.verification_audit else {
            return Ok(());
        };
        let policy = match decision {
            VerificationDecision::UnverifiedHint => {
                self.config.options()["unverified_hint_receiver"]
                    .as_str()
                    .unwrap_or("unverified_hint")
            }
            _ => "signature_required",
        };
        audit.record(self.config, validator, policy, decision).await
    }

    fn create_response(&self) -> Result<Response> {
        self.runtime
            .response
            .respond_to_valid_webhook(self.request, self.config)
    }
}
