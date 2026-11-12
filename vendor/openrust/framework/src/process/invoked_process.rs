use std::{
    io,
    process::ExitStatus,
    sync::{Arc, Mutex},
    time::Duration,
};

use tokio::{
    io::{AsyncRead, AsyncReadExt as _, AsyncWriteExt as _},
    sync::{mpsc, oneshot},
    task::JoinHandle,
    time::{Instant, sleep_until, timeout},
};

use super::{Command, Error, ProcessResult, Result, child::ChildGroup};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutputStream {
    Stdout,
    Stderr,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Signal {
    Interrupt,
    Terminate,
    Kill,
}

#[derive(Default)]
struct Output {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

pub(super) type OutputCallback = Box<dyn FnMut(OutputStream, &[u8]) + Send>;
type SignalRequest = (Signal, oneshot::Sender<io::Result<()>>);

pub(super) struct Options {
    pub(super) input: Vec<u8>,
    pub(super) timeout: Option<Duration>,
    pub(super) idle_timeout: Option<Duration>,
    pub(super) callback: Option<OutputCallback>,
}

/// A running process and its live output. Dropping it cancels the process.
pub struct InvokedProcess {
    command: Command,
    id: Option<u32>,
    output: Arc<Mutex<Output>>,
    task: JoinHandle<Result<i32>>,
    signals: mpsc::UnboundedSender<SignalRequest>,
    stdout_offset: usize,
    stderr_offset: usize,
}

#[allow(non_snake_case, reason = "Matches Laravel's invoked process API")]
impl InvokedProcess {
    pub(super) fn spawn(command: Command, child: ChildGroup, options: Options) -> Self {
        let id = child.child.id();
        let output = Arc::default();
        let (signals, receiver) = mpsc::unbounded_channel();
        let task = tokio::spawn(drive(child, options, Arc::clone(&output), receiver));
        Self {
            command,
            id,
            output,
            task,
            signals,
            stdout_offset: 0,
            stderr_offset: 0,
        }
    }

    pub(super) fn fake(
        command: Command,
        result: ProcessResult,
        mut callback: Option<OutputCallback>,
    ) -> Self {
        if let Some(callback) = callback.as_mut() {
            if !result.stdout.is_empty() {
                callback(OutputStream::Stdout, &result.stdout);
            }
            if !result.stderr.is_empty() {
                callback(OutputStream::Stderr, &result.stderr);
            }
        }
        let output = Arc::new(Mutex::new(Output {
            stdout: result.stdout,
            stderr: result.stderr,
        }));
        let (signals, _) = mpsc::unbounded_channel();
        let task = tokio::spawn(async move { Ok(result.exit_code) });
        Self {
            command,
            id: None,
            output,
            task,
            signals,
            stdout_offset: 0,
            stderr_offset: 0,
        }
    }

    pub fn command(&self) -> &Command {
        &self.command
    }

    pub fn id(&self) -> Option<u32> {
        self.running().then_some(self.id).flatten()
    }

    pub fn running(&self) -> bool {
        self.id.is_some() && !self.task.is_finished()
    }

    pub fn output(&self) -> Vec<u8> {
        self.read_output().stdout.clone()
    }

    pub fn errorOutput(&self) -> Vec<u8> {
        self.read_output().stderr.clone()
    }

    pub fn latestOutput(&mut self) -> Vec<u8> {
        let latest = self.read_output().stdout[self.stdout_offset..].to_vec();
        self.stdout_offset += latest.len();
        latest
    }

    pub fn latestErrorOutput(&mut self) -> Vec<u8> {
        let latest = self.read_output().stderr[self.stderr_offset..].to_vec();
        self.stderr_offset += latest.len();
        latest
    }

    pub async fn signal(&self, signal: Signal) -> Result<()> {
        if self.running() {
            let (acknowledge, result) = oneshot::channel();
            if self.signals.send((signal, acknowledge)).is_ok() {
                result.await.unwrap_or(Ok(()))?;
            }
        }
        Ok(())
    }

    pub async fn stop(mut self, grace: Duration) -> Result<ProcessResult> {
        self.signal(Signal::Terminate).await?;
        if let Ok(result) = timeout(grace, &mut self.task).await {
            return self.finish(result.map_err(io::Error::other)?);
        }
        self.signal(Signal::Kill).await?;
        self.wait().await
    }

    pub async fn wait(mut self) -> Result<ProcessResult> {
        self.wait_ref().await
    }

    pub(crate) async fn wait_ref(&mut self) -> Result<ProcessResult> {
        let result = (&mut self.task).await.map_err(io::Error::other)?;
        self.finish(result)
    }

    fn finish(&self, status: Result<i32>) -> Result<ProcessResult> {
        let exit_code = status?;
        let output = std::mem::take(&mut *self.read_output());
        Ok(ProcessResult {
            command: self.command.clone(),
            exit_code,
            stdout: output.stdout,
            stderr: output.stderr,
        })
    }

    fn read_output(&self) -> std::sync::MutexGuard<'_, Output> {
        self.output
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl Drop for InvokedProcess {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn drive(
    mut group: ChildGroup,
    mut options: Options,
    output: Arc<Mutex<Output>>,
    mut signals: mpsc::UnboundedReceiver<SignalRequest>,
) -> Result<i32> {
    let mut stdout = group.child.stdout.take();
    let mut stderr = group.child.stderr.take();
    let stdin = group.child.stdin.take();
    let input = write_input(stdin, std::mem::take(&mut options.input));
    tokio::pin!(input);
    let mut writing = true;
    let mut status = None;
    let total_deadline = deadline(options.timeout)?;
    let mut idle_deadline = deadline(options.idle_timeout)?;
    let mut out_buffer = [0; 8192];
    let mut err_buffer = [0; 8192];

    while status.is_none() || stdout.is_some() || stderr.is_some() {
        tokio::select! {
            result = group.wait(), if status.is_none() => status = Some(result?),
            result = &mut input, if writing => {
                result?;
                writing = false;
            }
            result = read(&mut stdout, &mut out_buffer), if stdout.is_some() => {
                let count = result?;
                if count == 0 { stdout = None; } else {
                    record(&output, &mut options.callback, OutputStream::Stdout, &out_buffer[..count]);
                    idle_deadline = deadline(options.idle_timeout)?;
                }
            }
            result = read(&mut stderr, &mut err_buffer), if stderr.is_some() => {
                let count = result?;
                if count == 0 { stderr = None; } else {
                    record(&output, &mut options.callback, OutputStream::Stderr, &err_buffer[..count]);
                    idle_deadline = deadline(options.idle_timeout)?;
                }
            }
            Some((signal, acknowledge)) = signals.recv() => {
                let result = if status.is_none() { group.signal(signal) } else { Ok(()) };
                let _ = acknowledge.send(result);
            }
            () = wait_deadline(total_deadline) => {
                terminate(&mut group, status.is_none()).await?;
                return Err(Error::TimedOut { idle: false });
            }
            () = wait_deadline(idle_deadline) => {
                terminate(&mut group, status.is_none()).await?;
                return Err(Error::TimedOut { idle: true });
            }
        }
    }
    Ok(exit_code(status.expect("process exit status recorded")))
}

async fn terminate(group: &mut ChildGroup, running: bool) -> Result<()> {
    if running {
        group.signal(Signal::Kill)?;
        group.wait().await?;
    }
    Ok(())
}

fn deadline(duration: Option<Duration>) -> Result<Option<Instant>> {
    duration
        .map(|duration| {
            Instant::now()
                .checked_add(duration)
                .ok_or(Error::InvalidTimeout)
        })
        .transpose()
}

async fn wait_deadline(deadline: Option<Instant>) {
    if let Some(deadline) = deadline {
        sleep_until(deadline).await;
    } else {
        std::future::pending().await
    }
}

async fn read(stream: &mut Option<impl AsyncRead + Unpin>, buffer: &mut [u8]) -> io::Result<usize> {
    if let Some(stream) = stream {
        stream.read(buffer).await
    } else {
        Ok(0)
    }
}

async fn write_input(stdin: Option<tokio::process::ChildStdin>, input: Vec<u8>) -> io::Result<()> {
    if let Some(mut stdin) = stdin {
        match stdin.write_all(&input).await {
            Err(error) if error.kind() == io::ErrorKind::BrokenPipe => {}
            result => result?,
        }
    }
    Ok(())
}

fn record(
    output: &Mutex<Output>,
    callback: &mut Option<OutputCallback>,
    stream: OutputStream,
    bytes: &[u8],
) {
    {
        let mut output = output
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match stream {
            OutputStream::Stdout => output.stdout.extend_from_slice(bytes),
            OutputStream::Stderr => output.stderr.extend_from_slice(bytes),
        }
    }
    if let Some(callback) = callback {
        callback(stream, bytes);
    }
}

fn exit_code(status: ExitStatus) -> i32 {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt as _;
        status
            .code()
            .or_else(|| status.signal().map(|signal| 128 + signal))
            .unwrap_or(1)
    }
    #[cfg(not(unix))]
    status.code().unwrap_or(1)
}
