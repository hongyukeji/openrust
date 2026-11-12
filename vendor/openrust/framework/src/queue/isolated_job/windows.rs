use std::{
    io::{self, Read as _},
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    process::{ExitStatus, Stdio},
    ptr,
    time::Duration,
};

use tokio::{
    io::AsyncWriteExt as _,
    process::{Child, Command},
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JobObjectBasicAccountingInformation, JobObjectExtendedLimitInformation,
    QueryInformationJobObject, SetInformationJobObject, TerminateJobObject,
};
use windows_sys::Win32::System::Threading::CREATE_NEW_PROCESS_GROUP;

const ASSIGNED: u8 = 1;

/// The executor cannot start the application until it belongs to the parent's job.
pub(crate) fn wait_for_assignment() -> io::Result<()> {
    let mut ready = [0];
    io::stdin().read_exact(&mut ready)?;
    if ready != [ASSIGNED] {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid queue executor start signal",
        ));
    }
    Ok(())
}

pub(super) struct ChildJob {
    child: Child,
    job: OwnedHandle,
}

impl ChildJob {
    pub(super) async fn spawn(command: &mut Command) -> io::Result<Self> {
        // SAFETY: Null pointers request an unnamed job with a non-inheritable handle.
        let raw = unsafe { CreateJobObjectW(ptr::null(), ptr::null()) };
        if raw.is_null() {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: CreateJobObjectW returned a new, owned handle.
        let job = unsafe { OwnedHandle::from_raw_handle(raw) };
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // SAFETY: The handle is live and the pointer and size match the information class.
        if unsafe {
            SetInformationJobObject(
                job.as_raw_handle(),
                JobObjectExtendedLimitInformation,
                ptr::from_ref(&limits).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        // The Worker owns cancellation; console Ctrl+C must not kill the executor directly.
        let mut child = command
            .creation_flags(CREATE_NEW_PROCESS_GROUP)
            .stdin(Stdio::piped())
            .kill_on_drop(true)
            .spawn()?;
        let process = child
            .raw_handle()
            .ok_or_else(|| io::Error::other("queue executor already exited"))?;
        // SAFETY: Both handles are live. The child waits for a byte on its stdin before booting.
        if unsafe { AssignProcessToJobObject(job.as_raw_handle(), process) } == 0 {
            let error = io::Error::last_os_error();
            let _ = child.kill().await;
            return Err(error);
        }
        child
            .stdin
            .take()
            .expect("executor stdin is piped")
            .write_all(&[ASSIGNED])
            .await?;
        Ok(Self { child, job })
    }

    pub(super) fn terminate(&self) -> io::Result<()> {
        // SAFETY: The job handle remains valid until self is dropped.
        if unsafe { TerminateJobObject(self.job.as_raw_handle(), 1) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    pub(super) async fn wait(&mut self) -> io::Result<ExitStatus> {
        let status = self.child.wait().await?;
        // On ordinary exit the executor may have left descendants. Clean them up as well.
        if self.active_processes()? != 0 {
            self.terminate()?;
            while self.active_processes()? != 0 {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }
        Ok(status)
    }

    fn active_processes(&self) -> io::Result<u32> {
        let mut accounting = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
        // SAFETY: The handle is live and the writable buffer matches the information class.
        if unsafe {
            QueryInformationJobObject(
                self.job.as_raw_handle(),
                JobObjectBasicAccountingInformation,
                ptr::from_mut(&mut accounting).cast(),
                size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                ptr::null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(accounting.ActiveProcesses)
    }
}
