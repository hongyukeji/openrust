use std::{collections::VecDeque, sync::Arc};

use super::{Factory, OutputStream, ProcessResult, invoked_process::OutputCallback};

/// An ordered sequence of output lines and the exit status of a fake process.
#[derive(Clone, Debug)]
pub struct FakeProcessDescription {
    id: u32,
    iterations: usize,
    chunks: Vec<(OutputStream, Vec<u8>)>,
    exit_code: i32,
}

impl Default for FakeProcessDescription {
    fn default() -> Self {
        Self {
            id: 1000,
            iterations: 0,
            chunks: Vec::new(),
            exit_code: 0,
        }
    }
}

impl FakeProcessDescription {
    pub fn id(mut self, id: u32) -> Self {
        self.id = id;
        self
    }

    pub fn runs_for(mut self, iterations: usize) -> Self {
        self.iterations = iterations;
        self
    }

    pub fn iterations(self, iterations: usize) -> Self {
        self.runs_for(iterations)
    }

    pub fn exit_code(mut self, code: i32) -> Self {
        self.exit_code = code;
        self
    }

    pub fn output(mut self, line: impl AsRef<[u8]>) -> Self {
        self.push(OutputStream::Stdout, line.as_ref());
        self
    }

    pub fn error_output(mut self, line: impl AsRef<[u8]>) -> Self {
        self.push(OutputStream::Stderr, line.as_ref());
        self
    }

    pub fn replace_output(mut self, output: impl AsRef<[u8]>) -> Self {
        self.replace(OutputStream::Stdout, output.as_ref());
        self
    }

    pub fn replace_error_output(mut self, output: impl AsRef<[u8]>) -> Self {
        self.replace(OutputStream::Stderr, output.as_ref());
        self
    }

    /// Use this result with a fake registration or a fake sequence.
    pub fn result(self) -> ProcessResult {
        let mut result = Factory::result(Vec::new(), Vec::new(), self.exit_code);
        for (stream, bytes) in &self.chunks {
            match stream {
                OutputStream::Stdout => result.stdout.extend_from_slice(bytes),
                OutputStream::Stderr => result.stderr.extend_from_slice(bytes),
            }
        }
        result.description = Some(Arc::new(self));
        result
    }

    fn replace(&mut self, stream: OutputStream, output: &[u8]) {
        self.chunks.retain(|(kind, _)| *kind != stream);
        if !output.is_empty() {
            self.push(stream, output);
        }
    }

    fn push(&mut self, stream: OutputStream, line: &[u8]) {
        let end = line
            .iter()
            .rposition(|&byte| byte != b'\n')
            .map_or(0, |index| index + 1);
        let mut bytes = line[..end].to_vec();
        bytes.push(b'\n');
        self.chunks.push((stream, bytes));
    }

    pub(super) fn invocation(
        &self,
        callback: Option<OutputCallback>,
        deliver: bool,
    ) -> FakePlayback {
        FakePlayback {
            id: self.id,
            remaining_polls: self.iterations,
            chunks: if deliver {
                self.chunks.iter().cloned().collect()
            } else {
                VecDeque::new()
            },
            callback,
            signals: Vec::new(),
        }
    }
}

pub(super) struct FakePlayback {
    pub(super) id: u32,
    pub(super) remaining_polls: usize,
    pub(super) chunks: VecDeque<(OutputStream, Vec<u8>)>,
    pub(super) callback: Option<OutputCallback>,
    pub(super) signals: Vec<super::Signal>,
}
