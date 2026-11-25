use crate::{
    AutoScaler, Error, Repository, Result, SupervisorOptions, process_output::OutputHandler,
    repository::MasterOwnership, supervisor_commands::Command,
};
use openrust::{
    contracts::events::Event,
    process::{Factory, InvokedProcess, OutputStream, PendingProcess},
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
    output: Option<OutputHandler>,
}

impl WorkerProcess {
    /// 创建一个 Worker 包装器。排空时使用指定的宽限期。
    pub fn new(supervisor: impl Into<String>, queue: impl Into<String>, grace: Duration) -> Self {
        Self {
            supervisor: supervisor.into(),
            queue: queue.into(),
            process: None,
            pid: None,
            exit_code: None,
            restart_again_at: None,
            stopping: None,
            grace,
            output: None,
        }
    }

    /// 设置后续启动使用的原始输出回调。
    pub fn handle_output_using(
        &mut self,
        handler: impl Fn(OutputStream, &[u8]) + Send + Sync + 'static,
    ) -> &mut Self {
        self.output = Some(Arc::new(handler));
        self
    }

    /// 启动已配置的子进程，复用当前输出回调。
    pub fn start(&mut self, process: PendingProcess) -> openrust::process::Result<()> {
        crate::process_output::start_child(&mut self.process, process, self.output.as_ref())?;
        self.pid = self.process.as_ref().and_then(InvokedProcess::id);
        Ok(())
    }

    pub fn process(&self) -> Option<&InvokedProcess> {
        self.process.as_ref()
    }

