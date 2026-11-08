//! 默认应用的 HTTP、队列与调度角色共用一次引导与关闭信号。

use super::{serve_http, serve_listener_for_supervisor};
use crate::config::{DatabaseConfig, QueueConfig};
use crate::core::Cancellation;
use crate::database::{Db, DbBackend};
use crate::foundation::http::build_router;
use crate::foundation::shutdown_signal;
use crate::http::health::ServerReadiness;
use crate::queue::{Queue, QueueContributions, QueueWorker};
use crate::schedule::Scheduler;
use crate::{Application, Error, Result};
use std::{sync::Arc, time::Duration};
use tokio::{net::TcpListener, task::JoinSet};

pub(super) async fn start(app: &Application) -> Result<()> {
    let Some((worker, scheduler, queue_grace)) = prepare(app).await? else {
        return serve_http(app, false).await;
    };
    let router = build_router(app)?;
    let listener = TcpListener::bind(app.config().server.bind_addr()).await?;
    let address = listener.local_addr()?;
    let http_grace = Duration::from_secs(app.config().server.shutdown_timeout_secs);
    let readiness = app.state().try_get::<ServerReadiness>()?;
    let stop = Cancellation::new();
    let state = app.state().clone();
    let http_stop = stop.clone();
    let worker_stop = stop.clone();
    let mut roles = JoinSet::new();

    tracing::info!(%address, scheduler = scheduler.is_some(), "应用 HTTP 与队列角色已配置");
    roles.spawn(async move {
        let result = serve_listener_for_supervisor(
            state,
            http_grace,
            listener,
            router,
            http_stop.cancelled(),
        )
        .await;
        ("HTTP", result)
    });
    roles.spawn(async move {
        (
            "队列 worker",
            worker.run_until(wait_for_stop(worker_stop), false).await,
        )
    });
    if let Some(scheduler) = scheduler {
        let schedule_stop = stop.clone();
        roles.spawn(async move {
            (
                "调度器",
                scheduler.run_until(wait_for_stop(schedule_stop)).await,
            )
        });
    }

    let first = tokio::select! {
        () = shutdown_signal() => None,
        exit = roles.join_next() => Some(role_result(exit, true)),
    };
    readiness.stop_accepting();
    stop.cancel();
    let mut failure = first.and_then(std::result::Result::err);
    let drain = async {
        while let Some(exit) = roles.join_next().await {
            if let Err(error) = role_result(Some(exit), false)
                && failure.is_none()
            {
                failure = Some(error);
            }
        }
    };
    let grace = http_grace.max(queue_grace) + Duration::from_secs(1);
    if tokio::time::timeout(grace, drain).await.is_err() {
        readiness.cancel_requests();
        roles.abort_all();
        while roles.join_next().await.is_some() {}
        failure.get_or_insert_with(|| {
            Error::custom("application_shutdown_timeout", "应用角色未在停机期限内退出")
        });
    }
    failure.map_or(Ok(()), Err)
}

async fn prepare(
    app: &Application,
) -> Result<Option<(QueueWorker, Option<Arc<Scheduler>>, Duration)>> {
    if !app.state().container().contains::<Queue>() {
        return Ok(None);
    }
    let config = app.section::<QueueConfig>("queue")?;
    let connection = config.default.clone();
    let selected = Queue::connection_config(&config, &connection)?;
    let has_schedule_entries = !app
        .state()
        .resolve::<QueueContributions>()
        .await?
        .schedules()
        .is_empty();
    let scheduler = if selected.driver == "database" {
        let database = app.config_repository().get("database")?.is_some()
            && app
                .section::<DatabaseConfig>("database")?
                .has_default_connection();
        if !database {
            return if has_schedule_entries {
                Err(Error::custom(
                    "application_schedule_database_missing",
                    "调度器需要配置默认数据库连接",
                ))
            } else {
                Ok(None)
            };
        }
        let db = app.state().resolve::<Db>().await?;
        if !matches!(db.backend()?, DbBackend::MySql | DbBackend::Postgres) {
            tracing::warn!("默认数据库不支持内置数据库队列与调度，HTTP 将单独运行");
            return Ok(None);
        }
        if has_schedule_entries {
            Some(
                app.state()
                    .resolve::<Scheduler>()
                    .await
                    .map_err(|error| failed_role("调度器", &error))?,
            )
        } else {
            None
        }
    } else {
        if has_schedule_entries {
            tracing::warn!("应用已登记定时任务。同进程调度要求默认队列使用 database 驱动");
        }
        None
    };
    let mut queues = vec![selected.queue];
    for queue in &config.embedded_queues {
        if !queues.contains(queue) {
            queues.push(queue.clone());
        }
    }
    let grace = Duration::from_secs(config.shutdown_timeout_secs.saturating_add(2));
    let worker = QueueWorker::prepare(app, config, &connection, queues)
        .await
        .map_err(|error| failed_role("队列 worker", &error))?;
    Ok(Some((worker, scheduler, grace)))
}

async fn wait_for_stop(stop: Cancellation) -> Result<()> {
    stop.cancelled().await;
    Ok(())
}

fn role_result(
    joined: Option<std::result::Result<(&str, Result<()>), tokio::task::JoinError>>,
    unexpected: bool,
) -> Result<()> {
    let (role, result) = joined
        .and_then(std::result::Result::ok)
        .ok_or_else(|| Error::custom("application_role_stopped", "应用角色任务意外中止"))?;
    match result {
        Err(error) => Err(failed_role(role, &error)),
        Ok(()) if unexpected => Err(Error::custom(
            "application_role_stopped",
            format!("{role} 在退出信号前停止"),
        )),
        Ok(()) => Ok(()),
    }
}

fn failed_role(role: &str, error: &Error) -> Error {
    Error::custom(
        "application_role_failed",
        format!("{role} 失败 ({})", error.code()),
    )
}
