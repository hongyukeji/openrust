use std::{
    io,
    process::ExitStatus,
    sync::{Arc, Mutex},
    time::Duration,
};

use tokio::{
    io::{AsyncRead, AsyncReadExt as _, AsyncWriteExt as _},
    sync::{Notify, mpsc, oneshot},
    task::JoinHandle,
    time::{Instant, sleep_until, timeout},
};

use super::{
    Command, Error, ProcessResult, Result, child::ChildGroup, fake_description::FakePlayback,
};

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
    Raw(i32),
}

#[derive(Default)]
struct Output {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    chunks: Vec<(OutputStream, std::ops::Range<usize>)>,
}

impl Output {
    fn append(&mut self, stream: OutputStream, bytes: &[u8]) {
        let buffer = match stream {
            OutputStream::Stdout => &mut self.stdout,
            OutputStream::Stderr => &mut self.stderr,
        };
        let start = buffer.len();
        buffer.extend_from_slice(bytes);
        self.chunks.push((stream, start..buffer.len()));
    }
}

#[derive(Default)]
struct LiveOutput {
    data: Mutex<Output>,
    changed: Notify,
}

#[derive(Clone, Copy)]
struct Completion {
    exit_code: i32,
    idle_timeout: Option<bool>,
}

type ProcessTask = JoinHandle<Result<Completion>>;

pub(super) type OutputCallback = Box<dyn FnMut(OutputStream, &[u8]) + Send>;
enum SignalRequest {
    Signal(Signal, oneshot::Sender<io::Result<()>>),
    Detach(oneshot::Sender<io::Result<()>>),
}

pub(super) struct Options {
    pub(super) input: Vec<u8>,
    pub(super) timeout: Option<Duration>,
    pub(super) idle_timeout: Option<Duration>,
    pub(super) callback: Option<OutputCallback>,
    pub(super) capture: bool,
}

