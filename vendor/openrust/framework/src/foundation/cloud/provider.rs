use super::CloudManager;
use crate::{
    contracts::{config::Repository, container::Container as _},
    foundation::Application,
    http::HttpFuture,
    queue::QueueManager,
    support::{ProviderResult, ServiceProvider},
};
use std::sync::Arc;
pub struct CloudServiceProvider;
impl ServiceProvider for CloudServiceProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            let owner = Arc::downgrade(&app.container_handle());
            app.container_handle()
                .singleton::<CloudManager, _>(move |_| {
                    Ok(Arc::new(CloudManager::new(owner.clone())))
                })?;
            Ok(())
        })
    }
    fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            let owner = app.container_handle();
            if !owner.make::<CloudManager>()?.hosted()? {
                return Ok(());
            }
            let env = owner.make::<crate::config::Environment>()?;
            if let Some(config) = env.get("LARAVEL_CLOUD_MANAGED_QUEUES_CONFIG") {
                let mut value: serde_json::Value = serde_json::from_str(config)?;
                let connection = value
                    .get_mut("connection")
                    .and_then(serde_json::Value::as_object_mut)
                    .ok_or(crate::queue::QueueError::InvalidConfiguration)?;
                if connection
                    .get("after_commit")
                    .is_none_or(serde_json::Value::is_null)
                {
                    connection.insert(
                        "after_commit".into(),
                        environment_value(&env, "CLOUD_QUEUE_AFTER_COMMIT", false.into()),
                    );
                }
                if connection
                    .get("overflow")
                    .is_none_or(serde_json::Value::is_null)
                {
                    connection.insert("overflow".into(),serde_json::json!({
                    "enabled":environment_value(&env,"CLOUD_QUEUE_OVERFLOW_ENABLED",false.into()),
                    "store":environment_value(&env,"CLOUD_QUEUE_OVERFLOW_STORE",serde_json::Value::Null),
                    "always":environment_value(&env,"CLOUD_QUEUE_OVERFLOW_ALWAYS",false.into()),
                    "delete_after_processing":environment_value(&env,"CLOUD_QUEUE_OVERFLOW_DELETE_AFTER_PROCESSING",true.into()),
                }));
                }
                owner
                    .get::<dyn Repository>("config")?
                    .set("queue.connections.cloud", value)?;
            }
            if !owner.make::<CloudManager>()?.uses_managed_queues()? {
                return Ok(());
            }
            owner.extend::<dyn crate::contracts::database::LostConnectionDetector, _>(
                crate::container::ServiceKey::typed::<
                    dyn crate::contracts::database::LostConnectionDetector,
                >(),
                |detector, _| Ok(Arc::new(super::AgentAwareLostConnectionDetector(detector))),
            )?;
            owner
                .make::<QueueManager>()?
                .extend("cloud", |owner, name, raw| {
                    Ok(owner.make::<QueueManager>()?.make_cloud(name, raw)?
                        as Arc<dyn crate::contracts::queue::Queue>)
                });
            Ok(())
        })
    }
}

fn environment_value(
    env: &crate::config::Environment,
    key: &str,
    default: serde_json::Value,
) -> serde_json::Value {
    let Some(raw) = env.get(key) else {
        return default;
    };
    match raw.to_ascii_lowercase().as_str() {
        "true" | "(true)" => true.into(),
        "false" | "(false)" => false.into(),
        "null" | "(null)" => serde_json::Value::Null,
        "empty" | "(empty)" => "".into(),
        _ => raw.into(),
    }
}
