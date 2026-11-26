use std::sync::Arc;

use openrust::{
    container::Container,
    contracts::{container::Container as _, foundation::Application as _},
    support::facades::{App, Facade as _},
};

use crate::{
    BackoffStrategy, CallWebhookJob, DefaultSigner, Error, ExponentialBackoffStrategy, Result,
    Signer, StandardWebhookSigner,
};

struct NamedSigner(Arc<dyn Signer>);
struct NamedBackoff(Arc<dyn BackoffStrategy>);
struct NamedJob(Arc<dyn CallWebhookJob>);

pub fn register_signer(container: &Container, name: &str, signer: Arc<dyn Signer>) -> Result<()> {
    let key = key("signer", name)?;
    if matches!(name, "default" | "standard") {
        return Err(Error::InvalidConfig("signer"));
    }
    container
        .instance_named(&key, Arc::new(NamedSigner(signer)))
        .map(|_| ())
        .map_err(|_| Error::InvalidConfig("signer"))
}

pub fn register_backoff_strategy(
    container: &Container,
    name: &str,
    backoff: Arc<dyn BackoffStrategy>,
) -> Result<()> {
    let key = key("backoff_strategy", name)?;
    if name == "exponential" {
        return Err(Error::InvalidConfig("backoff_strategy"));
    }
    container
        .instance_named(&key, Arc::new(NamedBackoff(backoff)))
        .map(|_| ())
        .map_err(|_| Error::InvalidConfig("backoff_strategy"))
}

pub fn register_call_webhook_job(
    container: &Container,
    name: &str,
    job: Arc<dyn CallWebhookJob>,
) -> Result<()> {
    let binding = key("job", name)?;
    if name == "default" {
        return Err(Error::InvalidConfig("delivery.job"));
    }
    container
        .instance_named(&binding, Arc::new(NamedJob(job)))
        .map(|_| ())
        .map_err(|_| Error::InvalidConfig("delivery.job"))
}

pub(super) fn call_webhook_job(name: &str) -> Result<Arc<dyn CallWebhookJob>> {
    Ok(application_binding::<NamedJob>("job", name)
        .map_err(|_| Error::InvalidConfig("delivery.job"))?
        .0
        .clone())
}

pub(super) fn signer(name: &str, header: &str) -> Result<Arc<dyn Signer>> {
    match name {
        "default" => Ok(Arc::new(DefaultSigner::new(header)?)),
        "standard" => Ok(Arc::new(StandardWebhookSigner)),
        name => Ok(application_binding::<NamedSigner>("signer", name)?
            .0
            .clone()),
    }
}

pub(super) fn backoff(name: &str) -> Result<Arc<dyn BackoffStrategy>> {
    match name {
        "exponential" => Ok(Arc::new(ExponentialBackoffStrategy)),
        name => Ok(
            application_binding::<NamedBackoff>("backoff_strategy", name)?
                .0
                .clone(),
        ),
    }
}

fn application_binding<T: Send + Sync + 'static>(
    field: &'static str,
    name: &str,
) -> Result<Arc<T>> {
    let key = key(field, name)?;
    let app = App::get_facade_root().map_err(|_| Error::InvalidConfig(field))?;
    app.container()
        .get::<T>(key.as_str())
        .map_err(|_| Error::InvalidConfig(field))
}

fn key(kind: &'static str, name: &str) -> Result<String> {
    validate_name(kind, name)?;
    Ok(format!("webhook-server.{kind}.{name}"))
}

pub(super) fn validate_name(field: &'static str, name: &str) -> Result<()> {
    if name.is_empty()
        || name.len() > 128
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.:".contains(&byte))
    {
        return Err(Error::InvalidConfig(field));
    }
    Ok(())
}
