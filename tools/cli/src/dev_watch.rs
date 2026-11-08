//! 开发时先构建候选二进制，再回收旧进程并切换运行文件。

use std::io;
use std::path::{Path, PathBuf};
use std::process::{ExitCode, ExitStatus, Stdio};
use std::time::Duration;

use anyhow::{Context as _, bail};
use openrust::tokio;
use rustix::io::Errno;
use rustix::process::{Pid, Signal, WaitId, WaitIdOptions, kill_process_group, waitid};
use serde_json::Value;
use tempfile::{Builder, TempDir, TempPath};
use tokio::io::{AsyncBufReadExt as _, AsyncReadExt as _, BufReader, Lines};
use tokio::process::{Child, ChildStdout, Command};
use tokio::signal::unix::{SignalKind, signal};
use tokio::task::JoinHandle;
use tokio::time::{Instant, sleep, sleep_until, timeout};

mod files;

use files::{Change, Files};

const SAVE_WINDOW: Duration = Duration::from_millis(250);
const APP_GRACE: Duration = Duration::from_secs(30);
const TOOL_GRACE: Duration = Duration::from_secs(2);
const REDIS_START_TIMEOUT: Duration = Duration::from_secs(20);

pub(super) fn run(
    workspace: &Path,
    app: &Path,
    namespace: &str,
    environment: Option<&str>,
) -> anyhow::Result<ExitCode> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let mut interrupt = signal(SignalKind::interrupt())?;
        let mut terminate = signal(SignalKind::terminate())?;
        let mut metadata_job = Job::start(metadata_command(app), Phase::Metadata)?;
        let metadata = tokio::select! {
            result = metadata_result(&mut metadata_job) => result?,
            _ = interrupt.recv() => {
                metadata_job.process.stop(TOOL_GRACE).await?;
                return Ok(ExitCode::SUCCESS);
            }
            _ = terminate.recv() => {
                metadata_job.process.stop(TOOL_GRACE).await?;
                return Ok(ExitCode::SUCCESS);
            }
        };
        let project = Project {
            workspace,
            app,
            namespace,
            environment,
        };
        let mut supervisor = Supervisor::new(project, &metadata)?;
        let result = tokio::select! {
            result = supervisor.watch() => result,
            _ = interrupt.recv() => Ok(()),
            _ = terminate.recv() => Ok(()),
        };
        supervisor.close().await?;
        result?;
        Ok(ExitCode::SUCCESS)
    })
}

struct Project<'a> {
    workspace: &'a Path,
    app: &'a Path,
    namespace: &'a str,
    environment: Option<&'a str>,
}

impl Project<'_> {
    fn target(&self, metadata: &Value) -> anyhow::Result<Target> {
        let manifest = self.app.join("Cargo.toml");
        let package = metadata["packages"]
            .as_array()
            .context("Cargo metadata 缺少包清单")?
            .iter()
            .find(|package| package["manifest_path"].as_str().map(Path::new) == Some(&manifest))
            .context("Cargo metadata 找不到当前应用")?;
        let name = package["name"]
            .as_str()
            .context("Cargo metadata 缺少应用包名")?;
        let binaries = package["targets"]
            .as_array()
            .context("Cargo metadata 缺少编译目标")?
            .iter()
            .filter(|target| {
                target["kind"]
                    .as_array()
                    .is_some_and(|kinds| kinds.iter().any(|kind| kind == "bin"))
            })
            .filter_map(|target| target["name"].as_str())
            .collect::<Vec<_>>();
        let binary = match (package["default_run"].as_str(), binaries.as_slice()) {
            (Some(binary), _) if binaries.contains(&binary) => binary,
            (None, [binary]) => binary,
            _ => bail!("开发重载需要唯一应用二进制或 Cargo default-run"),
        };
        Ok(Target {
            package: name.to_owned(),
            binary: binary.to_owned(),
        })
    }

    fn files(&self, metadata: &Value) -> anyhow::Result<Files> {
        Files::new(
            metadata,
            self.app,
            self.workspace,
            self.namespace,
            self.environment,
        )
    }
}

struct Target {
    package: String,
    binary: String,
}

