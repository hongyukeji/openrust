use std::time::Duration;

use openrust::{
    cache::CacheManager,
    console::{Command, CommandDefinition, ConsoleError, ConsoleFuture, Input, Output},
    contracts::{cache::Repository as _, foundation::Application as _},
    foundation::{Application, RuntimeService, wait_for_shutdown},
};
use tokio_util::sync::CancellationToken;

use super::{RESTART, console_error};
use crate::servers::reverb::server::ReverbRuntime;

pub(super) struct StartServer(pub(super) CommandDefinition);

impl Command for StartServer {
    fn definition(&self) -> &CommandDefinition {
        &self.0
    }

    fn handle<'a>(
        &'a self,
        app: &'a Application,
        input: &'a Input,
        _: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            let config = app.config().map_err(console_error)?;
            let mut runtime = ReverbRuntime::from_config(config.as_ref()).map_err(console_error)?;
            if let Some(host) = input.matches().get_one::<String>("host") {
                runtime.settings.host.clone_from(host);
            }
            if let Some(port) = input.matches().get_one::<u16>("port") {
                runtime.settings.port = *port;
            }
            if let Some(path) = input.matches().get_one::<String>("path") {
                runtime.settings.path.clone_from(path);
            }
            let cache = app
                .make::<CacheManager>()
                .await
                .map_err(console_error)?
                .store(None)
                .map_err(console_error)?;
            let restart = cache.get(RESTART).await.map_err(console_error)?;
            let shutdown = CancellationToken::new();
            let mut service = runtime
                .prepare(shutdown.clone())
                .await
                .map_err(console_error)?;
            let restart_signal = async {
                loop {
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    if cache.get(RESTART).await.map_err(console_error)? != restart {
                        return Ok::<(), ConsoleError>(());
                    }
                }
            };
            let stop = tokio::select! {
                biased;
                signal = wait_for_shutdown() => signal.map_err(console_error),
                result = restart_signal => result,
                result = &mut service => {
                    result.map_err(console_error)?;
                    return Ok(0);
                }
            };
            shutdown.cancel();
            service.await.map_err(console_error)?;
            stop?;
            Ok(0)
        })
    }
}
