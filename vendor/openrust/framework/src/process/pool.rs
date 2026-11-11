use std::time::Duration;

use futures_util::future::join_all;
use indexmap::IndexMap;

use super::{Command, Factory, InvokedProcess, PendingProcess, ProcessResult, Result, Signal};

/// The commands to start together, indexed by their registration names.
pub struct Pool {
    factory: Factory,
    processes: IndexMap<String, PendingProcess>,
}

impl Pool {
    pub(super) fn new(factory: Factory) -> Self {
        Self {
            factory,
            processes: IndexMap::new(),
        }
    }

    pub fn r#as(
        &mut self,
        name: impl Into<String>,
        command: impl Into<Command>,
        configure: impl FnOnce(PendingProcess) -> PendingProcess,
    ) -> &mut Self {
        self.processes
            .insert(name.into(), configure(self.factory.command(command)));
        self
    }

    pub fn start(self) -> Result<InvokedProcessPool> {
        let mut running = IndexMap::with_capacity(self.processes.len());
        for (name, pending) in self.processes {
            running.insert(name, pending.start()?);
        }
        Ok(InvokedProcessPool { running })
    }

    pub async fn run(self) -> Result<ProcessPoolResults> {
        Ok(self.start()?.wait().await)
    }
}

/// The live processes in a started pool.
pub struct InvokedProcessPool {
    running: IndexMap<String, InvokedProcess>,
}

pub type ProcessPoolResults = IndexMap<String, Result<ProcessResult>>;

impl InvokedProcessPool {
    pub fn get(&self, name: &str) -> Option<&InvokedProcess> {
        self.running.get(name)
    }

    pub fn running(&self) -> impl Iterator<Item = (&str, &InvokedProcess)> {
        self.running
            .iter()
            .filter_map(|(name, process)| process.running().then_some((name.as_str(), process)))
    }

    pub async fn signal(&self, signal: Signal) -> IndexMap<String, Result<()>> {
        join_all(
            self.running
                .iter()
                .map(|(name, process)| async move { (name.clone(), process.signal(signal).await) }),
        )
        .await
        .into_iter()
        .collect()
    }

    pub async fn stop(self, grace: Duration) -> ProcessPoolResults {
        join_all(
            self.running
                .into_iter()
                .map(|(name, process)| async move { (name, process.stop(grace).await) }),
        )
        .await
        .into_iter()
        .collect()
    }

    pub async fn wait(self) -> ProcessPoolResults {
        join_all(
            self.running
                .into_iter()
                .map(|(name, process)| async move { (name, process.wait().await) }),
        )
        .await
        .into_iter()
        .collect()
    }
}
