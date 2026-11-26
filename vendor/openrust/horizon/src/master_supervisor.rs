use crate::{
    Error, Repository, Result, SupervisorOptions, process_output::OutputHandler,
    repository::MasterLease,
};
use openrust::{
    contracts::events::{Event, Listener},
    events::{EventContext, EventError, EventFuture, EventResult, ListenerResponse},
    process::{Factory, InvokedProcess, OutputStream, PendingProcess},
    queue::QueueManager,
};
use std::{collections::BTreeMap, path::PathBuf, sync::Arc, time::Duration};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

/// A child supervisor owns its own worker pools and restarts independently.
pub struct SupervisorProcess {
    /// 冷却期结束后直接终止监控的退出码。退出码 13 始终立即终止。
    pub dont_restart_on: Vec<i32>,
    name: String,
    options: SupervisorOptions,
    process: Option<InvokedProcess>,
    restart_again_at: Instant,
    exit: Option<(i32, Option<u32>)>,
    dead: bool,
    from_command: bool,
    restarting: bool,
    output: Option<OutputHandler>,
}

impl SupervisorProcess {
    pub fn new(options: SupervisorOptions, process: Option<InvokedProcess>) -> Self {
        Self {
            dont_restart_on: vec![0, 2, 13],
            name: options.name.clone(),
            options,
            process,
            restart_again_at: Instant::now(),
            exit: None,
            dead: false,
            from_command: false,
            restarting: false,
            output: None,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn options(&self) -> &SupervisorOptions {
        &self.options
    }

    pub fn process(&self) -> Option<&InvokedProcess> {
        self.process.as_ref()
    }

    /// 将本次子进程交给调用方等待或关闭。
    pub fn take_process(&mut self) -> Option<InvokedProcess> {
        self.process.take()
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
        if self.process.is_none() {
            self.restart_again_at = Instant::now() + Duration::from_secs(1);
        }
        crate::process_output::start_child(&mut self.process, process, self.output.as_ref())?;
        self.exit = None;
        self.dead = false;
        Ok(())
    }

    pub fn cooling_down(&self) -> bool {
        Instant::now() < self.restart_again_at
    }

    /// 监控独立包装器。异常退出后向指定 Master 提交原部署选项。
    pub async fn monitor(
        &mut self,
        process: PendingProcess,
        repository: &Repository,
        master: &str,
    ) -> Result<()> {
        if crate::master_names::supervisor_suffix(master, &self.name).is_none()
            || !crate::master_names::valid_process_name(&self.name)
            || !crate::master_names::valid_process_name(master)
        {
            return Err(Error::InvalidConfiguration);
        }
        if self.dead {
            return Ok(());
        }
        if let Some(exit) = self.poll_exit().await? {
            if !self.terminal(exit.0)
                && let Err(error) = self.reprovision(repository, master, exit.1).await
            {
                self.exit = Some(exit);
                return Err(error);
            }
            self.dead = true;
        } else if self.process.is_none() && self.exit.is_none() && !self.cooling_down() {
            self.start(process).map_err(|_| Error::Process)?;
        }
        Ok(())
    }

    async fn poll_exit(&mut self) -> Result<Option<(i32, Option<u32>)>> {
        if self
            .process
            .as_ref()
            .is_some_and(|process| !process.running())
        {
            let process = self.take_process().expect("observed child");
            let pid = process.spawned_id();
            let result = process.wait().await.map_err(|_| Error::Process)?;
            self.exit = Some((result.exit_code(), pid));
        }
        if self
            .exit
            .is_some_and(|(status, _)| status == 13 || !self.cooling_down())
        {
            return Ok(self.exit.take());
        }
        Ok(None)
    }

    fn terminal(&self, status: i32) -> bool {
        status == 13 || self.dont_restart_on.contains(&status)
    }

    async fn reprovision(
        &self,
        repository: &Repository,
        master: &str,
        pid: Option<u32>,
    ) -> Result<()> {
        let options = self.options.to_array()?;
        let target = MasterSupervisor::command_queue_for(Some(master))?;
        if repository
            .forget_exited_supervisor(&self.name, master, pid)
            .await?
        {
            repository
                .push_structured_command(&target, AddSupervisor::NAME, options)
                .await?;
        }
        Ok(())
    }

    pub async fn pause(&self) -> Result<()> {
        crate::signals::set_paused(self.process(), true).await
    }

    pub async fn continue_working(&self) -> Result<()> {
        crate::signals::set_paused(self.process(), false).await
    }

    pub async fn terminate(&self) -> Result<()> {
        if let Some(process) = self.process() {
            crate::signals::terminate(process).await?;
        }
        Ok(())
    }

    pub async fn stop(&mut self) -> Result<()> {
        let grace = Duration::from_secs(u64::from(self.options.timeout) + 10);
        crate::signals::stop_child(&mut self.process, grace).await
    }

    /// 向监督器的结构化队列提交带退出码的终止命令。
    pub async fn terminate_with_status(&self, repository: &Repository, status: u8) -> Result<()> {
        let command = crate::supervisor_commands::TERMINATE;
        let options = serde_json::json!({"status": status});
        repository
            .push_structured_command(&self.name, command, options)
            .await
    }

    pub fn is_dead(&self) -> bool {
        self.dead
    }
}

/// 在运行中的 Master 上登记并启动额外的 Supervisor。
pub struct AddSupervisor;

impl AddSupervisor {
    pub const NAME: &str = "Laravel\\Horizon\\MasterSupervisorCommands\\AddSupervisor";

    pub fn process(&self, master: &mut MasterSupervisor, options: serde_json::Value) -> Result<()> {
        let mut options = SupervisorOptions::from_array(options)?;
        let Some(suffix) = crate::master_names::supervisor_suffix(&master.id, &options.name) else {
            return Err(Error::InvalidConfiguration);
        };
        if !crate::master_names::valid_process_name(&options.name) {
            return Err(Error::InvalidConfiguration);
        }
        let existing = master
            .supervisors
            .iter_mut()
            .find(|child| child.name == options.name);
        if existing
            .as_ref()
            .map_or_else(|| master.options.contains_key(suffix), |child| !child.dead)
        {
            return Ok(());
        }
        options.parent_id = std::process::id();
        if let Some(directory) = &mut options.directory
            && directory.is_relative()
        {
            *directory = master.base.join(&*directory);
        }
        let mut child = SupervisorProcess::new(options, None);
        child.from_command = true;
        child.output = master.output.clone();
        if let Some(existing) = existing {
            *existing = child;
        } else {
            master.supervisors.push(child);
        }
        Ok(())
    }
}

/// 公开事件，供调用方显式派发 Master 恢复通知。
#[derive(Clone, Debug)]
pub struct MasterSupervisorReviving {
    pub master: String,
}

impl MasterSupervisorReviving {
    pub const NAME: &str = "MasterSupervisorReviving";

    pub fn new(master: impl Into<String>) -> Self {
        Self {
            master: master.into(),
        }
    }
}

impl Event for MasterSupervisorReviving {
    fn name(&self) -> &str {
        Self::NAME
    }
}

/// 公开事件，供调用方显式派发并共享所观察的 Supervisor 进程。
#[derive(Clone)]
pub struct SupervisorProcessRestarting {
    pub process: Arc<SupervisorProcess>,
}

impl SupervisorProcessRestarting {
    pub const NAME: &str = "SupervisorProcessRestarting";

    pub fn new(process: Arc<SupervisorProcess>) -> Self {
        Self { process }
    }
}

impl Event for SupervisorProcessRestarting {
    fn name(&self) -> &str {
        Self::NAME
    }
}

/// Master -> supervisor -> process pool -> native queue worker.
pub struct MasterSupervisor {
    repository: Arc<Repository>,
    options: BTreeMap<String, SupervisorOptions>,
    deployment: Option<crate::provisioning_plan::Deployment>,
    executable: PathBuf,
    base: PathBuf,
    environment: String,
    id: String,
    basename: String,
    host: String,
    supervisors: Vec<SupervisorProcess>,
    failures: u64,
    fast_termination: bool,
    output: Option<OutputHandler>,
    job_trim_schedule: Arc<crate::job_trim::JobTrimSchedule>,
}

/// Master 已准备好部署计划中的监督器。
#[derive(Clone, Debug)]
pub struct MasterSupervisorDeployed {
    pub master: String,
    pub environment: String,
    pub supervisors: Vec<String>,
}

impl MasterSupervisorDeployed {
    pub const NAME: &str = "MasterSupervisorDeployed";
}

impl Event for MasterSupervisorDeployed {
    fn name(&self) -> &str {
        Self::NAME
    }
}

/// Master state observed after its heartbeat has been persisted.
#[derive(Clone)]
pub struct MasterSupervisorLooped {
    pub(crate) memory_check: Arc<crate::process_memory::MemoryCheck>,
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
            memory_check: Arc::default(),
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
        let host = crate::supervisor::hostname()?;
        let basename = repository.master_names.basename(&host)?;
        Ok(Self {
            repository,
            deployment: None,
            options,
            executable,
            base,
            environment,
            id: crate::master_names::process_name(&basename),
            basename,
            host,
            supervisors: Vec::new(),
            failures: 0,
            fast_termination: false,
            output: None,
            job_trim_schedule: Arc::default(),
        })
    }
    pub(crate) fn with_deployment(
        mut self,
        deployment: Option<crate::provisioning_plan::Deployment>,
    ) -> Self {
        self.deployment = deployment;
        self
    }
    pub fn fast_termination(mut self, enabled: bool) -> Self {
        self.fast_termination = enabled;
        self
    }
    pub fn memory_usage(&self) -> f64 {
        crate::process_memory::memory_usage()
    }
    pub fn name(&self) -> &str {
        &self.id
    }

    /// 返回当前应用解析的 Master 名称前缀。
    pub fn basename() -> Result<String> {
        crate::master_names::MasterNames::current_basename()
    }

    /// 使用当前应用的名称前缀和进程内固定的四字符令牌。
    pub fn process_name() -> Result<String> {
        Ok(crate::master_names::process_name(&Self::basename()?))
    }

    pub fn command_queue() -> Result<String> {
        Ok(format!("master:{}", Self::process_name()?))
    }

    pub fn command_queue_for(name: Option<&str>) -> Result<String> {
        match name.filter(|name| !name.is_empty() && *name != "0") {
            Some(name) => Ok(format!("master:{name}")),
            None => Self::command_queue(),
        }
    }

    /// 向本 Master 的监控循环提交重启请求。
    pub async fn restart(&self) -> Result<()> {
        let target = Self::command_queue_for(Some(&self.id))?;
        self.repository
            .push_structured_command(
                &target,
                crate::supervisor_commands::RESTART,
                serde_json::json!([]),
            )
            .await
    }

    /// 注册受管 Supervisor 的 stdout 和 stderr 回调。
    pub fn handle_output_using(
        mut self,
        handler: impl Fn(OutputStream, &[u8]) + Send + Sync + 'static,
    ) -> Self {
        self.output = Some(Arc::new(handler));
        self
    }

    pub(crate) fn with_output_handler(mut self, handler: Option<OutputHandler>) -> Self {
        self.output = handler;
        self
    }

    /// 将输出片段送往已注册的回调。
    pub fn output(&self, stream: OutputStream, bytes: &[u8]) {
        if let Some(handler) = &self.output {
            handler(stream, bytes);
        }
    }

    /// 在当前应用内设置 Master 名称前缀解析器。
    pub fn determine_name_using(
        resolve: impl Fn() -> String + Send + Sync + 'static,
    ) -> Result<()> {
        crate::master_names::MasterNames::determine_using(resolve)
    }

    pub async fn run(self, shutdown: &CancellationToken) -> Result<()> {
        self.run_with_start(shutdown, || {}).await
    }

    pub(crate) async fn run_with_start(
        mut self,
        shutdown: &CancellationToken,
        started: impl FnOnce() + Send,
    ) -> Result<()> {
        let generation = self.repository.generation().await?;
        for (name, mut options) in self.options.clone() {
            options.name = format!("{}:{name}", self.id);
            options.parent_id = std::process::id();
            let mut child = SupervisorProcess::new(options, None);
            child.output = self.output.clone();
            self.supervisors.push(child);
        }
        let mut control = crate::signals::ControlSignals::new()?;
        let paused = self.repository.paused().await?;
        let lease = self
            .repository
            .master_supervisors()
            .claim(&self.id, &self.status(paused, &[]))
            .await?;
        if let Some(deployment) = &self.deployment
            && let Err(error) = lease.deploy(&deployment.commands).await
        {
            let _ = lease.release().await;
            return Err(error);
        }
        if let Some(deployment) = self.deployment.take() {
            deployment.report().await;
        } else if !self.options.is_empty() {
            let event = MasterSupervisorDeployed {
                master: self.id.clone(),
                environment: self.environment.clone(),
                supervisors: self
                    .supervisors
                    .iter()
                    .map(|child| child.name.clone())
                    .collect(),
            };
            if let Err(error) = openrust::support::facades::Event::dispatch(event).await {
                let _ = openrust::support::facades::Log::warning(
                    &format!("Horizon Master 部署监听器失败: {error}"),
                    &Default::default(),
                );
            }
        }
        started();
        let mut next_poll = Instant::now() + Duration::from_secs(1);
        let result = loop {
            tokio::select! {
                _ = shutdown.cancelled() => break Ok(()),
                _ = tokio::time::sleep_until(next_poll) => {},
                paused = control.next() => {
                    if let Err(error) = lease.set_paused(paused).await {
                        break Err(error);
                    }
                    continue;
                }
            }
            match self.tick(&generation, &lease).await {
                Ok(true) => {}
                Ok(false) => break Ok(()),
                Err(error) => break Err(error),
            }
            next_poll = Instant::now() + Duration::from_secs(1);
        };
        let cleanup = {
            let stop = self.stop(&lease);
            tokio::pin!(stop);
            tokio::select! {
                result = &mut stop => result,
                result = lease.keep_alive() => result.and(stop.await),
            }
        };
        let forget = lease.release().await;
        result.and(cleanup).and(forget)
    }
    async fn tick(&mut self, generation: &Option<String>, lease: &MasterLease) -> Result<bool> {
        lease.renew().await?;
        if &self.repository.generation().await? != generation {
            return Ok(false);
        }
        let masters = self.repository.master_supervisors();
        #[cfg(not(unix))]
        if masters.termination_requested(&self.id).await?.is_some() {
            return Ok(false);
        }
        for item in lease.pending_commands().await? {
            let result = self.process_queued_command(item, lease).await;
            if let Err(error) = result {
                if !matches!(error, Error::InvalidRecord | Error::InvalidConfiguration) {
                    return Err(error);
                }
                let _ = openrust::support::facades::Log::warning(
                    &format!("Horizon Master 命令未执行: {error}"),
                    &Default::default(),
                );
            }
        }
        let paused = masters.paused(&self.id).await?;
        for supervisor in &mut self.supervisors {
            if let Some((exit, _)) = supervisor.poll_exit().await? {
                let restarted = std::mem::take(&mut supervisor.restarting);
                supervisor.dead = supervisor.terminal(exit);
                if !(supervisor.dead || restarted && exit == 1) {
                    self.failures = self.failures.saturating_add(1);
                }
                if let Some(previous) = self.repository.supervisor(&supervisor.name).await? {
                    self.failures = self
                        .failures
                        .saturating_add(previous["unexpected_exits"].as_u64().unwrap_or(0));
                }
                lease.forget_supervisor(&supervisor.name).await?;
            }
            if supervisor.dead
                || supervisor.process.is_some()
                || supervisor.exit.is_some()
                || (paused && supervisor.from_command)
                || supervisor.cooling_down()
            {
                continue;
            }
            let arguments = supervisor.options.to_supervisor_command(&self.executable)?;
            lease.forget_supervisor(&supervisor.name).await?;
            lease.prepare_commands(&supervisor.name).await?;
            let directory = if supervisor.from_command {
                supervisor
                    .options
                    .directory
                    .as_deref()
                    .unwrap_or(&self.base)
            } else {
                &self.base
            };
            let child = Factory::new()
                .command(arguments)
                .path(directory)
                .env([("APP_BASE_PATH", self.base.as_os_str().to_owned())])
                .env(lease.ownership().environment())
                .forever();
            #[cfg(windows)]
            let child = child.with_shutdown_pipe();
            match supervisor.start(child) {
                Ok(()) => {}
                Err(error) if supervisor.from_command => {
                    self.failures = self.failures.saturating_add(1);
                    let _ = openrust::support::facades::Log::warning(
                        &format!("Horizon 无法启动排队部署的 Supervisor: {error}"),
                        &Default::default(),
                    );
                }
                Err(_) => return Err(Error::Process),
            }
        }
        let states = self.repository.supervisors().await?;
        let states: Vec<_> = states
            .into_iter()
            .filter(|state| {
                let Some(name) = state["name"].as_str() else {
                    return false;
                };
                if let Some(master) = state["master"].as_str() {
                    master == self.id
                        && crate::master_names::supervisor_suffix(&self.id, name).is_some()
                } else {
                    self.supervisors.iter().any(|child| child.name == name)
                }
            })
            .collect();
        lease.publish(&self.status(paused, &states)).await?;
        let event = MasterSupervisorLooped::new(self, paused);
        if let Err(error) = openrust::support::facades::Event::dispatch(event.clone()).await {
            let _ = openrust::support::facades::Log::warning(
                &format!("Horizon master loop listener failed: {error}"),
                &Default::default(),
            );
        }
        // 自定义监听器可能提前终止派发，进程仍须检查内存。
        crate::MonitorMasterSupervisorMemory::check(&event)
            .await
            .map_err(|_| Error::Unavailable)?;
        if event.memory_check.exceeded() {
            return Err(Error::MemoryLimitExceeded);
        }
        Ok(true)
    }

    async fn process_queued_command(
        &mut self,
        item: crate::QueuedCommand,
        lease: &MasterLease,
    ) -> Result<()> {
        if let Some(handler) = crate::command_handler::resolve::<
            dyn crate::MasterSupervisorCommandHandler,
        >(&item.command)?
        {
            return handler.process(self, item.options).await;
        }
        match item.command.as_str() {
            AddSupervisor::NAME => AddSupervisor.process(self, item.options),
            crate::supervisor_commands::RESTART => self.restart_supervisors(lease).await,
            _ => Err(Error::InvalidRecord),
        }
    }

    async fn restart_supervisors(&mut self, lease: &MasterLease) -> Result<()> {
        let mut children = self
            .supervisors
            .iter_mut()
            .filter(|child| {
                !child.restarting && child.process.as_ref().is_some_and(InvokedProcess::running)
            })
            .collect::<Vec<_>>();
        let names = children
            .iter()
            .map(|child| child.name.as_str())
            .collect::<Vec<_>>();
        lease.restart_supervisors(&names).await?;
        for child in &mut children {
            child.restarting = true;
        }
        Ok(())
    }

    fn status(&self, paused: bool, states: &[serde_json::Value]) -> serde_json::Value {
        let workers: Vec<_> = states
            .iter()
            .flat_map(|state| state["workers"].as_array().into_iter().flatten().cloned())
            .collect();
        let targets: BTreeMap<_, _> = states
            .iter()
            .filter_map(|state| {
                Some((
                    crate::master_names::supervisor_suffix(&self.id, state["name"].as_str()?)?
                        .to_owned(),
                    state["targets"].as_object()?.values().next()?.clone(),
                ))
            })
            .collect();
        serde_json::json!({
            "name": self.id,
            "basename": self.basename,
            "host": self.host,
            "pid": std::process::id(),
            "status": if paused { "paused" } else { "running" },
            "environment": self.environment,
            "supervisors": self.supervisors.iter().filter(|s| !s.dead).map(|s| &s.name).collect::<Vec<_>>(),
            "workers": workers,
            "targets": targets,
            "unexpected_exits": self.failures + states.iter().filter_map(|state| state["unexpected_exits"].as_u64()).sum::<u64>(),
        })
    }
    async fn stop(&mut self, lease: &MasterLease) -> Result<()> {
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
            failed |= supervisor.terminate().await.is_err();
        }
        let wait = !self.fast_termination
            || self
                .repository
                .master_supervisors()
                .termination_wait(Some(&self.id))
                .await?;
        for supervisor in &mut self.supervisors {
            if let Some(process) = supervisor.process.take() {
                if wait {
                    failed |= crate::signals::drain(process, deadline).await.is_err();
                } else {
                    failed |= process.detach().await.is_err();
                }
            }
            failed |= lease.forget_supervisor(&supervisor.name).await.is_err();
        }
        if failed { Err(Error::Process) } else { Ok(()) }
    }
}
