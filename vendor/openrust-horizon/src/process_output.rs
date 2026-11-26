use openrust::process::{InvokedProcess, OutputStream, PendingProcess, Result};
use std::{io, sync::Arc};

pub(crate) type OutputHandler = Arc<dyn Fn(OutputStream, &[u8]) + Send + Sync>;

pub(crate) fn start_child(
    slot: &mut Option<InvokedProcess>,
    process: PendingProcess,
    handler: Option<&OutputHandler>,
) -> Result<()> {
    if slot.is_some() {
        return Err(
            io::Error::new(io::ErrorKind::AlreadyExists, "请先取出上次启动的子进程").into(),
        );
    }
    *slot = Some(start(process, handler)?);
    Ok(())
}

pub(crate) fn start(
    process: PendingProcess,
    handler: Option<&OutputHandler>,
) -> Result<InvokedProcess> {
    if let Some(handler) = handler.cloned() {
        process.start_with_streamed_output(move |stream, bytes| handler(stream, bytes))
    } else {
        process.quietly().start()
    }
}