struct Supervisor<'a> {
    project: Project<'a>,
    files: Files,
    watcher: Watcher,
    artifacts: TempDir,
    target: Target,
    deadline: Option<Instant>,
    manifest_changed: bool,
    job: Option<Job>,
    redis_setup: Option<ChildGroup>,
    running: Option<Running>,
}

impl<'a> Supervisor<'a> {
    fn new(project: Project<'a>, metadata: &Value) -> anyhow::Result<Self> {
        let application_target = project.target(metadata)?;
        let mut files = project.files(metadata)?;
        let target = metadata["target_directory"]
            .as_str()
            .map(Path::new)
            .context("Cargo metadata 缺少构建目录")?;
        std::fs::create_dir_all(target)?;
        let artifacts = Builder::new().prefix("openrust-dev-").tempdir_in(target)?;
        files.exclude(artifacts.path());
        let watcher = Watcher::start(project.app, &files)?;
        Ok(Self {
            project,
            files,
            watcher,
            artifacts,
            target: application_target,
            deadline: Some(Instant::now()),
            manifest_changed: false,
            job: None,
            redis_setup: None,
            running: None,
        })
    }

    async fn watch(&mut self) -> anyhow::Result<()> {
        loop {
            tokio::select! {
                line = self.watcher.lines.next_line() => {
                    let line = line?.context("Watchexec 事件流已结束")?;
                    match self.files.classify(&line)? {
                        Change::None => {}
                        Change::Source => self.defer_build(),
                        Change::Manifest => {
                            self.manifest_changed = true;
                            self.defer_build();
                        }
                    }
                }
                status = wait_optional(self.job.as_mut().map(|job| &mut job.process)) => {
                    self.finish_job(status?).await?;
                }
                status = wait_optional(self.running.as_mut().map(|app| &mut app.process)) => {
                    eprintln!("开发应用已退出（{}），等待文件变更", status?);
                    self.running = None;
                }
                () = wait_deadline(self.deadline), if self.job.is_none() => {
                    self.deadline = None;
                    self.job = Some(if std::mem::take(&mut self.manifest_changed) {
                        Job::start(metadata_command(self.project.app), Phase::Metadata)?
                    } else {
                        self.start_build()?
                    });
                }
            }
        }
    }

    fn defer_build(&mut self) {
        self.deadline = Some(Instant::now() + SAVE_WINDOW);
    }

    fn start_build(&self) -> anyhow::Result<Job> {
        eprintln!("开发重载正在构建 {}", self.target.package);
        let mut command = Command::new("cargo");
        command.current_dir(self.project.app).args([
            "build",
            "--package",
            &self.target.package,
            "--bin",
            &self.target.binary,
            "--message-format=json-render-diagnostics",
        ]);
        Job::start(command, Phase::Build)
    }

    async fn finish_job(&mut self, status: ExitStatus) -> anyhow::Result<()> {
        let job = self.job.take().context("缺少已完成的开发构建任务")?;
        let output = job.output.await??;
        if !status.success() {
            eprintln!("开发构建或 Cargo 清单解析失败（{status}），等待下次保存");
            return Ok(());
        }
        match job.phase {
            Phase::Metadata => {
                if let Err(error) = self.refresh_metadata(&output).await {
                    eprintln!("开发项目配置无法更新：{error:#}");
                } else if self.deadline.is_none() {
                    self.job = Some(self.start_build()?);
                }
            }
            Phase::Build if self.deadline.is_none() => {
                if let Err(error) = self.install(&output).await {
                    eprintln!("开发候选应用无法启动：{error:#}");
                }
            }
            Phase::Build => {}
        }
        Ok(())
    }

    async fn refresh_metadata(&mut self, output: &[u8]) -> anyhow::Result<()> {
        let metadata = serde_json::from_slice(output).context("Cargo metadata 格式无效")?;
        let target = self.project.target(&metadata)?;
        let mut files = self.project.files(&metadata)?;
        files.exclude(self.artifacts.path());
        if files.watch_args() != self.files.watch_args() {
            let watcher = Watcher::start(self.project.app, &files)?;
            self.watcher.process.stop(TOOL_GRACE).await?;
            self.watcher = watcher;
        }
        self.files = files;
        self.target = target;
        Ok(())
    }

