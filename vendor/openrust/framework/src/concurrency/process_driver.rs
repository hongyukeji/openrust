use std::{
    io::{Seek as _, Write as _},
    path::PathBuf,
    time::Duration,
};

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

#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
impl ProcessDriver {
    pub(super) fn launch_detached(
        &self,
        task: Task,
        context: &DehydratedContext,
        environment: Option<&str>,
    ) -> Result<()> {
        let input = serde_json::to_vec(&(task, context))?;
        if input.len() > protocol::INPUT_LIMIT {
            return Err(Error::PayloadTooLarge);
        }
        let mut stdin = tempfile::tempfile()?;
        stdin.write_all(&input)?;
        stdin.rewind()?;
        let executable = std::env::current_exe()?;
        let mut args = ["artisan", protocol::COMMAND, "--detached"]
            .map(str::to_owned)
            .to_vec();
        if let Some(environment) = environment {
            args.push(format!("--env={environment}"));
        }
        #[cfg(windows)]
        {
            crate::process::windows_background::spawn(
                &executable,
                &self.base_path,
                &args,
                Some(stdin),
            )
            .map_err(Error::Io)
        }
        #[cfg(not(windows))]
        {
            use std::process::Stdio;

            let mut command = tokio::process::Command::new(executable);
            command
                .args(args)
                .env("APP_BASE_PATH", &self.base_path)
                .current_dir(&self.base_path)
                .stdin(stdin)
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            #[cfg(unix)]
            {
                // setsid is async-signal-safe and detaches the child before the executable starts.
                unsafe {
                    command.pre_exec(|| rustix::process::setsid().map(|_| ()).map_err(Into::into));
                }
            }
            let mut child = command.spawn()?;
            tokio::spawn(async move {
                let _ = child.wait().await;
            });
            Ok(())
        }
    }

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
        self.run_with_context(tasks, timeout, DehydratedContext::default(), None)
            .await
    }

    pub(super) async fn run_with_context(
        &self,
        tasks: IndexMap<String, Task>,
        timeout: Option<Duration>,
        context: DehydratedContext,
        environment: Option<&str>,
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
            let input = serde_json::to_vec(&invocation)?;
            if input.len() > protocol::INPUT_LIMIT {
                return Err(Error::PayloadTooLarge);
            }
            payloads.push((name, key, input));
        }
        let mut worker = self.worker.clone();
        if let (Command::Arguments(args), Some(environment)) = (&mut worker, environment) {
            args.push(format!("--env={environment}").into());
        }
        let mut keys = IndexMap::with_capacity(payloads.len());
        let results = self
            .processes
            .concurrently(|pool| {
                for (name, key, input) in payloads {
                    keys.insert(name.clone(), key);
                    pool.r#as(name, worker.clone(), |pending| {
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
            .await?;
        results
            .into_iter()
            .map(|(name, result)| {
                let result = result?;
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