    /// 将本次子进程交给调用方等待或关闭。
    pub fn take_process(&mut self) -> Option<InvokedProcess> {
        self.process.take()
    }
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
    #[cfg(unix)]
    parent: u32,
    #[cfg(windows)]
    parent_shutdown: Option<openrust::process::ParentShutdown>,
    ownership: Option<MasterOwnership>,
    initially_paused: bool,
    balanced: BTreeMap<String, Instant>,
    targets: BTreeMap<String, Vec<usize>>,
    failures: u64,
    fast_termination: bool,
    output: Option<crate::process_output::OutputHandler>,
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
            if !crate::master_names::valid_process_name(name)
                || !crate::master_names::valid_process_name(&option.name)
            {
                return Err(Error::InvalidConfiguration);
            }
            option.validate()?;
        }
        #[cfg(unix)]
        let parent = options.values().next().map_or(0, |option| option.parent_id);
        #[cfg(windows)]
        let parent_shutdown = options
            .values()
            .next()
            .filter(|option| option.parent_id != 0)
            .map(|_| {
                openrust::process::ParentShutdown::inherited().ok_or(Error::InvalidConfiguration)
            })
            .transpose()?;
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
            #[cfg(unix)]
            parent,
            #[cfg(windows)]
            parent_shutdown,
            ownership: None,
            initially_paused: false,
            balanced: BTreeMap::new(),
            targets: BTreeMap::new(),
            failures: 0,
            fast_termination: false,
            output: None,
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
    /// 注册受管 Worker 的 stdout 和 stderr 回调。
    pub fn handle_output_using(
        mut self,
        handler: impl Fn(openrust::process::OutputStream, &[u8]) + Send + Sync + 'static,
    ) -> Self {
        self.output = Some(Arc::new(handler));
        self
    }
    pub(crate) fn with_output_handler(
        mut self,
        handler: Option<crate::process_output::OutputHandler>,
    ) -> Self {
        self.output = handler;
        self
    }
    /// 将输出片段送往已注册的回调。
    pub fn output(&self, stream: openrust::process::OutputStream, bytes: &[u8]) {
        if let Some(handler) = &self.output {
            handler(stream, bytes);
        }
    }
    pub fn paused(mut self, paused: bool) -> Self {
        self.initially_paused = paused;
        self
    }
    pub async fn run(self, shutdown: &CancellationToken) -> Result<()> {
        self.run_with_status(shutdown).await.map(|_| ())
    }

    pub async fn run_with_status(mut self, shutdown: &CancellationToken) -> Result<i32> {
        let queue = crate::repository::command_queue::configured()?;
        if self.ownership.is_some() {
            // Master 已在启动子进程前清空队列，保留启动期间收到的新命令。
            self.repository
                .check_master_ownership(self.ownership.as_ref())
                .await?;
        } else if let Some(queue) = &queue {
            self.repository
                .prepare_custom_commands(queue.as_ref(), &self.id, None)
                .await?;
        } else {
            self.repository
                .flush_structured_supervisor_commands(&self.id, None)
                .await?;
        }
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
        #[cfg(windows)]
        let parent_closed = crate::signals::parent_closed(self.parent_shutdown);
        #[cfg(not(windows))]
        let parent_closed = std::future::pending::<std::io::Result<()>>();
        tokio::pin!(parent_closed);
        let result = loop {
            tokio::select! {
                biased;
                closed = &mut parent_closed => break closed.map(|()| 0).map_err(|_| Error::Process),
                _=shutdown.cancelled()=>break Ok(0),
                paused=control.next()=>{
                    if let Err(error) = self.set_paused(paused).await {
                        break Err(error);
                    }
                    continue;
                },
                _=ticker.tick()=>{},
            }
            match self.tick(&generation, queue.as_deref()).await {
                Ok(None) => {}
                Ok(Some(status)) => break Ok(status),
                Err(error) => break Err(error),
            }
        };
        let cleanup = self.stop().await;
        let flushed = if let Some(queue) = queue {
            queue
                .flush(&crate::CommandQueueTarget::for_owner(
                    &self.id,
                    self.ownership.as_ref(),
                ))
                .await
        } else {
            Ok(())
        };
        let forget = self
            .repository
            .forget_supervisor_for_master(&self.id, self.ownership.as_ref())
            .await;
        result.and_then(|status| cleanup.and(flushed).and(forget).map(|()| status))
    }
    async fn set_paused(&self, paused: bool) -> Result<()> {
        self.repository
            .set_supervisor_paused(&self.id, paused, self.ownership.as_ref())
            .await
    }
    async fn tick(
        &mut self,
        generation: &Option<String>,
        queue: Option<&dyn crate::HorizonCommandQueue>,
    ) -> Result<Option<i32>> {
        if &self.repository.generation().await? != generation {
            return Ok(Some(0));
        }
        #[cfg(unix)]
        if self.parent > 1
            && rustix::process::getppid().is_none_or(|pid| pid.as_raw_pid() as u32 != self.parent)
        {
            return Ok(Some(0));
        }
        for command in self
            .repository
            .take_supervisor_commands(&self.id, self.ownership.as_ref())
            .await?
        {
            if let Some(status) = self.apply_command(Command::legacy(&command)?).await? {
                return Ok(Some(status));
            }
        }
        let incoming = if let Some(queue) = queue {
            self.repository
                .take_custom_commands(queue, &self.id, self.ownership.as_ref())
                .await?
        } else {
            self.repository
                .take_structured_supervisor_commands(&self.id, self.ownership.as_ref())
                .await?
        };
        for command in incoming {
            match self.process_queued_command(command).await {
                Ok(Some(status)) => return Ok(Some(status)),
                Ok(None) => {}
                Err(error @ (Error::InvalidRecord | Error::InvalidConfiguration)) => {
                    let _ = openrust::support::facades::Log::warning(
                        &format!("Horizon Supervisor 命令未执行: {error}"),
                        &Default::default(),
                    );
                }
                Err(error) => return Err(error),
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
        Ok(None)
    }

    async fn process_queued_command(&mut self, item: crate::QueuedCommand) -> Result<Option<i32>> {
        if let Some(handler) =
            crate::command_handler::resolve::<dyn crate::SupervisorCommandHandler>(&item.command)?
        {
            return handler
                .process(self, item.options)
                .await
                .map(|status| status.map(i32::from));
        }
        self.apply_command(Command::queued(item)?).await
    }

    async fn apply_command(&mut self, command: Command) -> Result<Option<i32>> {
        match command {
            Command::Pause => self.set_paused(true).await?,
            Command::Continue => self.set_paused(false).await?,
            Command::Terminate(status) => return Ok(Some(status)),
            Command::Restart => {
                self.set_paused(false).await?;
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
            Command::Balance(targets) => self.balance(targets).await?,
            Command::Scale(processes) => {
                let count = self.pools.len();
                let option = self
                    .options
                    .values_mut()
                    .next()
                    .ok_or(Error::InvalidConfiguration)?;
                option.max_processes = option.max_processes.max(processes).max(count);
                let targets = option
                    .pools()
                    .into_iter()
                    .map(|queue| (queue, processes / count))
                    .collect();
                self.balance(targets).await?;
            }
        }
        Ok(None)
    }

    async fn balance(&mut self, targets: Vec<(String, usize)>) -> Result<()> {
        let (name, options) = self
            .options
            .iter()
            .next()
            .map(|(name, options)| (name.clone(), options.clone()))
            .ok_or(Error::InvalidConfiguration)?;
        for (queue, count) in targets {
            if self.pools.contains_key(&queue) {
                self.resize(&queue, &options, count).await?;
            }
        }
        self.targets.insert(
            name,
            options
                .pools()
                .iter()
                .map(|queue| self.pools[queue].active_process_count())
                .collect(),
        );
        Ok(())
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
                let mut worker = WorkerProcess::new(
                    self.id.clone(),
                    queue,
                    Duration::from_secs(u64::from(options.timeout) + 5),
                );
                worker.output = self.output.clone();
                pool.workers.push(worker);
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
            .forever();
        if let Some(ownership) = &self.ownership {
            process = process.env(ownership.environment());
        }
        #[cfg(windows)]
        let process = process.with_shutdown_pipe();
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
            let process = self.prepare_worker(queue, options)?;
            let worker = &mut self.pools.get_mut(queue).expect("configured pool").workers[index];
            worker.restart_again_at = Some(restart_again_at);
            worker.exit_code = None;
            worker.pid = None;
            match worker.start(process) {
                Ok(()) => {}
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
        let master = self.master_name();
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
            .or_else(|| crate::master_names::supervisor_parent(&self.id))
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
