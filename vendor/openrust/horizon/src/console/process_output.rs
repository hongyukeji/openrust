use crate::process_output::OutputHandler;
use openrust::console::Output;
use openrust::process::OutputStream;
use std::{
    borrow::Cow,
    io,
    sync::{Arc, Mutex},
};

type CapturedOutput = Arc<Mutex<Option<[Vec<u8>; 2]>>>;

pub(super) struct CommandOutput {
    handler: Option<OutputHandler>,
    captured: Option<CapturedOutput>,
}

impl CommandOutput {
    pub(super) fn new(output: &Output) -> Self {
        Self::build(output, false)
    }

    pub(super) fn with_error_output(output: &Output) -> Self {
        Self::build(output, true)
    }

    fn build(output: &Output, separate_stderr: bool) -> Self {
        if output.is_quiet() || output.is_silent() {
            return Self {
                handler: None,
                captured: None,
            };
        }
        let terminal = output.streams_to_terminal();
        let captured = (!terminal).then(|| Arc::new(Mutex::new(Some([Vec::new(), Vec::new()]))));
        let buffer = captured.clone();
        let formatter = output.get_formatter();
        let order = Mutex::new(());
        Self {
            handler: Some(Arc::new(move |stream, bytes| {
                let _order = order
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let stderr = separate_stderr && stream == OutputStream::Stderr;
                // 保留 Cargo 的诊断和无法按 UTF-8 解释的原始字节。
                let mut rendered = Cow::Borrowed(bytes);
                if !stderr
                    && let Ok(message) = std::str::from_utf8(bytes)
                    && let Ok(formatted) = formatter.format(Some(message))
                {
                    rendered = Cow::Owned(formatted.unwrap_or_default().into_bytes());
                }
                if let Some(buffer) = &buffer {
                    if let Some(buffer) = buffer
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .as_mut()
                    {
                        buffer[usize::from(stderr)].extend_from_slice(&rendered);
                    }
                } else if stderr {
                    write(&mut io::stderr().lock(), &rendered);
                } else {
                    write(&mut io::stdout().lock(), &rendered);
                }
            })),
            captured,
        }
    }

    pub(super) fn handler(&self) -> Option<OutputHandler> {
        self.handler.clone()
    }

    pub(super) fn finish(self, output: &mut Output) {
        if let Some(captured) = self.captured
            && let Some([stdout, stderr]) = captured
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take()
        {
            output.stdout.push_str(&String::from_utf8_lossy(&stdout));
            output.stderr.push_str(&String::from_utf8_lossy(&stderr));
        }
    }
}

fn write(writer: &mut impl io::Write, bytes: &[u8]) {
    let _ = writer.write_all(bytes).and_then(|()| writer.flush());
}

pub(super) fn info(output: &mut Output, message: &str) {
    output.info(message);
    if output.streams_to_terminal() && !output.is_quiet() && !output.is_silent() {
        let pending = std::mem::take(&mut output.stdout);
        let _ = output.write_raw(&pending);
    }
}
