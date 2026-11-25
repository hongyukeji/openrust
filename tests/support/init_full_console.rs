use std::{io::Write as _, path::PathBuf, time::Duration};

use openrust::{
    console::AsyncCommandCallback,
    support::facades::{Cache, Schedule},
};

fn init_full_artifact(name: &str) -> Result<PathBuf, ConsoleError> {
    let root = std::env::var_os("OPENRUST_INIT_TEST_ARTIFACTS")
        .ok_or_else(|| ConsoleError::new("缺少验收产物目录"))?;
    Ok(PathBuf::from(root).join(name))
}

fn fixture_error(error: impl std::fmt::Display) -> ConsoleError {
    ConsoleError::new(error.to_string())
}

fn init_full_acceptance_commands() -> Result<(), ConsoleError> {
    Artisan::command_with(
        "init-full-test:migration-lock",
        AsyncCommandCallback::new(|_, _, _| {
            Box::pin(async {
                let mut lock =
                    Cache::lock("framework/command-migrate", Some(Duration::from_secs(180)))
                        .map_err(fixture_error)?;
                if !lock.get().await.map_err(fixture_error)? {
                    return Err(ConsoleError::new("验收迁移锁被占用"));
                }
                std::fs::write(init_full_artifact("migration-ready")?, [])
                    .map_err(fixture_error)?;
                let release = init_full_artifact("migration-release")?;
                tokio::time::timeout(Duration::from_secs(180), async {
                    while !release.exists() {
                        tokio::time::sleep(Duration::from_millis(20)).await;
                    }
                })
                .await
                .map_err(fixture_error)?;
                lock.release().await.map_err(fixture_error)?;
                Ok(0)
            })
        }),
    )?;
    Artisan::command_with(
        "init-full-test:enqueue",
        AsyncCommandCallback::new(|_, _, _| {
            Box::pin(async {
                Artisan::queue("init-full-test:consume", &[])
                    .on_connection("redis")
                    .on_queue("init-full-recovery")
                    .dispatch()
                    .await
                    .map_err(fixture_error)?;
                Ok(0)
            })
        }),
    )?;
    Artisan::command_with(
        "init-full-test:consume",
        AsyncCommandCallback::new(|_, _, _| {
            Box::pin(async {
                if std::env::var_os("OPENRUST_INIT_TEST_PAUSE_WORKER").is_some() {
                    std::fs::write(init_full_artifact("worker-ready")?, [])
                        .map_err(fixture_error)?;
                    tokio::time::sleep(Duration::from_secs(45)).await;
                }
                std::fs::write(init_full_artifact("worker-finished")?, [])
                    .map_err(fixture_error)?;
                Ok(0)
            })
        }),
    )?;
    Artisan::command_with(
        "init-full-test:scheduled",
        AsyncCommandCallback::new(|_, _, _| {
            Box::pin(async {
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(init_full_artifact("scheduler")?)
                    .map_err(fixture_error)?
                    .write_all(b"tick\n")
                    .map_err(fixture_error)?;
                Ok(0)
            })
        }),
    )?;
    Schedule::configure(|schedule| {
        schedule
            .command("init-full-test:scheduled", [])
            .every_minute()
            .on_one_server();
        Ok(())
    })
}
