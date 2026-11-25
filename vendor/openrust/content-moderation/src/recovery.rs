use std::time::Duration;

use openrust::{
    console::{AsyncCommandCallback, ConsoleError, Input},
    contracts::queue::JobHandler,
    database::Db,
    queue::{
        DispatchOptions, DispatchResult, JobContext, JobOptions, Payload, QueueError, QueueFuture,
        QueueManager, QueueResult,
    },
    support::facades::Artisan,
};
use serde::{Deserialize, Serialize};

use crate::{Moderation, ModerationError, ModerationStore, VideoReviewService};

pub(crate) const JOB_NAME: &str = "moderation.video.recover";

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Work {
    application: String,
    tenant: String,
    database: String,
    review: String,
    version: i64,
}

pub(crate) struct RecoverVideoReview;

impl JobHandler for RecoverVideoReview {
    fn handle<'a>(
        &'a self,
        _: &'a mut JobContext<'_>,
        payload: &'a Payload,
    ) -> QueueFuture<'a, QueueResult<()>> {
        Box::pin(async move {
            let work: Work = serde_json::from_value(payload.data().clone())
                .map_err(|_| QueueError::InvalidPayload)?;
            let connection = Db::connection(Some(&work.database)).map_err(|_| storage_error())?;
            let store = ModerationStore::new(connection, &work.application, &work.tenant)
                .map_err(|_| QueueError::InvalidPayload)?;
            let Some(review) = store
                .find(&work.review)
                .await
                .map_err(|_| storage_error())?
            else {
                return Ok(());
            };
            if review.version() == work.version {
                let manager = Moderation::manager().map_err(|_| storage_error())?;
                VideoReviewService::new(store, manager)
                    .advance(&review)
                    .await
                    .map_err(|_| storage_error())?;
            }
            Ok(())
        })
    }
}

pub(crate) fn register_command() -> Result<(), ConsoleError> {
    Artisan::command_with(
        "moderation:recover {application} {tenant} {--database=} {--connection=} {--queue=} {--limit=100}",
        AsyncCommandCallback::new(|app, input, output| {
            Box::pin(async move {
                let application = argument(input, "application")?;
                let tenant = argument(input, "tenant")?;
                let limit = option(input, "limit")
                    .as_deref()
                    .unwrap_or("100")
                    .parse::<u32>()
                    .ok()
                    .filter(|limit| (1..=500).contains(limit))
                    .ok_or_else(|| ConsoleError::new("内容审核恢复数量必须为 1 至 500"))?;
                let database = option(input, "database");
                let connection = Db::connection(database.as_deref())
                    .map_err(|_| ConsoleError::from_source(ModerationError::ReviewStorage))?;
                let database = connection.get_name().to_owned();
                let store = ModerationStore::new(connection, &application, &tenant)
                    .map_err(ConsoleError::from_source)?;
                let queue = app
                    .make::<QueueManager>()
                    .await
                    .map_err(ConsoleError::from_source)?;
                let options = DispatchOptions {
                    connection: option(input, "connection"),
                    queue: option(input, "queue"),
                    ..DispatchOptions::default()
                };
                queue
                    .connection(options.connection.as_deref())
                    .map_err(|_| dispatch_error())?;
                let mut dispatched = 0;
                for claim in store
                    .claim_recovery(limit)
                    .await
                    .map_err(ConsoleError::from_source)?
                {
                    let payload = Payload::new(
                        JOB_NAME,
                        &Work {
                            application: application.clone(),
                            tenant: tenant.clone(),
                            database: database.clone(),
                            review: claim.id,
                            version: claim.version,
                        },
                    )
                    .map_err(|_| dispatch_error())?
                    .with_options(JobOptions {
                        max_tries: Some(1),
                        timeout: Some(Duration::from_secs(125)),
                        ..JobOptions::default()
                    });
                    match queue
                        .dispatch(payload, options.clone(), None)
                        .await
                        .map_err(|_| dispatch_error())?
                    {
                        DispatchResult::Queued(_) | DispatchResult::Completed => dispatched += 1,
                        _ => return Err(dispatch_error()),
                    }
                }
                output
                    .write_raw(&format!("{{\"dispatched\":{dispatched}}}\n"))
                    .map_err(ConsoleError::from_source)?;
                Ok(0)
            })
        }),
    )?;
    Ok(())
}

fn argument(input: &Input, name: &str) -> Result<String, ConsoleError> {
    input
        .argument(name)
        .and_then(|value| value.as_str().map(str::to_owned))
        .ok_or_else(|| ConsoleError::new("缺少内容审核作用域"))
}

fn option(input: &Input, name: &str) -> Option<String> {
    input
        .option(name)
        .and_then(|value| value.as_str().map(str::to_owned))
}

fn dispatch_error() -> ConsoleError {
    ConsoleError::from_source(ModerationError::RecoveryDispatch)
}

fn storage_error() -> QueueError {
    QueueError::Execution {
        source: std::sync::Arc::new(ModerationError::ReviewStorage),
    }
}
