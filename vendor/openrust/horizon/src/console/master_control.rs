use super::error;
use crate::Repository;
use openrust::{
    console::{ConsoleError, Output},
    contracts::foundation::Application as _,
    foundation::Application,
    queue::QueueManager,
};

#[derive(Clone, Copy)]
pub(super) enum Control {
    Pause,
    Continue,
    Terminate { wait: bool },
}

impl Control {
    pub(super) async fn handle(
        self,
        app: &Application,
        repository: &Repository,
        output: &mut Output,
    ) -> Result<(), ConsoleError> {
        let (action, signal) = match self {
            Self::Pause => ("pause", "USR2"),
            Self::Continue => ("continue", "CONT"),
            Self::Terminate { .. } => ("terminate", "TERM"),
        };
        let host = crate::supervisor::hostname().map_err(error)?;
        let masters = repository.master_supervisors();
        let local: Vec<_> = masters
            .all()
            .await
            .map_err(error)?
            .into_iter()
            .filter(|master| master["host"].as_str() == Some(host.as_str()))
            .collect();
        if local.is_empty() {
            output.info(&format!("No processes to {action}."));
        } else if cfg!(unix) {
            output.info(&format!("Sending {signal} signal to processes."));
        } else {
            output.info(&format!("Requesting Horizon processes to {action}."));
        }

        let fast = matches!(self, Self::Terminate { .. })
            && app
                .config()
                .map_err(error)?
                .get("horizon.fast_termination")
                .and_then(|value| value.as_bool())
                == Some(true);
        // 容器主进程退出时，命令也可能随容器被回收。先发布重启通知，再发送终止信号。
        let restart = if matches!(self, Self::Terminate { .. }) {
            match app.make::<QueueManager>().await {
                Ok(queues) => queues.restart_workers().await.map_err(error),
                Err(cause) => Err(error(cause)),
            }
        } else {
            Ok(())
        };
        for master in local {
            let name = master["name"]
                .as_str()
                .ok_or_else(|| error(crate::Error::InvalidRecord))?;
            let pid = master["pid"]
                .as_u64()
                .and_then(|pid| u32::try_from(pid).ok())
                .filter(|pid| *pid != 0);
            let Some(pid) = pid else {
                output.error("无法向 Horizon Master 发送命令，心跳缺少有效的进程 ID。");
                continue;
            };
            output.stdout.push_str(&format!("Process: {pid}\n"));
            #[cfg(unix)]
            {
                let Some(pid) = i32::try_from(pid)
                    .ok()
                    .and_then(rustix::process::Pid::from_raw)
                else {
                    output.error("无法向 Horizon Master 发送命令，心跳缺少有效的进程 ID。");
                    continue;
                };
                if let Self::Terminate { wait } = self
                    && fast
                {
                    masters
                        .request_termination(name, wait)
                        .await
                        .map_err(error)?;
                }
                let signal = match self {
                    Self::Pause => rustix::process::Signal::USR2,
                    Self::Continue => rustix::process::Signal::CONT,
                    Self::Terminate { .. } => rustix::process::Signal::TERM,
                };
                if let Err(error) = rustix::process::kill_process(pid, signal) {
                    output.error(&format!("Failed to kill process: {pid} ({error})"));
                }
            }
            #[cfg(not(unix))]
            match self {
                Self::Pause => masters.set_paused(name, true).await.map_err(error)?,
                Self::Continue => masters.set_paused(name, false).await.map_err(error)?,
                Self::Terminate { wait } => masters
                    .request_termination(name, fast && wait)
                    .await
                    .map_err(error)?,
            }
        }
        restart
    }
}
