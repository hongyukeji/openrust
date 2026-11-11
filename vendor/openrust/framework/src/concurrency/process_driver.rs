use std::{path::PathBuf, time::Duration};

use indexmap::IndexMap;
use serde_json::Value;

use crate::{
    log::context::DehydratedContext,
    process::{Command, Factory},
};

use super::{
    Error, Result, Task,
    protocol::{self, Invocation, Outcome},
};

/// Dispatches registered tasks through an Artisan worker command.
#[derive(Clone)]
pub struct ProcessDriver {
    processes: Factory,
    worker: Command,
    base_path: PathBuf,
}

#[allow(non_snake_case, reason = "Matches Laravel's concurrency driver API")]
impl ProcessDriver {
    pub fn new(
        processes: Factory,
        worker: impl Into<Command>,
        base_path: impl Into<PathBuf>,
    ) -> Self {
        Self {
            processes,
            worker: worker.into(),
            base_path: base_path.into(),
        }
    }

    pub async fn runNamed(
        &self,
        tasks: IndexMap<String, Task>,
        timeout: Option<Duration>,
    ) -> Result<IndexMap<String, Value>> {
        self.run_with_context(tasks, timeout, DehydratedContext::default())
            .await
    }

    pub(super) async fn run_with_context(
        &self,
        tasks: IndexMap<String, Task>,
        timeout: Option<Duration>,
        context: DehydratedContext,
    ) -> Result<IndexMap<String, Value>> {
        let mut payloads = Vec::with_capacity(tasks.len());
        for (name, task) in tasks {
            let key = aws_lc_rs::rand::generate::<[u8; 32]>(&aws_lc_rs::rand::SystemRandom::new())
                .map_err(|_| {
                    Error::Io(std::io::Error::other("could not generate concurrency key"))
                })?
                .expose();
            let invocation = Invocation {
                task,
                context: context.clone(),
                key,
            };
            let input = serde_json::to_vec(&invocation).map_err(Error::Encoding)?;
            if input.len() > protocol::INPUT_LIMIT {
                return Err(Error::PayloadTooLarge);
            }
            payloads.push((name, key, input));
        }
        let mut keys = IndexMap::with_capacity(payloads.len());
        let results = self
            .processes
            .concurrently(|pool| {
                for (name, key, input) in payloads {
                    keys.insert(name.clone(), key);
                    pool.r#as(name, self.worker.clone(), |pending| {
                        let pending = pending
                            .path(&self.base_path)
                            .env([("APP_BASE_PATH", self.base_path.as_os_str())])
                            .input(input);
                        if let Some(timeout) = timeout {
                            pending.timeout(timeout)
                        } else {
                            pending
                        }
                    });
                }
            })
            .await
            .map_err(Error::Process)?;
        results
            .into_iter()
            .map(|(name, result)| {
                let result = result.map_err(Error::Process)?;
                if result.failed() {
                    return Err(Error::ChildFailed {
                        task: name,
                        exit_code: result.exitCode(),
                    });
                }
                match protocol::decode(result.output(), &keys[&name])? {
                    Outcome::Success(value) => Ok((name, value)),
                    Outcome::Failed(message) => Err(Error::TaskFailed {
                        task: name,
                        message,
                    }),
                }
            })
            .collect()
    }
}
