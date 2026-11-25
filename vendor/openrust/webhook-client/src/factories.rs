//! 具名实现复用应用原生容器；没有进程全局注册表或跨端点账号回退。
use crate::{
    DatabaseWebhookStore, DefaultRespondsTo, DefaultSignatureValidator, Error,
    ProcessEverythingWebhookProfile, RespondsToWebhook, Result, SignatureValidator,
    StandardSignatureValidator, WebhookConfig, WebhookJobDispatcher, WebhookProfile,
    WebhookRuntime, WebhookStore, models::WebhookCall,
};
use openrust::{
    container::Container,
    contracts::{container::Container as _, foundation::Application as _},
    foundation::Application,
    http::HttpFuture,
};
use std::sync::{Arc, Mutex};
type Creator<T> = dyn Fn(&WebhookConfig, &Container) -> Result<Arc<T>> + Send + Sync;
struct Factory<T: ?Sized> {
    create: Arc<Creator<T>>,
}
pub(crate) fn validate_reference(role: &'static str, name: &str) -> Result<()> {
    if name.is_empty()
        || name.len() > 128
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.:".contains(&b))
    {
        return Err(Error::InvalidConfig(role));
    }
    Ok(())
}
fn key(role: &'static str, name: &str) -> Result<String> {
    validate_reference(role, name)?;
    Ok(format!("webhook-client.{role}.{name}"))
}

fn register<T: ?Sized + Send + Sync + 'static>(
    container: &Container,
    role: &'static str,
    name: &str,
    create: impl Fn(&WebhookConfig, &Container) -> Result<Arc<T>> + Send + Sync + 'static,
) -> Result<()> {
    container
        .instance_named(
            &key(role, name)?,
            Arc::new(Factory {
                create: Arc::new(create),
            }),
        )
        .map_err(Error::binding)?;
    Ok(())
}
fn get<T: ?Sized + Send + Sync + 'static>(
    container: &Container,
    role: &'static str,
    name: &str,
) -> Result<Arc<Factory<T>>> {
    container
        .get::<Factory<T>>(key(role, name)?.as_str())
        .map_err(|source| Error::InvalidImplementation {
            field: role,
            source,
        })
}
fn option<'a>(
    config: &'a WebhookConfig,
    field: &'static str,
    default: Option<&'a str>,
) -> Result<&'a str> {
    match config.options().get(field) {
        Some(serde_json::Value::String(value)) => Ok(value),
        None | Some(serde_json::Value::Null) => default.ok_or(Error::InvalidConfig(field)),
        _ => Err(Error::InvalidConfig(field)),
    }
}
macro_rules! registration {
    ($method:ident,$role:literal,$contract:ty) => {
        pub fn $method(
            container: &Container,
            name: &str,
            create: impl Fn(&WebhookConfig, &Container) -> Result<Arc<$contract>>
            + Send
            + Sync
            + 'static,
        ) -> Result<()> {
            register::<$contract>(container, $role, name, create)
        }
    };
}
impl WebhookRuntime {
    registration!(
        register_signature_validator,
        "signature_validator",
        dyn SignatureValidator
    );
    registration!(
        register_webhook_profile,
        "webhook_profile",
        dyn WebhookProfile
    );
    registration!(
        register_webhook_response,
        "webhook_response",
        dyn RespondsToWebhook
    );
    registration!(register_webhook_store, "webhook_model", dyn WebhookStore);
    registration!(
        register_job_dispatcher,
        "process_webhook_job",
        dyn WebhookJobDispatcher
    );

    registration!(
        register_unverified_hint_receiver,
        "unverified_hint_receiver",
        dyn crate::hints::ReceivesUnverifiedHint
    );

    /// 每次按当前应用和不可变配置快照解析，工厂代码只来自应用的 Rust 注册。
    pub async fn from_application(
        app: Arc<Application>,
        config: Arc<WebhookConfig>,
    ) -> Result<Self> {
        let owner = app.clone();
        app.call(
            openrust::container::Callback::new(&[], move |_| async move {
                Ok(Self::from_application_scoped(owner, config))
            }),
            Default::default(),
        )
        .await
        .map_err(Error::binding)?
    }