/// A running process and its live output. Dropping it cancels the process.
pub struct InvokedProcess {
    command: Command,
    id: Option<u32>,
    output: Arc<LiveOutput>,
    task: Option<ProcessTask>,
    completion: Option<Completion>,
    fake: Option<Mutex<FakePlayback>>,
    capture: bool,
    chunk_offset: usize,
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
        let capture = options.capture;
        let task = tokio::spawn(drive(child, options, Arc::clone(&output), receiver));
        Self {
            command,
            id,
            output,
            task: Some(task),
            completion: None,
            fake: None,
            capture,
            chunk_offset: 0,
            signals,
            stdout_offset: 0,
            stderr_offset: 0,
        }
    }

    pub(super) fn fake(
        command: Command,
        result: ProcessResult,
        mut callback: Option<OutputCallback>,
        capture: bool,
    ) -> Self {
        let output = Arc::new(LiveOutput::default());
        let fake = result
            .description
            .as_ref()
            .map(|description| Mutex::new(description.invocation(callback.take(), capture)));
        if fake.is_none() {
            for (stream, bytes) in [
                (OutputStream::Stdout, result.stdout),
                (OutputStream::Stderr, result.stderr),
            ] {
                if !bytes.is_empty() {
                    record(&output, &mut callback, stream, &bytes);
                }
            }
        }
        let (signals, _) = mpsc::unbounded_channel();
        Self {
            command,
            id: None,
            output,
            task: None,
            completion: Some(Completion {
                exit_code: result.exit_code,
                idle_timeout: None,
            }),
            fake,
            capture,
            chunk_offset: 0,
            signals,
            stdout_offset: 0,
            stderr_offset: 0,
        }
    }

    pub fn command(&self) -> &Command {
        &self.command
    }

    pub fn id(&self) -> Option<u32> {
        if let Some(fake) = self.playback() {
            return Some(fake.id);
        }
        self.running().then_some(self.id).flatten()
    }

    pub fn running(&self) -> bool {
        if let Some(mut fake) = self.playback() {
            if fake.remaining_polls == 0 {
                while self.publish_fake_chunk(&mut fake) {}
                return false;
            }
            fake.remaining_polls -= 1;
            self.publish_fake_chunk(&mut fake);
            return true;
        }
        self.id.is_some() && self.task.as_ref().is_some_and(|task| !task.is_finished())
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
        if let Some(mut fake) = self.playback() {
            fake.signals.push(signal);
            return Ok(());
        }
        if self.running() {
            let (acknowledge, result) = oneshot::channel();
            if self
                .signals
                .send(SignalRequest::Signal(signal, acknowledge))
                .is_ok()
            {
                result.await.unwrap_or(Ok(()))?;
            }
        }
        Ok(())
    }

    /// Release process ownership so a background child may finish after this runtime exits.
    /// Output should be disabled before spawning a child intended to outlive its parent.
    pub async fn detach(mut self) -> Result<()> {
        if self.playback().is_some() || !self.running() {
            return Ok(());
        }
        let (acknowledge, result) = oneshot::channel();
        self.signals
            .send(SignalRequest::Detach(acknowledge))
            .map_err(|_| io::Error::other("process control closed"))?;
        result
            .await
            .map_err(|_| io::Error::other("process detach acknowledgement closed"))??;
        // Dropping a JoinHandle detaches the task; Drop must no longer abort it.
        self.task.take();
        Ok(())
    }

    pub fn hasReceivedSignal(&self, signal: Signal) -> bool {
        self.playback()
            .is_some_and(|fake| fake.signals.contains(&signal))
    }

    pub async fn stop(mut self, grace: Duration) -> Result<ProcessResult> {
        self.signal(Signal::Terminate).await?;
        if let Ok(result) = timeout(grace, self.collect()).await {
            return self.finish(result?);
        }
        self.signal(Signal::Kill).await?;
        self.wait().await
    }

    pub async fn wait(mut self) -> Result<ProcessResult> {
        self.wait_ref().await
    }

    pub async fn waitUntil(
        &mut self,
        mut ready: impl FnMut(OutputStream, &[u8]) -> bool,
    ) -> Result<bool> {
        if !self.capture {
            return Err(Error::OutputUnavailable);
        }
        let output = Arc::clone(&self.output);
        loop {
            while let Some((stream, bytes)) = self.next_chunk() {
                if ready(stream, &bytes) {
                    return Ok(true);
                }
            }
            if self
                .playback()
                .is_some_and(|mut fake| self.publish_fake_chunk(&mut fake))
            {
                continue;
            }
            if let Some(status) = self.completion {
                if let Some(mut fake) = self.playback() {
                    fake.remaining_polls = 0;
                }
                if status.idle_timeout.is_some() {
                    self.finish(status)?;
                }
                return Ok(false);
            }
            tokio::select! {
                result = self.collect() => { result?; }
                () = output.changed.notified() => {}
            }
        }
    }

    pub async fn ensureNotTimedOut(&mut self) -> Result<()> {
        if self.task.as_ref().is_some_and(JoinHandle::is_finished) || self.completion.is_some() {
            let status = match self.completion {
                Some(status) => status,
                None => self.collect().await?,
            };
            if status.idle_timeout.is_some() {
                self.finish(status)?;
            }
        }
        Ok(())
    }

    pub async fn waitWithOutput(
        mut self,
        mut callback: impl FnMut(OutputStream, &[u8]),
    ) -> Result<ProcessResult> {
        self.waitUntil(|stream, bytes| {
            callback(stream, bytes);
            false
        })
        .await?;
        self.wait().await
    }

    pub(crate) async fn wait_ref(&mut self) -> Result<ProcessResult> {
        let status = self.collect().await?;
        self.finish(status)
    }

    async fn collect(&mut self) -> Result<Completion> {
        if let Some(mut fake) = self.playback() {
            while self.publish_fake_chunk(&mut fake) {}
            fake.remaining_polls = 0;
        }
        if let Some(status) = self.completion {
            return Ok(status);
        }
        let task = self
            .task
            .as_mut()
            .ok_or_else(|| io::Error::other("process result unavailable"))?;
        let result = task.await;
        self.task = None;
        let status = result.map_err(io::Error::other)??;
        self.completion = Some(status);
        Ok(status)
    }

    fn playback(&self) -> Option<std::sync::MutexGuard<'_, FakePlayback>> {
        self.fake.as_ref().map(|fake| {
            fake.lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
        })
    }

    fn publish_fake_chunk(&self, fake: &mut FakePlayback) -> bool {
        let Some((stream, bytes)) = fake.chunks.pop_front() else {
            return false;
        };
        record(&self.output, &mut fake.callback, stream, &bytes);
        true
    }

    fn next_chunk(&mut self) -> Option<(OutputStream, Vec<u8>)> {
        let chunk = {
            let output = self.read_output();
            let (stream, range) = output.chunks.get(self.chunk_offset)?;
            let buffer = match stream {
                OutputStream::Stdout => &output.stdout,
                OutputStream::Stderr => &output.stderr,
            };
            (*stream, buffer[range.clone()].to_vec())
        };
        self.chunk_offset += 1;
        Some(chunk)
    }

    fn finish(&self, status: Completion) -> Result<ProcessResult> {
        let output = self.read_output();
        let result = ProcessResult {
            command: self.command.clone(),
            exit_code: status.exit_code,
            stdout: output.stdout.clone(),
            stderr: output.stderr.clone(),
            description: None,
        };
        match status.idle_timeout {
            Some(idle) => Err(Error::TimedOut {
                idle,
                result: Box::new(result),
            }),
            None => Ok(result),
        }
    }

    fn read_output(&self) -> std::sync::MutexGuard<'_, Output> {
        self.output
            .data
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl Drop for InvokedProcess {
    fn drop(&mut self) {
        if let Some(task) = &self.task {
            task.abort();
        }
    }
}

async fn drive(
    mut group: ChildGroup,
    mut options: Options,
    output: Arc<LiveOutput>,
    mut signals: mpsc::UnboundedReceiver<SignalRequest>,
) -> Result<Completion> {
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

    let idle_timeout = loop {
        if status.is_some() && stdout.is_none() && stderr.is_none() {
            break None;
        }
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
            Some(request) = signals.recv() => {
                match request {
                    SignalRequest::Signal(signal, acknowledge) => {
                        let result = if status.is_none() { group.signal(signal) } else { Ok(()) };
                        let _ = acknowledge.send(result);
                    }
                    SignalRequest::Detach(acknowledge) => { let _ = acknowledge.send(group.detach()); }
                }
            }
            () = wait_deadline(total_deadline) => {
                break Some(false);
            }
            () = wait_deadline(idle_deadline) => {
                break Some(true);
            }
        }
    };
    let status = match status {
        Some(status) => status,
        None => {
            group.signal(Signal::Kill)?;
            group.wait().await?
        }
    };
    Ok(Completion {
        exit_code: exit_code(status),
        idle_timeout,
    })
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
    output: &LiveOutput,
    callback: &mut Option<OutputCallback>,
    stream: OutputStream,
    bytes: &[u8],
) {
    output
        .data
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .append(stream, bytes);
    output.changed.notify_one();
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
