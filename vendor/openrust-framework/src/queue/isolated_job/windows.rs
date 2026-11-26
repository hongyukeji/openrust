use std::{io, process::ExitStatus};

use tokio::process::{Child, Command};

use crate::process::windows_job::WindowsJob;
pub(crate) use crate::process::windows_job::wait_for_assignment;

pub(super) struct ChildJob {
    child: Child,
    job: WindowsJob,
}

impl ChildJob {
    pub(super) async fn spawn(command: &mut Command) -> io::Result<Self> {
        let (child, job) = WindowsJob::spawn_gated(command).await?;
        Ok(Self { child, job })
    }

    pub(super) fn terminate(&self) -> io::Result<()> {
        self.job.terminate()
    }

    pub(super) async fn wait(&mut self) -> io::Result<ExitStatus> {
        let status = self.child.wait().await?;
        self.job.reap().await?;
        Ok(status)
    }
}
