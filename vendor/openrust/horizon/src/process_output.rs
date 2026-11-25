use openrust::process::{InvokedProcess, OutputStream, PendingProcess, Result};
use std::sync::Arc;

pub(crate) type OutputHandler = Arc<dyn Fn(OutputStream, &[u8]) + Send + Sync>;

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
