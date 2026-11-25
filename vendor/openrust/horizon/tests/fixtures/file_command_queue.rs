use openrust_horizon::{
    CommandFuture, CommandQueueTarget, Error, HorizonCommandQueue, QueuedCommand, Result,
};
use serde_json::Value;
use std::{
    fs::OpenOptions,
    hash::{DefaultHasher, Hash, Hasher},
    io::{Read as _, Seek as _, Write as _},
    path::PathBuf,
    time::Duration,
};

pub struct FileCommandQueue(pub PathBuf);

impl FileCommandQueue {
    fn change<T>(
        &self,
        target: &CommandQueueTarget,
        update: impl FnOnce(&mut Vec<QueuedCommand>) -> T,
    ) -> Result<T> {
        let mut digest = DefaultHasher::new();
        (target.name(), target.scope()).hash(&mut digest);
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(
                self.0
                    .join(format!("command-queue-{:016x}.json", digest.finish())),
            )
            .map_err(|_| Error::Process)?;
        file.lock().map_err(|_| Error::Process)?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).map_err(|_| Error::Process)?;
        let mut commands = if bytes.is_empty() {
            Vec::new()
        } else {
            serde_json::from_slice(&bytes).map_err(|_| Error::InvalidRecord)?
        };
        let result = update(&mut commands);
        file.rewind().map_err(|_| Error::Process)?;
        file.set_len(0).map_err(|_| Error::Process)?;
        let bytes = serde_json::to_vec(&commands).map_err(|_| Error::InvalidRecord)?;
        file.write_all(&bytes).map_err(|_| Error::Process)?;
        Ok(result)
    }
}

impl HorizonCommandQueue for FileCommandQueue {
    fn push<'a>(
        &'a self,
        target: &'a CommandQueueTarget,
        command: &'a str,
        options: Value,
    ) -> CommandFuture<'a, ()> {
        Box::pin(async move {
            self.change(target, |commands| {
                commands.push(QueuedCommand {
                    command: command.into(),
                    options,
                })
            })
        })
    }

    fn pending<'a>(
        &'a self,
        target: &'a CommandQueueTarget,
    ) -> CommandFuture<'a, Vec<QueuedCommand>> {
        Box::pin(async move {
            let hold = self.0.join("hold-command-read");
            if target.name().starts_with("master:") && hold.exists() {
                std::fs::write(
                    self.0.join("command-read-scope"),
                    target.scope().unwrap_or_default(),
                )
                .map_err(|_| Error::Process)?;
                while hold.exists() {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            }
            self.change(target, std::mem::take)
        })
    }

    fn flush<'a>(&'a self, target: &'a CommandQueueTarget) -> CommandFuture<'a, ()> {
        Box::pin(async move { self.change(target, Vec::clear) })
    }
}