    async fn install(&mut self, output: &[u8]) -> anyhow::Result<()> {
        let manifest = self.project.app.join("Cargo.toml");
        let executable = build_executable(output, &manifest, &self.target.binary)?;
        let artifact = Builder::new()
            .prefix("app-")
            .tempfile_in(self.artifacts.path())?
            .into_temp_path();
        std::fs::copy(executable, &artifact).context("复制开发运行产物")?;
        self.prepare_redis().await?;
        if let Some(previous) = self.running.as_mut() {
            previous.process.stop(APP_GRACE).await?;
            self.running = None;
        }
        let mut command = Command::new(artifact.as_os_str());
        command
            .current_dir(self.project.app)
            .env_remove("OPENRUST_ARTISAN");
        if let Some(environment) = self.project.environment {
            command.args(["--env", environment]);
        }
        let process = ChildGroup::spawn(&mut command).context("启动开发应用")?;
        eprintln!("开发应用已启动（PID {}）", process.group.as_raw_pid());
        self.running = Some(Running {
            process,
            _artifact: artifact,
        });
        Ok(())
    }

    async fn prepare_redis(&mut self) -> anyhow::Result<()> {
        let Some((host, port)) = super::development_redis_endpoint(
            self.project.app,
            self.project.namespace,
            self.project.environment,
        )?
        else {
            return Ok(());
        };
        if redis_ready(&host, port).await? {
            return Ok(());
        }
        let mut command = Command::from(super::development_redis_compose(
            self.project.workspace,
            port,
        )?);
        let process = self.redis_setup.insert(ChildGroup::spawn(&mut command)?);
        let Ok(status) = timeout(REDIS_START_TIMEOUT, process.wait()).await else {
            process.stop(TOOL_GRACE).await?;
            self.redis_setup = None;
            bail!("Docker Compose 启动开发 Redis 超时");
        };
        let status = status?;
        self.redis_setup = None;
        if !status.success() {
            bail!("Docker Compose 无法启动开发 Redis（{status}）");
        }
        timeout(Duration::from_secs(10), async {
            while !redis_ready(&host, port).await? {
                sleep(Duration::from_millis(100)).await;
            }
            Ok(())
        })
        .await
        .context("开发 Redis 在 10 秒内未就绪")?
    }

    async fn close(&mut self) -> anyhow::Result<()> {
        let (watcher, job, redis, app) = tokio::join!(
            self.watcher.process.stop(TOOL_GRACE),
            stop_optional(self.job.as_mut().map(|job| &mut job.process), TOOL_GRACE),
            stop_optional(self.redis_setup.as_mut(), TOOL_GRACE),
            stop_optional(self.running.as_mut().map(|app| &mut app.process), APP_GRACE)
        );
        watcher?;
        job?;
        redis?;
        app
    }
}

struct Running {
    process: ChildGroup,
    _artifact: TempPath,
}

struct Watcher {
    process: ChildGroup,
    lines: Lines<BufReader<ChildStdout>>,
}

impl Watcher {
    fn start(app: &Path, files: &Files) -> anyhow::Result<Self> {
        let mut command = Command::new("watchexec");
        command
            .current_dir(app)
            .args(files.watch_args())
            .stdin(Stdio::null())
            .stdout(Stdio::piped());
        let mut process = ChildGroup::spawn(&mut command)
            .context("无法启动 Watchexec，请在 PATH 提供支持 --only-emit-events 的版本")?;
        let stdout = process
            .child
            .stdout
            .take()
            .context("缺少 Watchexec 事件管道")?;
        Ok(Self {
            process,
            lines: BufReader::new(stdout).lines(),
        })
    }
}

#[derive(Clone, Copy)]
enum Phase {
    Metadata,
    Build,
}

struct Job {
    process: ChildGroup,
    output: JoinHandle<io::Result<Vec<u8>>>,
    phase: Phase,
}

impl Job {
    fn start(mut command: Command, phase: Phase) -> anyhow::Result<Self> {
        command.stdin(Stdio::null()).stdout(Stdio::piped());
        let mut process = ChildGroup::spawn(&mut command).context("启动 Cargo 开发构建")?;
        let mut stdout = process.child.stdout.take().context("缺少 Cargo 输出管道")?;
        let output = tokio::spawn(async move {
            let mut bytes = Vec::new();
            stdout.read_to_end(&mut bytes).await?;
            Ok(bytes)
        });
        Ok(Self {
            process,
            output,
            phase,
        })
    }
}

