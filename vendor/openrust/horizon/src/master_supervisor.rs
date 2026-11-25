use crate::{Error, Repository, Result, SupervisorOptions};
use openrust::support::facades::Facade;
use openrust::{
    contracts::events::{Event, Listener},
    events::{EventContext, EventError, EventFuture, EventResult, ListenerResponse},
    process::{Factory, InvokedProcess},
    queue::QueueManager,
};
use std::{collections::BTreeMap, path::PathBuf, sync::Arc, time::Duration};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

/// A child supervisor owns its own worker pools and restarts independently.
pub struct SupervisorProcess {
    name: String,
    options: SupervisorOptions,
    process: Option<InvokedProcess>,
    restart_again_at: Instant,
    dead: bool,
}

/// Master -> supervisor -> process pool -> native queue worker.
pub struct MasterSupervisor {
    repository: Arc<Repository>,
    options: BTreeMap<String, SupervisorOptions>,
    executable: PathBuf,
    base: PathBuf,
    environment: String,
    id: String,
    supervisors: Vec<SupervisorProcess>,
    failures: u64,
    fast_termination: bool,
    job_trim_schedule: Arc<crate::job_trim::JobTrimSchedule>,
}

/// Master state observed after its heartbeat has been persisted.
pub struct MasterSupervisorLooped {
    pub name: String,
    pub paused: bool,
    pub(crate) repository: Arc<Repository>,
    pub(crate) job_trim_schedule: Arc<crate::job_trim::JobTrimSchedule>,
}

impl MasterSupervisorLooped {
    pub const NAME: &str = "MasterSupervisorLooped";

    pub fn new(master: &MasterSupervisor, paused: bool) -> Self {
        Self {
            name: master.id.clone(),
            paused,
            repository: master.repository.clone(),
            job_trim_schedule: master.job_trim_schedule.clone(),
        }
    }
}

impl Event for MasterSupervisorLooped {
    fn name(&self) -> &str {
        Self::NAME
    }
}

pub struct ExpireSupervisors;

impl Listener for ExpireSupervisors {
    fn handle<'a>(
        &'a self,
        event: &'a dyn Event,
        _: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
        Box::pin(async move {
            let Some(event) = event.downcast_ref::<MasterSupervisorLooped>() else {
                return Ok(None);
            };
            async {
                event
                    .repository
                    .master_supervisors()
                    .flush_expired()
                    .await?;
                event
                    .repository
                    .supervisor_repository()
                    .flush_expired()
                    .await
            }
            .await
            .map_err(|source| EventError::Listener {
                source: Box::new(source),
            })?;
            Ok(None)
        })
    }
}

