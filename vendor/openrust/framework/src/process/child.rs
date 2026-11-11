use std::{io, process::ExitStatus};

use tokio::process::{Child, Command};

use super::Signal;

#[cfg(unix)]
use rustix::process::{Pid, Signal as UnixSignal, kill_process_group};

pub(super) struct ChildGroup {
    pub(super) child: Child,
    #[cfg(unix)]
    group: Option<Pid>,
    #[cfg(unix)]
    exited: tokio::signal::unix::Signal,
}

impl ChildGroup {
    pub(super) fn spawn(command: &mut Command) -> io::Result<Self> {
        command.kill_on_drop(true);
        #[cfg(unix)]
        {
            let exited = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::child())?;
            let child = command.process_group(0).spawn()?;
            let group = child
                .id()
                .and_then(|id| i32::try_from(id).ok())
                .and_then(Pid::from_raw);
            Ok(Self {
                child,
                group,
                exited,
            })
        }
        #[cfg(not(unix))]
        {
            Ok(Self {
                child: command.spawn()?,
            })
        }
    }

    pub(super) async fn wait(&mut self) -> io::Result<ExitStatus> {
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

    pub(super) fn signal(&mut self, signal: Signal) -> io::Result<()> {
        #[cfg(unix)]
        if let Some(group) = self.group {
            let signal = match signal {
                Signal::Interrupt => UnixSignal::INT,
                Signal::Terminate => UnixSignal::TERM,
                Signal::Kill => UnixSignal::KILL,
            };
            return match kill_process_group(group, signal) {
                Ok(()) | Err(rustix::io::Errno::SRCH) => Ok(()),
                #[cfg(target_os = "macos")]
                Err(rustix::io::Errno::PERM)
                    if rustix::process::waitid(
                        rustix::process::WaitId::Pid(group),
                        rustix::process::WaitIdOptions::EXITED
                            | rustix::process::WaitIdOptions::NOHANG
                            | rustix::process::WaitIdOptions::NOWAIT,
                    )?
                    .is_some() =>
                {
                    Ok(())
                }
                Err(error) => Err(error.into()),
            };
        }
        match signal {
            Signal::Kill | Signal::Terminate => self.child.start_kill(),
            Signal::Interrupt => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "process interrupt unavailable",
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
