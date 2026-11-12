use std::{error::Error, io, path::Path, time::Duration};

use serde::{Deserialize, Serialize};

use crate::{
    container::Container,
    contracts::debug::{Exception, ExceptionHandler},
    contracts::{container::Container as _, queue::Job},
    foundation::Application,
    support::facades::isolated_scope,
};

use super::{ChainUpdate, JobContext, Payload, QueueManager, QueueResult, backend};

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub(crate) use windows::wait_for_assignment;

pub(crate) const COMMAND: &str = "__queue-execute-reservation";

#[derive(Serialize, Deserialize)]
struct Request {
    payload: Payload,
    connection: String,
    queue: String,
    attempts: u32,
    environment: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct Response {
    error: Option<String>,
    release_after: Option<Duration>,
    fail_immediately: bool,
    stop_retries: Option<bool>,
    chain_update: Option<ChainUpdate>,
}

pub(crate) async fn serve(
    mut app: Application,
    directory: Option<&Path>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let directory =
        directory.ok_or_else(|| io::Error::other("missing queue execution directory"))?;
    let request: Request =
        serde_json::from_slice(&tokio::fs::read(directory.join("request.json")).await?)?;
    #[cfg(unix)]
    crate::process::inherit_queue_process_group();
    app.boot_with_environment(request.environment.as_deref())
        .await?;
    let container = app.container_handle();
    let manager = container.make::<QueueManager>()?;
    let mut context = JobContext {
        connection: request.connection,
        queue: request.queue,
        attempts: request.attempts,
        ..Default::default()
    };
    let execution = isolated_scope(container.clone(), async {
        use crate::log::context::Context;
        let repository = Context::repository().map_err(backend)?;
        Context::hooks()
            .map_err(backend)?
            .hydrate(&request.payload.context, &repository);
        let result = manager
            .fire_isolated_job(&mut context, &request.payload)
            .await;
        if let Err(error) = &result
            && !context.fail_immediately
        {
            context.stop_retries = Some(
                container
                    .make::<dyn ExceptionHandler>()
                    .is_ok_and(|handler| handler.shouldStopRetries(&Exception::Error(error))),
            );
        }
        result
    });
    let result = execution.await;
    let response = Response {
        error: result.err().map(|error| {
            let mut text = error.to_string();
            let mut source = error.source();
            while let Some(error) = source {
                text.push_str(": ");
                text.push_str(&error.to_string());
                source = error.source();
            }
            text
        }),
        release_after: context.release_after,
        fail_immediately: context.fail_immediately,
        stop_retries: context.stop_retries,
        chain_update: context.chain_update,
    };
    tokio::fs::write(
        directory.join("response.json"),
        serde_json::to_vec(&response)?,
    )
    .await?;
    Ok(())
}

#[cfg(any(unix, windows))]
pub(super) async fn execute(
    app: &Container,
    job: &mut dyn Job,
    context: &mut JobContext<'_>,
    timeout: Duration,
) -> QueueResult<Option<QueueResult<()>>> {
    use std::path::PathBuf;

    use super::QueueError;
    #[cfg(unix)]
    use crate::process::{ChildGroup, Signal};

    let directory = tempfile::tempdir().map_err(backend)?;
    let environment_file = app
        .get::<PathBuf>("env.file")
        .map_err(QueueError::Unavailable)?;
    let environment = environment_file
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.strip_prefix(".env."))
        .map(str::to_owned);
    let request = Request {
        payload: job.payload().clone(),
        connection: job.get_connection_name().into(),
        queue: job.get_queue().into(),
        attempts: job.attempts(),
        environment,
    };
    let encoded = serde_json::to_vec(&request).map_err(backend)?;
    tokio::fs::write(directory.path().join("request.json"), encoded)
        .await
        .map_err(backend)?;
    let mut command = tokio::process::Command::new(std::env::current_exe().map_err(backend)?);
    command.arg("artisan").arg(COMMAND).arg(directory.path());
    if let Some(root) = environment_file.parent() {
        command.env("APP_BASE_PATH", root);
    }
    #[cfg(unix)]
    let mut child =
        ChildGroup::spawn(command.stdin(std::process::Stdio::null())).map_err(backend)?;
    #[cfg(windows)]
    let mut child = windows::ChildJob::spawn(&mut command)
        .await
        .map_err(backend)?;
    // The driver's lease is already ticking while the executor boots.
    let result = if timeout.is_zero() {
        Ok(child.wait().await)
    } else {
        tokio::time::timeout(timeout, child.wait()).await
    };
    let status = match result {
        Ok(status) => status.map_err(backend)?,
        Err(_) => {
            #[cfg(unix)]
            child.signal(Signal::Kill).map_err(backend)?;
            #[cfg(windows)]
            child.terminate().map_err(backend)?;
            child.wait().await.map_err(backend)?;
            return Ok(None);
        }
    };
    if !status.success() {
        return Err(QueueError::Execution {
            source: Box::new(io::Error::other(format!(
                "queue execution process exited with {status}"
            ))),
        });
    }
    let response: Response = serde_json::from_slice(
        &tokio::fs::read(directory.path().join("response.json"))
            .await
            .map_err(backend)?,
    )
    .map_err(backend)?;
    context.release_after = response.release_after;
    context.fail_immediately = response.fail_immediately;
    context.stop_retries = response.stop_retries;
    context.chain_update = response.chain_update;
    Ok(Some(response.error.map_or(Ok(()), |error| {
        Err(QueueError::Execution {
            source: Box::new(io::Error::other(error)),
        })
    })))
}

#[cfg(not(any(unix, windows)))]
pub(super) async fn execute(
    _: &Container,
    _: &mut dyn Job,
    _: &mut JobContext<'_>,
    _: Duration,
) -> QueueResult<Option<QueueResult<()>>> {
    Err(backend(io::Error::new(
        io::ErrorKind::Unsupported,
        "hard queue timeouts require Unix or Windows",
    )))
}
