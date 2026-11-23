use crate::{
    File, Options, contracts::Printer, printers::CliPrinter, value_objects::MessageLogged,
};
use openrust::console::{ConsoleError, Output};
use std::{
    fs,
    io::{self, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    time::Duration,
};
use tokio_util::sync::CancellationToken;

/// Why a native log-following session stopped.
#[derive(Debug, PartialEq, Eq)]
pub enum ProcessOutcome {
    Interrupted,
    TimedOut,
}

/// Native replacement for Pail's `tail -F` process and PCNTL signal handling.
#[derive(Default)]
pub struct ProcessFactory {
    shutdown: Option<CancellationToken>,
}
impl ProcessFactory {
    pub fn with_shutdown(mut self, shutdown: Option<CancellationToken>) -> Self {
        self.shutdown = shutdown;
        self
    }

    pub async fn run(
        &self,
        file: &File,
        output: &mut Output,
        base_path: impl Into<PathBuf>,
        options: &Options,
    ) -> Result<ProcessOutcome, ConsoleError> {
        if options.timeout() < 0 {
            return Err(ConsoleError::new(
                "The timeout value must be a valid positive integer or float number.",
            ));
        }
        let timeout = async {
            if options.timeout() == 0 {
                std::future::pending::<()>().await;
            } else {
                tokio::time::sleep(Duration::from_secs(options.timeout() as u64)).await;
            }
        };
        let shutdown = async {
            match &self.shutdown {
                Some(token) => token.cancelled().await,
                None => std::future::pending::<()>().await,
            }
        };
        let terminal = output.streams_to_terminal();
        let signals = async {
            if terminal {
                openrust::foundation::wait_for_shutdown().await
            } else {
                std::future::pending::<io::Result<()>>().await
            }
        };
        tokio::pin!(timeout, shutdown, signals);
        let mut reader = Reader::new(file.path());
        let mut printer = CliPrinter::new(output, base_path);
        let mut remaining = Vec::new();
        loop {
            let read = tokio::task::spawn_blocking(move || {
                let result = reader.read();
                (reader, result)
            });
            let (next, chunk) = tokio::select! {
                biased;
                _ = &mut timeout => return Ok(ProcessOutcome::TimedOut),
                _ = &mut shutdown => return Ok(ProcessOutcome::Interrupted),
                result = &mut signals => { result.map_err(error)?; return Ok(ProcessOutcome::Interrupted); },
                result = read => result.map_err(error)?,
            };
            reader = next;
            let (reset, bytes) = chunk.map_err(error)?;
            if reset {
                remaining.clear();
            }
            let empty = bytes.is_empty();
            remaining.extend(bytes);
            let mut consumed = 0;
            for end in remaining
                .iter()
                .enumerate()
                .filter_map(|(i, b)| (*b == b'\n').then_some(i))
            {
                let line = &remaining[consumed..end];
                if !line.is_empty() {
                    let line = std::str::from_utf8(line).map_err(error)?;
                    let message = MessageLogged::from_json(line).map_err(error)?;
                    if options.accepts(&message) {
                        printer.print(&message)?;
                    }
                }
                consumed = end + 1;
            }
            remaining.drain(..consumed);
            if empty {
                tokio::select! {
                    biased;
                    _ = &mut timeout => return Ok(ProcessOutcome::TimedOut),
                    _ = &mut shutdown => return Ok(ProcessOutcome::Interrupted),
                    result = &mut signals => { result.map_err(error)?; return Ok(ProcessOutcome::Interrupted); },
                    _ = tokio::time::sleep(Duration::from_millis(50)) => {},
                }
            }
        }
    }
}
fn error(error: impl std::fmt::Display) -> ConsoleError {
    ConsoleError::new(error.to_string())
}

struct OpenFile {
    file: fs::File,
    identity: same_file::Handle,
    offset: u64,
}
struct Reader {
    path: PathBuf,
    current: Option<OpenFile>,
    initial: bool,
}
impl Reader {
    fn new(path: &Path) -> Self {
        Self {
            path: path.into(),
            current: None,
            initial: true,
        }
    }
    fn read(&mut self) -> io::Result<(bool, Vec<u8>)> {
        // Drain the old descriptor before following a replacement by name.
        if let Some(current) = &mut self.current {
            let (reset, bytes) = Self::read_chunk(current)?;
            if reset || !bytes.is_empty() {
                return Ok((reset, bytes));
            }
        }
        let candidate = match fs::File::open(&self.path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok((false, vec![])),
            Err(error) => return Err(error),
        };
        let identity = same_file::Handle::from_file(candidate.try_clone()?)?;
        if self
            .current
            .as_ref()
            .is_some_and(|current| current.identity == identity)
        {
            return Ok((false, vec![]));
        }
        match candidate.try_lock_shared() {
            Ok(()) => {}
            Err(std::fs::TryLockError::WouldBlock) => return Ok((false, vec![])),
            Err(std::fs::TryLockError::Error(error)) => return Err(error),
        }
        let mut current = OpenFile {
            file: candidate,
            identity,
            offset: 0,
        };
        let result = if self.initial {
            tail_start(&mut current.file)
        } else {
            Ok(0)
        };
        current.file.unlock()?;
        current.offset = result?;
        current.file.seek(SeekFrom::Start(current.offset))?;
        let reset = !self.initial;
        self.initial = false;
        let (_, bytes) = Self::read_chunk(&mut current)?;
        self.current = Some(current);
        Ok((reset, bytes))
    }
    fn read_chunk(current: &mut OpenFile) -> io::Result<(bool, Vec<u8>)> {
        match current.file.try_lock_shared() {
            Ok(()) => {}
            Err(std::fs::TryLockError::WouldBlock) => return Ok((false, vec![])),
            Err(std::fs::TryLockError::Error(error)) => return Err(error),
        }
        let result = (|| {
            let reset = current.file.metadata()?.len() < current.offset;
            if reset {
                current.offset = 0;
                current.file.seek(SeekFrom::Start(0))?;
            }
            let mut bytes = vec![0; 8192];
            let read = current.file.read(&mut bytes)?;
            bytes.truncate(read);
            current.offset += read as u64;
            Ok((reset, bytes))
        })();
        current.file.unlock()?;
        result
    }
}
// GNU tail's initial last-ten-lines behavior, without loading the entire file.
fn tail_start(file: &mut fs::File) -> io::Result<u64> {
    let len = file.metadata()?.len();
    let mut end = len;
    let mut lines = 0;
    let mut buffer = [0; 8192];
    while end > 0 {
        let start = end.saturating_sub(buffer.len() as u64);
        let count = (end - start) as usize;
        file.seek(SeekFrom::Start(start))?;
        file.read_exact(&mut buffer[..count])?;
        for i in (0..count).rev() {
            if buffer[i] == b'\n' && start + i as u64 + 1 != len {
                lines += 1;
                if lines == 10 {
                    return Ok(start + i as u64 + 1);
                }
            }
        }
        end = start;
    }
    Ok(0)
}
