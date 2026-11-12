use std::io;

use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

#[cfg(unix)]
use tokio::signal::unix::{Signal, SignalKind, signal};

use super::{QueueManager, Worker, WorkerOptions, events::WorkerInterrupted};

#[cfg(unix)]
use super::events::{WorkerPausing, WorkerResuming};

enum WorkerSignal {
    Interrupt(i32),
    #[cfg(unix)]
    Pause,
    #[cfg(unix)]
    Resume,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum WorkerSignalState {
    Running,
    #[cfg(unix)]
    Paused,
    Stopping,
    Interrupted(i32),
}

pub(super) struct WorkerSignals<'a> {
    connection: &'a str,
    queue: String,
    options: &'a WorkerOptions,
    #[cfg(unix)]
    interrupt: Signal,
    #[cfg(unix)]
    terminate: Signal,
    #[cfg(unix)]
    quit: Signal,
    #[cfg(unix)]
    pause: Signal,
    #[cfg(unix)]
    resume: Signal,
}

impl<'a> WorkerSignals<'a> {
    pub(super) fn new(
        connection: &'a str,
        queue: String,
        options: &'a WorkerOptions,
    ) -> io::Result<Self> {
        Ok(Self {
            connection,
            queue,
            options,
            #[cfg(unix)]
            interrupt: signal(SignalKind::interrupt())?,
            #[cfg(unix)]
            terminate: signal(SignalKind::terminate())?,
            #[cfg(unix)]
            quit: signal(SignalKind::quit())?,
            #[cfg(unix)]
            pause: signal(SignalKind::user_defined2())?,
            #[cfg(unix)]
            resume: signal(SignalKind::from_raw(rustix::process::Signal::CONT.as_raw()))?,
        })
    }

    #[cfg(unix)]
    async fn receive(&mut self) -> io::Result<WorkerSignal> {
        let event = tokio::select! {
            _ = self.interrupt.recv() => WorkerSignal::Interrupt(SignalKind::interrupt().as_raw_value()),
            _ = self.terminate.recv() => WorkerSignal::Interrupt(SignalKind::terminate().as_raw_value()),
            _ = self.quit.recv() => WorkerSignal::Interrupt(SignalKind::quit().as_raw_value()),
            _ = self.pause.recv() => WorkerSignal::Pause,
            _ = self.resume.recv() => WorkerSignal::Resume,
        };
        Ok(event)
    }

    #[cfg(not(unix))]
    async fn receive(&mut self) -> io::Result<WorkerSignal> {
        tokio::signal::ctrl_c().await?;
        Ok(WorkerSignal::Interrupt(2))
    }

    pub(super) async fn monitor(
        &mut self,
        worker: &Worker,
        manager: &QueueManager,
        shutdown: &CancellationToken,
        state: &watch::Sender<WorkerSignalState>,
    ) {
        loop {
            let result = match self.receive().await {
                Ok(WorkerSignal::Interrupt(signal)) => {
                    state.send_replace(WorkerSignalState::Stopping);
                    let result = manager
                        .emit_control_event(WorkerInterrupted {
                            signal,
                            connection_name: self.connection.into(),
                            queue: self.queue.clone(),
                            worker_options: self.options.clone(),
                        })
                        .await;
                    if let Err(error) = result {
                        worker.report_worker_error(&error).await;
                    }
                    state.send_replace(WorkerSignalState::Interrupted(signal));
                    shutdown.cancel();
                    return;
                }
                #[cfg(unix)]
                Ok(WorkerSignal::Pause) => {
                    state.send_replace(WorkerSignalState::Paused);
                    manager
                        .emit_control_event(WorkerPausing {
                            connection_name: self.connection.into(),
                            queue: self.queue.clone(),
                            worker_options: self.options.clone(),
                        })
                        .await
                }
                #[cfg(unix)]
                Ok(WorkerSignal::Resume) => {
                    let result = manager
                        .emit_control_event(WorkerResuming {
                            connection_name: self.connection.into(),
                            queue: self.queue.clone(),
                            worker_options: self.options.clone(),
                        })
                        .await;
                    state.send_replace(WorkerSignalState::Running);
                    result
                }
                Err(error) => {
                    worker.report_worker_error(&super::backend(error)).await;
                    shutdown.cancel();
                    return;
                }
            };
            if let Err(error) = result {
                worker.report_worker_error(&error).await;
            }
        }
    }
}
