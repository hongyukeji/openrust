use std::{io, os::windows::io::AsRawHandle, process::Stdio, ptr, time::Duration};

use tokio::process::Command;
use windows_sys::Win32::{
    Foundation::{ERROR_BROKEN_PIPE, ERROR_PIPE_NOT_CONNECTED},
    System::Pipes::PeekNamedPipe,
};

const SHUTDOWN_PIPE: &str = "OPENRUST_PROCESS_SHUTDOWN_PIPE";

pub(super) struct ShutdownSender(Option<io::PipeWriter>);

impl ShutdownSender {
    pub(super) fn attach(command: &mut Command) -> io::Result<Self> {
        let (reader, writer) = io::pipe()?;
        command.stdin(Stdio::from(reader)).env(SHUTDOWN_PIPE, "1");
        Ok(Self(Some(writer)))
    }

    pub(super) fn close(&mut self) {
        self.0.take();
    }
}

/// 由 `PendingProcess::with_shutdown_pipe` 启动的子进程可监听父进程关停。
#[derive(Clone, Copy)]
pub struct ParentShutdown(());

impl ParentShutdown {
    pub fn inherited() -> Option<Self> {
        (std::env::var(SHUTDOWN_PIPE).as_deref() == Ok("1")).then_some(Self(()))
    }

    pub fn is_closed(&self) -> io::Result<bool> {
        let stdin = io::stdin();
        // SAFETY: stdin 在调用期间有效，其他空指针对应可选的输出缓冲区。
        let connected = unsafe {
            PeekNamedPipe(
                stdin.as_raw_handle(),
                ptr::null_mut(),
                0,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            )
        };
        if connected != 0 {
            return Ok(false);
        }
        let error = io::Error::last_os_error();
        if matches!(
            error.raw_os_error().map(|code| code as u32),
            Some(ERROR_BROKEN_PIPE | ERROR_PIPE_NOT_CONNECTED)
        ) {
            Ok(true)
        } else {
            Err(error)
        }
    }

    pub async fn closed(&self) -> io::Result<()> {
        while !self.is_closed()? {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        Ok(())
    }
}