    fn from_application_scoped(app: Arc<Application>, config: Arc<WebhookConfig>) -> Result<Self> {
        let container = app.container();
        let validator_name = option(&config, "signature_validator", None)?;
        let validator =
            get::<dyn SignatureValidator>(container, "signature_validator", validator_name)?;
        let validator = (validator.create)(&config, container)?;
        let profile = get::<dyn WebhookProfile>(
            container,
            "webhook_profile",
            option(&config, "webhook_profile", None)?,
        )?;
        let profile = (profile.create)(&config, container)?;
        let response = get::<dyn RespondsToWebhook>(
            container,
            "webhook_response",
            option(&config, "webhook_response", Some("default"))?,
        )?;
        let response = (response.create)(&config, container)?;
        let store_name = option(&config, "webhook_model", None)?.to_owned();
        let job = get::<dyn WebhookJobDispatcher>(
            container,
            "process_webhook_job",
            option(&config, "process_webhook_job", None)?,
        )?;
        let events = container
            .get::<openrust::events::Dispatcher>("events")
            .map_err(Error::binding)?;
        let hint = match config.options().get("unverified_hint_receiver") {
            None | Some(serde_json::Value::Null) => None,
            Some(serde_json::Value::String(name)) => {
                let factory = get::<dyn crate::hints::ReceivesUnverifiedHint>(
                    container,
                    "unverified_hint_receiver",
                    name,
                )?;
                Some((factory.create)(&config, container)?)
            }
            _ => return Err(Error::InvalidConfig("unverified_hint_receiver")),
        };
        let audit = match config.options().get("verification_audit") {
            None | Some(serde_json::Value::Null | serde_json::Value::Bool(false)) => None,
            Some(serde_json::Value::Bool(true)) => {
                let scope = option(&config, "scope", None)?;
                let connection = match config.options().get("connection") {
                    None | Some(serde_json::Value::Null) => None,
                    Some(serde_json::Value::String(connection)) => Some(connection.as_str()),
                    _ => return Err(Error::InvalidConfig("connection")),
                };
                let manager = container.make::<openrust::database::DatabaseManager>()?;
                Some((
                    crate::verification::DatabaseVerificationAudit::new(
                        manager.connection(connection)?,
                        scope,
                    )?,
                    validator_name.to_owned(),
                ))
            }
            _ => return Err(Error::InvalidConfig("verification_audit")),
        };
        let owner = app.clone();
        let mut runtime = Self::new(
            Arc::new(DeferredStore {
                app: app.clone(),
                config: config.clone(),
                name: store_name,
                resolved: Mutex::new(None),
            }),
            Arc::new(DeferredJob {
                app,
                config,
                factory: job,
                resolved: Mutex::new(None),
            }),
            events,
        )
        .with_signature_validator(validator)
        .with_webhook_profile(profile)
        .with_webhook_response(response);
        if let Some(hint) = hint {
            runtime = runtime.with_unverified_hint_receiver(hint);
        }
        if let Some((audit, validator)) = audit {
            runtime = runtime.with_verification_audit(audit, &validator)?;
        }
        runtime.application = Some(owner);
        Ok(runtime)
    }
}
pub(crate) fn defaults(container: &Container) -> Result<()> {
    macro_rules! default {
        ($method:ident,$role:literal,$name:literal,$factory:expr) => {
            if !container.has(key($role, $name)?.as_str()) {
                WebhookRuntime::$method(container, $name, $factory)?;
            }
        };
    }
    default!(
        register_signature_validator,
        "signature_validator",
        "default",
        |_, _| Ok(Arc::new(DefaultSignatureValidator))
    );
    default!(
        register_signature_validator,
        "signature_validator",
        "standard",
        |_, _| Ok(Arc::new(StandardSignatureValidator))
    );
    default!(
        register_webhook_profile,
        "webhook_profile",
        "process-everything",
        |_, _| Ok(Arc::new(ProcessEverythingWebhookProfile))
    );
    default!(
        register_webhook_response,
        "webhook_response",
        "default",
        |_, _| Ok(Arc::new(DefaultRespondsTo))
    );
    default!(
        register_webhook_store,
        "webhook_model",
        "database",
        |config, container| {
            let scope = config.options()["scope"]
                .as_str()
                .ok_or(Error::InvalidConfig("scope"))?;
            let connection = match &config.options()["connection"] {
                serde_json::Value::Null => None,
                serde_json::Value::String(name) if !name.is_empty() && name.len() <= 128 => {
                    Some(name.as_str())
                }
                _ => return Err(Error::InvalidConfig("connection")),
            };
            let manager = container
                .make::<openrust::database::DatabaseManager>()
                .map_err(Error::binding)?;
            Ok(Arc::new(DatabaseWebhookStore::new(
                manager.connection(connection)?,
                scope,
            )?))
        }
    );
    Ok(())
}
struct DeferredStore {
    app: Arc<Application>,
    config: Arc<WebhookConfig>,
    name: String,
    resolved: Mutex<Option<Arc<dyn WebhookStore>>>,
}
impl DeferredStore {
    fn resolve(&self) -> Result<Arc<dyn WebhookStore>> {
        let mut resolved = self
            .resolved
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(value) = resolved.as_ref() {
            return Ok(value.clone());
        }
        let factory = get::<dyn WebhookStore>(self.app.container(), "webhook_model", &self.name)?;
        let store = (factory.create)(&self.config, self.app.container())?;
        *resolved = Some(store.clone());
        Ok(store)
    }
}
impl WebhookStore for DeferredStore {
    fn store_and_dispatch_atomic<'a>(
        &'a self,
        webhook: &'a crate::VerifiedWebhook<'_>,
        dispatcher: &'a dyn WebhookJobDispatcher,
    ) -> HttpFuture<'a, Result<()>> {
        Box::pin(async move {
            self.resolve()?
                .store_and_dispatch_atomic(webhook, dispatcher)
                .await
        })
    }
    fn store_webhook<'a>(
        &'a self,
        webhook: &'a crate::VerifiedWebhook<'_>,
    ) -> HttpFuture<'a, Result<WebhookCall>> {
        Box::pin(async move { self.resolve()?.store_webhook(webhook).await })
    }
    fn find<'a>(&'a self, id: &'a str) -> HttpFuture<'a, Result<Option<WebhookCall>>> {
        Box::pin(async move { self.resolve()?.find(id).await })
    }
    fn payload<'a>(&'a self, id: &'a str) -> HttpFuture<'a, Result<Option<serde_json::Value>>> {
        Box::pin(async move { self.resolve()?.payload(id).await })
    }
    fn get_attachments<'a>(
        &'a self,
        id: &'a str,
    ) -> HttpFuture<'a, Result<openrust::http::UploadedFileValue>> {
        Box::pin(async move { self.resolve()?.get_attachments(id).await })
    }
    fn set_exception<'a>(
        &'a self,
        call: &'a WebhookCall,
        exception: Option<&'a serde_json::Value>,
    ) -> HttpFuture<'a, Result<()>> {
        Box::pin(async move { self.resolve()?.set_exception(call, exception).await })
    }
}
struct DeferredJob {
    app: Arc<Application>,
    config: Arc<WebhookConfig>,
    factory: Arc<Factory<dyn WebhookJobDispatcher>>,
    resolved: Mutex<Option<Arc<dyn WebhookJobDispatcher>>>,
}
impl DeferredJob {
    fn resolve(&self) -> Result<Arc<dyn WebhookJobDispatcher>> {
        let mut resolved = self
            .resolved
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(value) = resolved.as_ref() {
            return Ok(value.clone());
        }
        let job = (self.factory.create)(&self.config, self.app.container())?;
        *resolved = Some(job.clone());
        Ok(job)
    }
}
impl WebhookJobDispatcher for DeferredJob {
    fn prepare(&self, call: &WebhookCall) -> Result<()> {
        self.resolve()?.prepare(call)
    }
    fn dispatch_in_transaction<'a>(
        &'a self,
        call: &'a WebhookCall,
        transaction: &'a mut openrust::database::Transaction,
    ) -> HttpFuture<'a, Result<()>> {
        Box::pin(async move {
            self.resolve()?
                .dispatch_in_transaction(call, transaction)
                .await
        })
    }
    fn dispatch<'a>(&'a self, call: &'a WebhookCall) -> HttpFuture<'a, Result<()>> {
        Box::pin(async move { self.resolve()?.dispatch(call).await })
    }
}
