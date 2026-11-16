use std::{io, process::ExitStatus};

use tokio::process::{Child, Command};

use super::Signal;

#[cfg(unix)]
use rustix::process::Pid;

#[cfg(unix)]
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(unix)]
static INHERIT_PROCESS_GROUP: AtomicBool = AtomicBool::new(false);

/// Called only in the dedicated queue execution process.
#[cfg(unix)]
pub(crate) fn inherit_queue_process_group() {
    INHERIT_PROCESS_GROUP.store(true, Ordering::Relaxed);
}

pub(crate) struct ChildGroup {
    pub(super) child: Child,
    #[cfg(unix)]
    group: Option<Pid>,
    #[cfg(unix)]
    exited: tokio::signal::unix::Signal,
}

impl ChildGroup {
    pub(crate) fn spawn(command: &mut Command) -> io::Result<Self> {
        command.kill_on_drop(true);
        #[cfg(unix)]
        {
            Self::spawn_unix(command, INHERIT_PROCESS_GROUP.load(Ordering::Relaxed))
        }
        #[cfg(not(unix))]
        {
            Ok(Self {
                child: command.spawn()?,
            })
        }
    }

    #[cfg(unix)]
    pub(super) fn spawn_attached(command: &mut Command) -> io::Result<Self> {
        command.kill_on_drop(true);
        // The terminal's foreground group must keep ownership of terminal reads.
        Self::spawn_unix(command, true)
    }

    #[cfg(unix)]
    fn spawn_unix(command: &mut Command, inherit: bool) -> io::Result<Self> {
        let exited = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::child())?;
        if !inherit {
            command.process_group(0);
        }
        let child = command.spawn()?;
        let group = (!inherit)
            .then(|| child.id())
            .flatten()
            .and_then(|id| i32::try_from(id).ok())
            .and_then(Pid::from_raw);
        Ok(Self {
            child,
            group,
            exited,
        })
    }

    pub(crate) async fn wait(&mut self) -> io::Result<ExitStatus> {
        #[cfg(unix)]
        if let Some(group) = self.group {
            use rustix::process::{WaitId, WaitIdOptions, waitid};
            let flags = WaitIdOptions::EXITED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT;
            loop {
                match waitid(WaitId::Pid(group), flags) {
                    Ok(Some(_)) => break,
                    Ok(None) => {}
                    Err(rustix::io::Errno::INTR) => continue,
                    Err(error) => return Err(error.into()),
                }
                if self.exited.recv().await.is_none() {
                    return Err(io::Error::other("child exit notifications closed"));
                }
            }
            // Reap the leader only after signalling its group so the PID cannot be reused.
            self.signal(Signal::Kill)?;
            self.group = None;
        }
        self.child.wait().await
    }

    #[cfg(unix)]
    pub(crate) fn signal(&mut self, signal: Signal) -> io::Result<()> {
        let raw = match signal {
            Signal::Interrupt => libc::SIGINT,
            Signal::Terminate => libc::SIGTERM,
            Signal::Kill => libc::SIGKILL,
            Signal::Raw(raw) => raw,
        };
        if raw <= 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid process signal",
            ));
        }
        let target = match self.group {
            Some(group) => -group.as_raw_pid(),
            None => {
                let Some(pid) = self
                    .child
                    .id()
                    .and_then(|pid| i32::try_from(pid).ok())
                    .and_then(Pid::from_raw)
                else {
                    return Ok(());
                };
                pid.as_raw_pid()
            }
        };
        // The target is a still-owned child PID or its process group. The kernel validates the signal.
        if unsafe { libc::kill(target, raw) } == 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        match error.raw_os_error() {
            Some(libc::ESRCH) => Ok(()),
            #[cfg(target_os = "macos")]
            Some(libc::EPERM)
                if self.group.is_some_and(|group| {
                    use rustix::process::{WaitId, WaitIdOptions, waitid};
                    waitid(
                        WaitId::Pid(group),
                        WaitIdOptions::EXITED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT,
                    )
                    .is_ok_and(|status| status.is_some())
                }) =>
            {
                Ok(())
            }
            _ => Err(error),
        }
    }

    #[cfg(not(unix))]
    pub(crate) fn signal(&mut self, signal: Signal) -> io::Result<()> {
        match signal {
            Signal::Kill | Signal::Terminate => self.child.start_kill(),
            Signal::Interrupt | Signal::Raw(_) => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "process signal unavailable",
            )),
        }
    }
}

#[cfg(unix)]
impl Drop for ChildGroup {
    fn drop(&mut self) {
        if self.group.is_some() {
            let _ = self.signal(Signal::Kill);
        }
    }
}
