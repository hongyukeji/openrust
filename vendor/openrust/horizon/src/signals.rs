use crate::{Error, Result};

pub(crate) struct ControlSignals {
    #[cfg(unix)]
    pause: tokio::signal::unix::Signal,
    #[cfg(unix)]
    resume: tokio::signal::unix::Signal,
}
impl ControlSignals {
    pub(crate) fn new() -> Result<Self> {
        Ok(Self {
            #[cfg(unix)]
            pause: tokio::signal::unix::signal(tokio::signal::unix::SignalKind::user_defined2())
                .map_err(|_| Error::Process)?,
            #[cfg(unix)]
            resume: tokio::signal::unix::signal(tokio::signal::unix::SignalKind::from_raw(
                rustix::process::Signal::CONT.as_raw(),
            ))
            .map_err(|_| Error::Process)?,
        })
    }
    pub(crate) async fn next(&mut self) -> bool {
        #[cfg(unix)]
        {
            tokio::select! { _=self.pause.recv()=>true, _=self.resume.recv()=>false }
        }
        #[cfg(not(unix))]
        {
            std::future::pending().await
        }
    }
}

/// Graceful Horizon termination targets the supervisor or worker itself. Job
/// subprocesses share its group and must finish before the group is reclaimed.
pub(crate) async fn terminate(process: &openrust::process::InvokedProcess) -> Result<()> {
    if !process.running() {
        return Ok(());
    }
    #[cfg(unix)]
    {
        let pid = process
            .id()
            .and_then(|pid| i32::try_from(pid).ok())
            .and_then(rustix::process::Pid::from_raw)
            .ok_or(Error::Process)?;
        match rustix::process::kill_process(pid, rustix::process::Signal::TERM) {
            Ok(()) | Err(rustix::io::Errno::SRCH) => Ok(()),
            Err(_) => Err(Error::Process),
        }
    }
    #[cfg(not(unix))]
    {
        process
            .signal(openrust::process::Signal::Terminate)
            .await
            .map_err(|_| Error::Process)
    }
}
pub(crate) async fn drain(
    process: openrust::process::InvokedProcess,
    deadline: tokio::time::Instant,
) -> Result<()> {
    while process.running() && tokio::time::Instant::now() < deadline {
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    if process.running() {
        process.stop(std::time::Duration::ZERO).await
    } else {
        process.wait().await
    }
    .map(|_| ())
    .map_err(|_| Error::Process)
}

pub(crate) fn adjustNice(increment: i32) -> Result<()> {
    if increment == 0 {
        return Ok(());
    }
    #[cfg(unix)]
    {
        let current = rustix::process::getpriority_process(None).map_err(|_| Error::Process)?;
        let priority = current.saturating_add(increment).clamp(-20, 19);
        rustix::process::setpriority_process(None, priority).map_err(|_| Error::Process)?;
        // Linux stores niceness per thread. Keep Tokio's existing runtime threads
        // consistent so every subsequently spawned worker inherits the same value.
        #[cfg(target_os = "linux")]
        for thread in std::fs::read_dir("/proc/self/task").map_err(|_| Error::Process)? {
            let thread = thread.map_err(|_| Error::Process)?;
            if let Some(pid) = thread
                .file_name()
                .to_str()
                .and_then(|name| name.parse::<i32>().ok())
                .and_then(rustix::process::Pid::from_raw)
            {
                rustix::process::setpriority_process(Some(pid), priority)
                    .map_err(|_| Error::Process)?;
            }
        }
        Ok(())
    }
    #[cfg(not(unix))]
    {
        Err(Error::Unavailable)
    }
}