impl MasterSupervisor {
    pub fn new(
        repository: Arc<Repository>,
        _queues: Arc<QueueManager>,
        options: BTreeMap<String, SupervisorOptions>,
        executable: PathBuf,
        base: PathBuf,
        environment: String,
    ) -> Result<Self> {
        for (name, option) in &options {
            if !crate::label(name) {
                return Err(Error::InvalidConfiguration);
            }
            option.validate()?;
        }
        Ok(Self {
            repository,
            options,
            executable,
            base,
            environment,
            id: uuid::Uuid::now_v7().to_string(),
            supervisors: Vec::new(),
            failures: 0,
            fast_termination: false,
            job_trim_schedule: Arc::default(),
        })
    }
    pub fn fast_termination(mut self, enabled: bool) -> Self {
        self.fast_termination = enabled;
        self
    }
    pub fn memory_usage(&self) -> f64 {
        memory_stats::memory_stats().map_or(0., |usage| usage.physical_mem as f64 / 1024. / 1024.)
    }
    pub fn name(&self) -> &str {
        &self.id
    }
    pub async fn run(mut self, shutdown: &CancellationToken) -> Result<()> {
        let generation = self.repository.generation().await?;
        for (name, mut options) in self.options.clone() {
            options.name = format!("{}:{name}", self.id);
            options.parent_id = std::process::id();
            self.supervisors.push(SupervisorProcess {
                name: options.name.clone(),
                options,
                process: None,
                restart_again_at: Instant::now(),
                dead: false,
            });
        }
        let mut control = crate::signals::ControlSignals::new()?;
        let mut ticker = tokio::time::interval(Duration::from_secs(1));
        let result = loop {
            tokio::select! { _ = shutdown.cancelled() => break Ok(()), _ = ticker.tick() => {}, paused=control.next()=>{ if paused {self.repository.pause().await?;}else{self.repository.r#continue().await?;}continue; } }
            match self.tick(&generation).await {
                Ok(true) => {}
                Ok(false) => break Ok(()),
                Err(error) => break Err(error),
            }
        };
        let cleanup = self.stop().await;
        let forget = self.repository.forget_master(&self.id).await;
        result.and(cleanup).and(forget)
    }
    async fn tick(&mut self, generation: &Option<String>) -> Result<bool> {
        if &self.repository.generation().await? != generation {
            return Ok(false);
        }
        let paused = self.repository.paused().await?;
        for supervisor in &mut self.supervisors {
            if supervisor
                .process
                .as_ref()
                .is_some_and(|process| !process.running())
            {
                let result = supervisor
                    .process
                    .take()
                    .expect("observed child")
                    .wait()
                    .await
                    .map_err(|_| Error::Process)?;
                supervisor.dead = matches!(result.exit_code(), 0 | 2 | 13);
                if !supervisor.dead {
                    self.failures = self.failures.saturating_add(1);
                }
                if let Some(previous) = self.repository.supervisor(&supervisor.name).await? {
                    self.failures = self
                        .failures
                        .saturating_add(previous["unexpected_exits"].as_u64().unwrap_or(0));
                }
                self.repository.forget_supervisor(&supervisor.name).await?;
            }
            if supervisor.dead
                || supervisor.process.is_some()
                || Instant::now() < supervisor.restart_again_at
            {
                continue;
            }
            let mut arguments = supervisor.options.to_supervisor_command(&self.executable)?;
            if paused {
                arguments.push("--paused".into());
            }
            supervisor.process = Some(
                Factory::new()
                    .command(arguments)
                    .path(&self.base)
                    .env([("APP_BASE_PATH", self.base.as_os_str().to_owned())])
                    .forever()
                    .quietly()
                    .start()
                    .map_err(|_| Error::Process)?,
            );
            // Failed children cannot spin in a tight restart loop.
            supervisor.restart_again_at = Instant::now() + Duration::from_secs(1);
        }
        let states = self.repository.supervisors().await?;
        let states: Vec<_> = states
            .into_iter()
            .filter(|state| {
                state["name"]
                    .as_str()
                    .is_some_and(|name| name.starts_with(&format!("{}:", self.id)))
            })
            .collect();
        let workers: Vec<_> = states
            .iter()
            .flat_map(|state| state["workers"].as_array().into_iter().flatten().cloned())
            .collect();
        let targets: BTreeMap<_, _> = states
            .iter()
            .filter_map(|state| {
                Some((
                    state["name"].as_str()?.rsplit(':').next()?.to_owned(),
                    state["targets"].as_object()?.values().next()?.clone(),
                ))
            })
            .collect();
        self.repository.heartbeat(&self.id, &serde_json::json!({"name":self.id,"pid":std::process::id(),"status":if paused {"paused"} else {"running"},"environment":self.environment,"supervisors":self.supervisors.iter().filter(|s| !s.dead).map(|s| &s.name).collect::<Vec<_>>(),"workers":workers,"targets":targets,"unexpected_exits":self.failures + states.iter().filter_map(|state| state["unexpected_exits"].as_u64()).sum::<u64>()})).await?;
        if let Err(error) =
            openrust::support::facades::Event::dispatch(MasterSupervisorLooped::new(self, paused))
                .await
        {
            let _ = openrust::support::facades::Log::warning(
                &format!("Horizon master loop listener failed: {error}"),
                &Default::default(),
            );
        }
        let memory_limit = openrust::support::facades::Config::get_facade_root()
            .map_err(|_| Error::Unavailable)?
            .get("horizon.memory_limit")
            .and_then(|value| value.as_u64())
            .unwrap_or(64);
        let memory_usage = self.memory_usage();
        if memory_usage > memory_limit as f64 {
            crate::process_memory::dispatch(crate::MasterSupervisorOutOfMemory {
                name: self.id.clone(),
                memory_usage,
                memory_limit,
            })
            .await;
            return Err(Error::MemoryLimitExceeded);
        }
        Ok(true)
    }
    async fn stop(&mut self) -> Result<()> {
        let deadline = Instant::now()
            + Duration::from_secs(
                self.supervisors
                    .iter()
                    .map(|s| u64::from(s.options.timeout) + 10)
                    .max()
                    .unwrap_or(10),
            );
        let mut failed = false;
        for supervisor in &self.supervisors {
            if let Some(process) = &supervisor.process {
                failed |= crate::signals::terminate(process).await.is_err();
            }
        }
        let wait = !self.fast_termination || self.repository.termination_wait().await?;
        for supervisor in &mut self.supervisors {
            if let Some(process) = supervisor.process.take() {
                if wait {
                    failed |= crate::signals::drain(process, deadline).await.is_err();
                } else {
                    failed |= process.detach().await.is_err();
                }
            }
            failed |= self
                .repository
                .forget_supervisor(&supervisor.name)
                .await
                .is_err();
        }
        if failed { Err(Error::Process) } else { Ok(()) }
    }
}
