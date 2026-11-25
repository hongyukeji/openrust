use crate::{
    AutoScaler, Error, Repository, Result, SupervisorOptions, repository::MasterOwnership,
};
use openrust::{
    contracts::events::Event,
    process::{Factory, InvokedProcess, PendingProcess},
    queue::QueueManager,
};
use std::{collections::BTreeMap, path::PathBuf, sync::Arc, time::Duration};
use tokio::sync::Mutex;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

pub(crate) fn hostname() -> Result<String> {
    hostname::get()
        .ok()
        .and_then(|name| name.into_string().ok())
        .filter(|name| !name.is_empty())
        .ok_or(Error::InvalidConfiguration)
}

pub struct WorkerProcess {
    supervisor: String,
    queue: String,
    process: Option<InvokedProcess>,
    pid: Option<u32>,
    exit_code: Option<i32>,
    restart_again_at: Option<Instant>,
    stopping: Option<Instant>,
    grace: Duration,
}

/// 即将补位的 Worker 退出时的快照。
#[derive(Clone, Debug)]
pub struct WorkerProcessRestarting {
    pub supervisor: String,
    pub queue: String,
    pub pid: Option<u32>,
    pub exit_code: i32,
}

impl WorkerProcessRestarting {
    pub const NAME: &str = "WorkerProcessRestarting";
}

impl Event for WorkerProcessRestarting {
    fn name(&self) -> &str {
        Self::NAME
    }
}

/// Worker 进入较长冷却前最近一次退出的快照。
#[derive(Clone, Debug)]
pub struct UnableToLaunchProcess {
    pub supervisor: String,
    pub queue: String,
    pub pid: Option<u32>,
    pub exit_code: Option<i32>,
}

impl UnableToLaunchProcess {
    pub const NAME: &str = "UnableToLaunchProcess";
}

impl Event for UnableToLaunchProcess {
    fn name(&self) -> &str {
        Self::NAME
    }
}

/// Owns process pools for one deployed supervisor.
pub struct Supervisor {
    repository: Arc<Repository>,
    queues: Arc<QueueManager>,
    options: BTreeMap<String, SupervisorOptions>,
    executable: PathBuf,
    base: PathBuf,
    environment: String,
    factory: Factory,
    id: String,
    host: String,
    pools: BTreeMap<String, ProcessPool>,
    parent: u32,
    ownership: Option<MasterOwnership>,
    initially_paused: bool,
    balanced: BTreeMap<String, Instant>,
    targets: BTreeMap<String, Vec<usize>>,
    failures: u64,
    fast_termination: bool,
    last_wait_monitor: Arc<Mutex<Option<Instant>>>,
}

/// Supervisor state observed after its heartbeat has been persisted.
#[derive(Clone)]
pub struct SupervisorLooped {
    pub(crate) memory_limit: u64,
    pub(crate) memory_check: Arc<crate::process_memory::MemoryCheck>,
    pub name: String,
    pub paused: bool,
    pub(crate) repository: Arc<Repository>,
    pub(crate) last_wait_monitor: Arc<Mutex<Option<Instant>>>,
}

impl SupervisorLooped {
    pub const NAME: &str = "SupervisorLooped";

    pub fn new(supervisor: &Supervisor, paused: bool) -> Self {
        Self {
            name: supervisor.id.clone(),
            paused,
            memory_check: Arc::default(),
            memory_limit: supervisor
                .options
                .values()
                .next()
                .expect("validated options")
                .memory
                .into(),
            repository: supervisor.repository.clone(),
            last_wait_monitor: supervisor.last_wait_monitor.clone(),
        }
    }
}

impl Event for SupervisorLooped {
    fn name(&self) -> &str {
        Self::NAME
    }
}

