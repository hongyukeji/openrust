use crate::process_output::OutputHandler;
use openrust::console::Output;
use openrust::process::OutputStream;
use std::{
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
        if output.streams_to_terminal() {
            return Self {
                handler: Some(Arc::new(move |stream, bytes| {
                    if separate_stderr && stream == OutputStream::Stderr {
                        write(&mut io::stderr().lock(), bytes);
                    } else {
                        write(&mut io::stdout().lock(), bytes);
                    }
                })),
                captured: None,
            };
        }
        let captured = Arc::new(Mutex::new(Some([Vec::new(), Vec::new()])));
        let buffer = Arc::clone(&captured);
        Self {
            handler: Some(Arc::new(move |stream, bytes| {
                if let Some(buffer) = buffer
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .as_mut()
                {
                    let index = usize::from(separate_stderr && stream == OutputStream::Stderr);
                    buffer[index].extend_from_slice(bytes);
                }
            })),
            captured: Some(captured),
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
