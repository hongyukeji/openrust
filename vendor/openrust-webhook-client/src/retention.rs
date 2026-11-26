use std::collections::{BTreeMap, BTreeSet};

use openrust::{
    console::{ConsoleError, ConsoleFuture},
    contracts::foundation::Application as _,
    database::{DatabaseManager, eloquent::ModelPruner},
    foundation::Application,
    support::facades::Config,
};

use crate::{
    DatabaseWebhookStore, Error,
    factories::{database_store_options, uses_builtin_database_store},
    webhook_config::configured_profiles,
};

pub(crate) const MODEL: &str = "openrust_webhook_client::models::WebhookCall";
pub(crate) const MAX_CHUNK: u32 = 500;
pub(crate) struct PruneWebhookCalls;

pub(crate) fn retention_days() -> Result<Option<u32>, ConsoleError> {
    match Config::get("webhook-client.delete_after_days").map_err(ConsoleError::from_source)? {
        None => Ok(Some(30)),
        Some(serde_json::Value::Null) => Ok(None),
        Some(value) => value
            .as_u64()
            .filter(|days| *days <= 36_500)
            .and_then(|days| u32::try_from(days).ok())
            .map(Some)
            .ok_or_else(|| ConsoleError::from_source(Error::InvalidConfig("delete_after_days"))),
    }
}

async fn stores(app: &Application) -> Result<Vec<DatabaseWebhookStore>, ConsoleError> {
    if !uses_builtin_database_store(app.container()).map_err(ConsoleError::from_source)? {
        return Ok(vec![]);
    }
    let Some(configs) = Config::get("webhook-client.configs").map_err(ConsoleError::from_source)?
    else {
        return Ok(vec![]);
    };
    let configs = configured_profiles(&configs).map_err(ConsoleError::from_source)?;
    let manager = app
        .make::<DatabaseManager>()
        .await
        .map_err(ConsoleError::from_source)?;
    let mut targets = BTreeMap::new();
    let mut names = BTreeSet::new();
    for config in configs.iter().rev() {
        let name = config["name"].as_str().expect("配置名称已校验");
        if !names.insert(name) || config["webhook_model"].as_str() != Some("database") {
            continue;
        }
        let (scope, name) = database_store_options(config).map_err(ConsoleError::from_source)?;
        let connection = manager
            .connection(name)
            .map_err(ConsoleError::from_source)?;
        targets
            .entry((connection.get_name().to_owned(), scope))
            .or_insert(connection);
    }
    targets
        .into_iter()
        .map(|((_, scope), connection)| {
            DatabaseWebhookStore::new(connection, scope).map_err(ConsoleError::from_source)
        })
        .collect()
}

impl ModelPruner for PruneWebhookCalls {
    fn count<'a>(&'a self, app: &'a Application) -> ConsoleFuture<'a, Result<u64, ConsoleError>> {
        Box::pin(async move {
            let Some(days) = retention_days()? else {
                return Ok(0);
            };
            let mut total = 0_u64;
            for store in stores(app).await? {
                let count = store
                    .count_prunable(days)
                    .await
                    .map_err(ConsoleError::from_source)?;
                total = total
                    .checked_add(count)
                    .ok_or_else(|| ConsoleError::new("Webhook 清理预演计数溢出"))?;
            }
            Ok(total)
        })
    }

    fn prune_batch<'a>(
        &'a self,
        app: &'a Application,
        chunk: u32,
    ) -> ConsoleFuture<'a, Result<u64, ConsoleError>> {
        Box::pin(async move {
            let Some(days) = retention_days()? else {
                return Ok(0);
            };
            let mut remaining = chunk.min(MAX_CHUNK);
            let initial = remaining;
            for store in stores(app).await? {
                if remaining == 0 {
                    break;
                }
                let removed = store
                    .prune(days, remaining)
                    .await
                    .map_err(ConsoleError::from_source)?;
                let removed = u32::try_from(removed).map_err(ConsoleError::from_source)?;
                remaining = remaining
                    .checked_sub(removed)
                    .ok_or_else(|| ConsoleError::new("Webhook 清理删除条数超过批次限制"))?;
            }
            Ok(u64::from(initial - remaining))
        })
    }

    fn chunk_size(&self, requested: u32) -> u32 {
        requested.min(MAX_CHUNK)
    }
}