impl Supervisor {
    pub fn new(
        repository: Arc<Repository>,
        queues: Arc<QueueManager>,
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
        let parent = options.values().next().map_or(0, |option| option.parent_id);
        let pools = options
            .values()
            .flat_map(|option| option.pools())
            .map(|queue| {
                (
                    queue,
                    ProcessPool {
                        workers: Vec::new(),
                    },
                )
            })
            .collect();
        let id = options
            .values()
            .next()
            .map(|option| option.name.clone())
            .ok_or(Error::InvalidConfiguration)?;
        Ok(Self {
            repository,
            queues,
            options,
            executable,
            base,
            environment,
            factory: Factory::new(),
            id,
            host: hostname()?,
            pools,
            parent,
            ownership: None,
            initially_paused: false,
            balanced: BTreeMap::new(),
            targets: BTreeMap::new(),
            failures: 0,
            fast_termination: false,
            last_wait_monitor: Arc::default(),
        })
    }
    pub fn fast_termination(mut self, enabled: bool) -> Self {
        self.fast_termination = enabled;
        self
    }
    pub(crate) fn with_master_ownership(mut self, ownership: Option<MasterOwnership>) -> Self {
        self.ownership = ownership;
        self
    }
    pub fn memory_usage(&self) -> f64 {
        crate::process_memory::memory_usage()
    }
    pub fn name(&self) -> &str {
        &self.id
    }
    pub fn paused(mut self, paused: bool) -> Self {
        self.initially_paused = paused;
        self
    }
    pub async fn run(mut self, shutdown: &CancellationToken) -> Result<()> {
        self.repository
            .check_master_ownership(self.ownership.as_ref())
            .await?;
        if self.initially_paused && !self.repository.paused().await? {
            self.set_paused(true).await?;
        }
        // Upstream provisions an initial balanced worker count before auto-scaling.
        let option = self
            .options
            .values()
            .next()
            .cloned()
            .ok_or(Error::InvalidConfiguration)?;
        let initial =
            option.min_processes.saturating_add(option.max_processes) / 2 / self.pools.len();
        for queue in option.pools() {
            self.resize(&queue, &option, initial).await?;
        }
        let generation = self.repository.generation().await?;
        let mut control = crate::signals::ControlSignals::new()?;
        let mut ticker = tokio::time::interval(Duration::from_secs(1));
        let result = loop {
            tokio::select! {
                _=shutdown.cancelled()=>break Ok(()),
                _=ticker.tick()=>{},
                paused=control.next()=>{
                    if let Err(error) = self.set_paused(paused).await {
                        break Err(error);
                    }
                    continue;
                }
            }
            match self.tick(&generation).await {
                Ok(true) => {}
                Ok(false) => break Ok(()),
                Err(error) => break Err(error),
            }
        };
        let cleanup = self.stop().await;
        let forget = self
            .repository
            .forget_supervisor_for_master(&self.id, self.ownership.as_ref())
            .await;
        result.and(cleanup).and(forget)
    }
    async fn set_paused(&self, paused: bool) -> Result<()> {
        self.repository
            .set_supervisor_paused(&self.id, paused, self.ownership.as_ref())
            .await
    }
    async fn tick(&mut self, generation: &Option<String>) -> Result<bool> {
        if &self.repository.generation().await? != generation {
            return Ok(false);
        }
        if self.parent > 1 && !parent_is_running(self.parent).await? {
            return Ok(false);
        }
        for command in self
            .repository
            .take_supervisor_commands(&self.id, self.ownership.as_ref())
            .await?
        {
            match command.as_str() {
                "terminate" => return Ok(false),
                "pause" => self.set_paused(true).await?,
                "continue" => self.set_paused(false).await?,
                "restart" => {
                    let option = self
                        .options
                        .values()
                        .next()
                        .cloned()
                        .ok_or(Error::InvalidConfiguration)?;
                    for queue in option.pools() {
                        let count = self.pools[&queue].active_process_count();
                        self.resize(&queue, &option, 0).await?;
                        self.resize(&queue, &option, count).await?;
                    }
                }
                _ => return Err(Error::InvalidRecord),
            }
        }
        let paused = self
            .repository
            .reservations_paused(Some(&self.id), self.ownership.as_ref())
            .await?;
        for pool in self.pools.values_mut() {
            let mut index = 0;
            while index < pool.workers.len() {
                let worker = &mut pool.workers[index];
                if worker
                    .process
                    .as_ref()
                    .is_some_and(|process| !process.running())
                {
                    let process = worker.process.take().expect("observed process");
                    let result = process.wait().await.map_err(|_| Error::Process)?;
                    if worker.stopping.is_some() {
                        pool.workers.remove(index);
                        continue;
                    }
                    worker.exit_code = Some(result.exit_code());
                    if result.failed() {
                        self.failures = self.failures.saturating_add(1);
                    }
                } else if pool.workers[index]
                    .stopping
                    .is_some_and(|at| at.elapsed() >= pool.workers[index].grace)
                {
                    pool.workers
                        .remove(index)
                        .process
                        .expect("terminating process")
                        .stop(Duration::ZERO)
                        .await
                        .map_err(|_| Error::Process)?;
                    continue;
                }
                index += 1;
            }
        }
        if !paused {
            let options = self.options.clone();
            for (name, options) in &options {
                if self
                    .balanced
                    .get(name)
                    .is_some_and(|at| at.elapsed() < Duration::from_secs(options.balance_cooldown))
                {
                    continue;
                }
                self.balanced.insert(name.clone(), Instant::now());
                let connection = self
                    .queues
                    .connection(Some(&options.connection))
                    .map_err(|_| Error::Queue)?;
                let mut sizes = Vec::new();
                let mut runtimes = Vec::new();
                for pool in options.pools() {
                    let mut size = 0u64;
                    let mut time = 0.;
                    for queue in pool.split(',') {
                        let metrics = connection
                            .metrics(Some(queue))
                            .await
                            .map_err(|_| Error::Queue)?;
                        let ready = metrics.pending.ok_or(Error::Queue)?;
                        size = size.saturating_add(ready);
                        time += ready as f64
                            * self
                                .repository
                                .metrics(&options.connection, queue)
                                .await?
                                .runtime_for_queue();
                    }
                    sizes.push(size);
                    runtimes.push(if size == 0 { 0. } else { time / size as f64 });
                }
                let targets = AutoScaler::number_of_workers_per_queue(options, &sizes, &runtimes)?;
                let mut pools: Vec<_> = options
                    .pools()
                    .into_iter()
                    .zip(targets.iter().copied())
                    .collect();
                pools.sort_by(|a, b| a.1.total_cmp(&b.1));
                self.targets.insert(
                    name.clone(),
                    targets
                        .iter()
                        .map(|target| target.ceil() as usize)
                        .collect(),
                );
                for (queue, target) in pools {
                    self.scale_pool(&queue, options, target).await?;
                }
            }
            for option in options.values() {
                for queue in option.pools() {
                    self.monitor_workers(&queue, option).await?;
                }
            }
        }
        let workers: Vec<_> = self
            .pools
            .values()
            .flat_map(|pool| &pool.workers)
            .filter_map(|worker| {
                let pid = worker.process.as_ref()?.id()?;
                Some(serde_json::json!({
                    "supervisor": worker.supervisor,
                    "queue": worker.queue,
                    "pid": pid,
                    "terminating": worker.stopping.is_some(),
                }))
            })
            .collect();
        let options = self.options.values().next().expect("validated options");
        let processes = self
            .pools
            .iter()
            .map(|(queue, pool)| {
                (
                    format!("{}:{queue}", options.connection),
                    pool.active_process_count(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let master = self.master_name();
        let status = serde_json::json!({
            "name": self.id,
            "master": master,
            "basename": master.and_then(crate::master_names::basename_from_name),
            "host": self.host,
            "pid": std::process::id(),
            "status": if paused { "paused" } else { "running" },
            "workers": workers,
            "targets": self.targets,
            "unexpected_exits": self.failures,
            "options": options,
            "processes": processes,
        });
        self.repository
            .publish_supervisor(&self.id, &status, self.ownership.as_ref())
            .await?;
        let event = SupervisorLooped::new(self, paused);
        if let Err(error) = openrust::support::facades::Event::dispatch(event.clone()).await {
            let _ = openrust::support::facades::Log::warning(
                &format!("Horizon supervisor loop listener failed: {error}"),
                &Default::default(),
            );
        }
        // 自定义监听器可能提前终止派发，进程仍须检查内存。
        crate::MonitorSupervisorMemory::check(&event)
            .await
            .map_err(|_| Error::Unavailable)?;
        if event.memory_check.exceeded() {
            return Err(Error::MemoryLimitExceeded);
        }
        Ok(true)
    }
    async fn scale_pool(
        &mut self,
        queue: &str,
        options: &SupervisorOptions,
        workers: f64,
    ) -> Result<()> {
        let total_process_count = self.pools[queue].total_process_count();
        let supervisor_process_count = self
            .pools
            .values()
            .map(ProcessPool::total_process_count)
            .sum();
        let Some(target) = AutoScaler::scale_pool(
            options,
            total_process_count,
            supervisor_process_count,
            workers,
        )?
        else {
            return Ok(());
        };
        self.resize(queue, options, target).await
    }
    async fn resize(
        &mut self,
        queue: &str,
        options: &SupervisorOptions,
        target: usize,
    ) -> Result<()> {
        let active = self.pools[queue].active_process_count();
        if active > target {
            let pool = self.pools.get_mut(queue).expect("configured pool");
            let mut remaining = active - target;
            let mut index = 0;
            while remaining > 0 {
                let worker = &mut pool.workers[index];
                if worker.stopping.is_some() {
                    index += 1;
                    continue;
                }
                if let Some(process) = &worker.process {
                    crate::signals::terminate(process).await?;
                    worker.stopping = Some(Instant::now());
                    index += 1;
                } else {
                    pool.workers.remove(index);
                }
                remaining -= 1;
            }
        } else {
            let pool = self.pools.get_mut(queue).expect("configured pool");
            for _ in active..target {
                pool.workers.push(WorkerProcess {
                    supervisor: self.id.clone(),
                    queue: queue.into(),
                    pid: None,
                    process: None,
                    exit_code: None,
                    restart_again_at: None,
                    stopping: None,
                    grace: Duration::from_secs(u64::from(options.timeout) + 5),
                });
            }
        }
        Ok(())
    }

    fn prepare_worker(&self, queue: &str, options: &SupervisorOptions) -> Result<PendingProcess> {
        let mut worker_options = options.with_queue(queue);
        // Worker 继承专用监督器已应用的进程优先级。
        worker_options.nice = 0;
        let args = worker_options.to_worker_command(&self.executable)?;
        let mut process = self
            .factory
            .command(args)
            .path(options.directory.as_deref().unwrap_or(&self.base))
            .env([
                ("APP_BASE_PATH", self.base.as_os_str().to_owned()),
                ("APP_ENV", self.environment.clone().into()),
            ])
            .forever()
            .quietly();
        if let Some(ownership) = &self.ownership {
            process = process.env(ownership.environment());
        }
        Ok(process)
    }

    async fn monitor_workers(&mut self, queue: &str, options: &SupervisorOptions) -> Result<()> {
        for index in 0..self.pools[queue].workers.len() {
            let worker = &self.pools[queue].workers[index];
            if worker.process.is_some()
                || worker.stopping.is_some()
                || (worker.exit_code != Some(0)
                    && worker
                        .restart_again_at
                        .is_some_and(|at| Instant::now() < at))
            {
                continue;
            }
            if let Some(exit_code) = worker.exit_code {
                dispatch_worker_event(WorkerProcessRestarting {
                    supervisor: worker.supervisor.clone(),
                    queue: worker.queue.clone(),
                    pid: worker.pid,
                    exit_code,
                })
                .await;
            }
            let now = Instant::now();
            let restart_again_at = match worker.restart_again_at {
                None => now + Duration::from_secs(1),
                Some(at) if now < at => at,
                Some(_) => {
                    dispatch_worker_event(UnableToLaunchProcess {
                        supervisor: worker.supervisor.clone(),
                        queue: worker.queue.clone(),
                        pid: worker.pid,
                        exit_code: worker.exit_code,
                    })
                    .await;
                    now + Duration::from_secs(60)
                }
            };
            let result = self.prepare_worker(queue, options)?.start();
            let worker = &mut self.pools.get_mut(queue).expect("configured pool").workers[index];
            worker.restart_again_at = Some(restart_again_at);
            worker.exit_code = None;
            worker.pid = None;
            match result {
                Ok(process) => {
                    worker.pid = process.id();
                    worker.process = Some(process);
                }
                Err(error) => {
                    let _ = openrust::support::facades::Log::warning(
                        &format!(
                            "Horizon 无法为监督器 {} 的队列 {queue} 创建 Worker 进程: {error}",
                            self.id
                        ),
                        &Default::default(),
                    );
                }
            }
        }
        Ok(())
    }
    async fn stop(&mut self) -> Result<()> {
        let deadline = Instant::now()
            + self
                .pools
                .values()
                .flat_map(|pool| &pool.workers)
                .map(|w| w.grace)
                .max()
                .unwrap_or_default();
        let mut failed = false;
        for worker in self.pools.values().flat_map(|pool| &pool.workers) {
            if let Some(process) = &worker.process {
                failed |= crate::signals::terminate(process).await.is_err();
            }
        }
        let master = self.master_name().filter(|master| crate::label(master));
        let wait = !self.fast_termination
            || matches!(
                self.repository
                    .check_master_ownership(self.ownership.as_ref())
                    .await,
                Err(Error::MasterOwnershipLost)
            )
            || self
                .repository
                .master_supervisors()
                .termination_wait(master)
                .await?;
        for process in self
            .pools
            .values_mut()
            .flat_map(|pool| pool.workers.drain(..))
            .filter_map(|worker| worker.process)
        {
            if !wait {
                failed |= process.detach().await.is_err();
                continue;
            }
            failed |= crate::signals::drain(process, deadline).await.is_err();
        }
        if failed { Err(Error::Process) } else { Ok(()) }
    }

    fn master_name(&self) -> Option<&str> {
        self.ownership
            .as_ref()
            .map(|ownership| ownership.name.as_str())
            .or_else(|| self.id.split_once(':').map(|(master, _)| master))
    }
}

/// A queue pool keeps active and terminating workers in the scaling count.
pub struct ProcessPool {
    workers: Vec<WorkerProcess>,
}
impl ProcessPool {
    pub fn total_process_count(&self) -> usize {
        self.workers.len()
    }
    pub fn active_process_count(&self) -> usize {
        self.workers
            .iter()
            .filter(|worker| worker.stopping.is_none())
            .count()
    }
}

async fn dispatch_worker_event(event: impl Event + 'static) {
    if let Err(error) = openrust::support::facades::Event::dispatch(event).await {
        let _ = openrust::support::facades::Log::warning(
            &format!("Horizon Worker 监听器失败: {error}"),
            &Default::default(),
        );
    }
}

async fn parent_is_running(parent: u32) -> Result<bool> {
    #[cfg(unix)]
    {
        Ok(rustix::process::getppid().is_some_and(|pid| pid.as_raw_pid() as u32 == parent))
    }
    #[cfg(not(unix))]
    {
        let _ = parent;
        Err(Error::Unavailable)
    }
}
