use crate::{AutoScaler, Error, Repository, Result, SupervisorOptions};
use openrust::{
    process::{Factory, InvokedProcess},
    queue::QueueManager,
};
use std::{collections::BTreeMap, path::PathBuf, sync::Arc, time::Duration};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

pub struct WorkerProcess {
    supervisor: String,
    queue: String,
    process: InvokedProcess,
    stopping: Option<Instant>,
    grace: Duration,
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
    pools: BTreeMap<String, ProcessPool>,
    parent: u32,
    initially_paused: bool,
    balanced: BTreeMap<String, Instant>,
    targets: BTreeMap<String, Vec<usize>>,
    failures: u64,
    fast_termination: bool,
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
                        desired: 0,
                        restart_again_at: Instant::now(),
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
            pools,
            parent,
            initially_paused: false,
            balanced: BTreeMap::new(),
            targets: BTreeMap::new(),
            failures: 0,
            fast_termination: false,
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
    pub fn paused(mut self, paused: bool) -> Self {
        self.initially_paused = paused;
        self
    }
    pub async fn run(mut self, shutdown: &CancellationToken) -> Result<()> {
        if self.initially_paused && !self.repository.paused().await? {
            self.repository.pause_supervisor(&self.id).await?;
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
                    if paused { self.repository.pause_supervisor(&self.id).await?; }
                    else { self.repository.continue_supervisor(&self.id).await?; }
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
        let forget = self.repository.forget_supervisor(&self.id).await;
        result.and(cleanup).and(forget)
    }
    async fn tick(&mut self, generation: &Option<String>) -> Result<bool> {
        let memory_limit = self
            .options
            .values()
            .next()
            .ok_or(Error::InvalidConfiguration)?
            .memory as u64;
        if self.memory_usage() > memory_limit as f64 {
            return Err(Error::MemoryLimitExceeded);
        }
        if &self.repository.generation().await? != generation {
            return Ok(false);
        }
        if self.parent > 1 && !parent_is_running(self.parent).await? {
            return Ok(false);
        }
        for command in self.repository.pop_commands(&self.id).await? {
            match command.as_str() {
                "terminate" => return Ok(false),
                "pause" => self.repository.pause_supervisor(&self.id).await?,
                "continue" => self.repository.continue_supervisor(&self.id).await?,
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
        let paused =
            self.repository.paused().await? || self.repository.supervisor_paused(&self.id).await?;
        for pool in self.pools.values_mut() {
            let mut index = 0;
            while index < pool.workers.len() {
                if !pool.workers[index].process.running() {
                    let worker = pool.workers.remove(index);
                    let result = worker.process.wait().await.map_err(|_| Error::Process)?;
                    if worker.stopping.is_none() {
                        pool.restart_again_at = Instant::now() + Duration::from_secs(1);
                    }
                    if result.failed() && worker.stopping.is_none() {
                        self.failures = self.failures.saturating_add(1);
                    }
                } else if pool.workers[index]
                    .stopping
                    .is_some_and(|at| at.elapsed() >= pool.workers[index].grace)
                {
                    pool.workers
                        .remove(index)
                        .process
                        .stop(Duration::ZERO)
                        .await
                        .map_err(|_| Error::Process)?;
                } else {
                    index += 1;
                }
            }
        }
        if !paused {
            let option = self
                .options
                .values()
                .next()
                .cloned()
                .ok_or(Error::InvalidConfiguration)?;
            for queue in option.pools() {
                let pool = &self.pools[&queue];
                if pool.active_process_count() < pool.desired
                    && Instant::now() >= pool.restart_again_at
                {
                    let target = pool.desired;
                    self.resize(&queue, &option, target).await?;
                }
            }
            for (name, options) in self.options.clone() {
                if self
                    .balanced
                    .get(&name)
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
                let targets = AutoScaler::number_of_workers_per_queue(&options, &sizes, &runtimes)?;
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
                    self.scale_pool(&name, &queue, &options, target).await?;
                }
            }
        }
        let workers:Vec<_>=self.pools.values().flat_map(|pool| &pool.workers).map(|w|serde_json::json!({"supervisor":w.supervisor,"queue":w.queue,"pid":w.process.id(),"terminating":w.stopping.is_some()})).collect();
        self.repository.heartbeat_supervisor(&self.id,&serde_json::json!({"name":self.id,"master":self.id.rsplit_once(':').map(|(master,_)|master),"pid":std::process::id(),"status":if paused{"paused"}else{"running"},"workers":workers,"targets":self.targets,"unexpected_exits":self.failures,"options":self.options.values().next(),"processes":self.pools.iter().map(|(queue,pool)|(format!("{}:{queue}",self.options.values().next().expect("validated options").connection),pool.active_process_count())).collect::<BTreeMap<_,_>>() })).await?;
        Ok(true)
    }
    async fn scale_pool(
        &mut self,
        name: &str,
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
        let _ = name;
        self.resize(queue, options, target).await
    }
    async fn resize(
        &mut self,
        queue: &str,
        options: &SupervisorOptions,
        target: usize,
    ) -> Result<()> {
        let pool = self
            .pools
            .get_mut(queue)
            .ok_or(Error::InvalidConfiguration)?;
        pool.desired = target;
        let active = pool.active_process_count();
        if active > target {
            let mut remaining = active - target;
            for worker in pool.workers.iter_mut().filter(|w| w.stopping.is_none()) {
                if remaining == 0 {
                    break;
                }
                crate::signals::terminate(&worker.process).await?;
                worker.stopping = Some(Instant::now());
                remaining -= 1;
            }
        } else {
            for _ in 0..target - active {
                let mut worker_options = options.with_queue(queue);
                // The dedicated supervisor already applied nice; workers inherit it.
                worker_options.nice = 0;
                let args = worker_options.to_worker_command(&self.executable)?;
                let process = self
                    .factory
                    .command(args)
                    .path(options.directory.as_deref().unwrap_or(&self.base))
                    .env([
                        ("APP_BASE_PATH", self.base.as_os_str().to_owned()),
                        ("APP_ENV", self.environment.clone().into()),
                    ])
                    .forever()
                    .quietly()
                    .start()
                    .map_err(|_| Error::Process)?;
                pool.workers.push(WorkerProcess {
                    supervisor: self.id.clone(),
                    queue: queue.into(),
                    process,
                    stopping: None,
                    grace: Duration::from_secs(u64::from(options.timeout) + 5),
                });
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
            failed |= crate::signals::terminate(&worker.process).await.is_err();
        }
        let wait = !self.fast_termination || self.repository.termination_wait().await?;
        for worker in self
            .pools
            .values_mut()
            .flat_map(|pool| pool.workers.drain(..))
        {
            if !wait {
                failed |= worker.process.detach().await.is_err();
                continue;
            }
            failed |= crate::signals::drain(worker.process, deadline)
                .await
                .is_err();
        }
        if failed { Err(Error::Process) } else { Ok(()) }
    }
}

/// A queue pool keeps active and terminating workers in the scaling count.
pub struct ProcessPool {
    workers: Vec<WorkerProcess>,
    desired: usize,
    restart_again_at: Instant,
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