fn metadata_command(app: &Path) -> Command {
    let mut command = Command::new("cargo");
    command
        .current_dir(app)
        .args(["metadata", "--format-version=1"]);
    command
}

async fn metadata_result(job: &mut Job) -> anyhow::Result<Value> {
    let status = job.process.wait().await?;
    if !status.success() {
        bail!("Cargo metadata 失败（{status}）");
    }
    serde_json::from_slice(&(&mut job.output).await??).context("Cargo metadata 格式无效")
}

fn build_executable(output: &[u8], manifest: &Path, binary: &str) -> anyhow::Result<PathBuf> {
    String::from_utf8_lossy(output)
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find(|message| {
            message["reason"] == "compiler-artifact"
                && message["target"]["name"] == binary
                && message["manifest_path"].as_str().map(Path::new) == Some(manifest)
                && message["executable"].is_string()
        })
        .and_then(|message| message["executable"].as_str().map(PathBuf::from))
        .context("Cargo 未返回当前应用的可执行产物")
}

struct ChildGroup {
    child: Child,
    group: Pid,
    exited: tokio::signal::unix::Signal,
}

impl ChildGroup {
    fn spawn(command: &mut Command) -> anyhow::Result<Self> {
        let exited = signal(SignalKind::child())?;
        let child = command.process_group(0).kill_on_drop(true).spawn()?;
        let id = child.id().context("开发子进程没有 PID")?;
        let group = Pid::from_raw(i32::try_from(id)?).context("开发子进程 PID 无效")?;
        Ok(Self {
            child,
            group,
            exited,
        })
    }

    async fn wait(&mut self) -> io::Result<ExitStatus> {
        if self.child.id().is_some() {
            // 保留已退出的组首进程，避免在回收同组后代前复用其进程组 ID。
            while !self.leader_exited()? {
                if self.exited.recv().await.is_none() {
                    return Err(io::Error::other("开发子进程退出通知已关闭"));
                }
            }
            self.signal(Signal::KILL)?;
        }
        self.child.wait().await
    }

    fn leader_exited(&self) -> io::Result<bool> {
        let flags = WaitIdOptions::EXITED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT;
        Ok(waitid(WaitId::Pid(self.group), flags)?.is_some())
    }

    async fn stop(&mut self, grace: Duration) -> anyhow::Result<()> {
        if self.child.id().is_some() {
            self.signal(Signal::TERM)?;
            if let Ok(status) = timeout(grace, self.wait()).await {
                status?;
            } else {
                self.signal(Signal::KILL)?;
                self.child.wait().await?;
            }
        }
        Ok(())
    }

    fn signal(&self, signal: Signal) -> io::Result<()> {
        match kill_process_group(self.group, signal) {
            Ok(()) | Err(Errno::SRCH) => Ok(()),
            // Darwin 在组中只剩尚未收取的退出进程时可能返回 EPERM。
            #[cfg(target_os = "macos")]
            Err(Errno::PERM) if self.leader_exited()? => Ok(()),
            Err(error) => Err(error.into()),
        }
    }
}

async fn wait_optional(process: Option<&mut ChildGroup>) -> io::Result<ExitStatus> {
    match process {
        Some(process) => process.wait().await,
        None => std::future::pending().await,
    }
}

async fn redis_ready(host: &str, port: u16) -> anyhow::Result<bool> {
    let host = host.to_owned();
    Ok(tokio::task::spawn_blocking(move || super::redis_ping(&host, port)).await?)
}

async fn stop_optional(process: Option<&mut ChildGroup>, grace: Duration) -> anyhow::Result<()> {
    if let Some(process) = process {
        process.stop(grace).await?;
    }
    Ok(())
}

async fn wait_deadline(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => sleep_until(deadline).await,
        None => std::future::pending().await,
    }
}

#[cfg(test)]
#[path = "../tests/unit/dev_watch.rs"]
mod tests;
