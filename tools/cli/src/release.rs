//! 构建当前应用并发布可搬迁的应用目录。

use std::fs::{self, File, OpenOptions};
use std::io::{Read as _, Write as _};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

use anyhow::{Context as _, bail};
use clap::{Arg, ArgAction, Args as _, FromArgMatches as _, error::ErrorKind};
use openrust::config::{ConfigLoader, ENV_PREFIX};
use openrust::console::Command as ConsoleCommand;
use openrust::foundation::BootContext;
use openrust::support::{BoxFuture, ServiceProvider};
use openrust::{Application, Error, Result};
use serde_json::Value;

const INCOMPLETE: &str = ".openrust-build-incomplete";
const PLAN_LIMIT: u64 = 8 * 1024 * 1024;
const PACKAGE_ENTRIES: &[&str] = &[
    "artisan",
    ".env.example",
    "public",
    "resources",
    "lang",
    "modules",
    "storage",
    "bootstrap",
];

/// 独立 CLI 与源码 Artisan 共用的构建参数。
#[derive(Debug, clap::Args)]
pub struct BuildArgs {
    #[arg(long, hide = true)]
    project_dir: Option<PathBuf>,
    /// 使用 Compose 和 Docker Buildx 构建 Linux 产物。
    #[arg(long)]
    docker: bool,
    /// Docker 构建的单一 Linux 平台，默认使用宿主 CPU。
    #[arg(long, requires = "docker", value_parser = ["linux/amd64", "linux/arm64"])]
    platform: Option<String>,
    /// 本机构建额外启用的 Cargo features，以逗号分隔。
    #[arg(long, conflicts_with = "docker")]
    features: Option<String>,
    /// 全新产物目录；显式路径的父目录须已存在。
    #[arg(long)]
    output: Option<PathBuf>,
}

impl BuildArgs {
    /// 使用当前应用的默认构建环境导出发布包。
    pub fn run(self) -> anyhow::Result<ExitCode> {
        self.run_for_environment(None)
    }

    /// 构建发布包，Docker 构建按应用的 `APP_` 命名空间选择 Compose 插值环境。
    ///
    /// 此入口不启动应用，也不求值编译期配置。
    pub fn run_for_environment(self, environment: Option<&str>) -> anyhow::Result<ExitCode> {
        self.build(|project| {
            Ok(ConfigLoader::application_dotenv_path(
                project,
                ENV_PREFIX,
                environment,
            )?)
        })
    }

    fn build(
        self,
        select_dotenv: impl FnOnce(&Path) -> anyhow::Result<PathBuf>,
    ) -> anyhow::Result<ExitCode> {
        let project = self.project_dir.map_or_else(std::env::current_dir, Ok)?;
        let project = project.canonicalize().context("build_project_invalid")?;
        path_text(&project)?;
        let platform = match self.platform.as_deref() {
            Some(platform) => platform,
            None => host_platform(
                if self.docker {
                    "linux"
                } else {
                    std::env::consts::OS
                },
                std::env::consts::ARCH,
            )?,
        };
        let manifest = read_manifest(&project)?;
        let name = application_name(&manifest)?;
        let destination = destination(&project, name, platform, self.output.as_deref())?;
        reject_existing(&destination)?;
        let mut staging = tempfile::Builder::new();
        staging.prefix("openrust-release-");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            staging.permissions(fs::Permissions::from_mode(0o700));
        }
        let temporary = staging.tempdir().context("build_staging_create_failed")?;
        let result = (|| {
            let staging = temporary.path().canonicalize()?;
            if staging.starts_with(&project) {
                bail!("build_staging_in_context: 临时目录须在源码目录外");
            }
            let artifacts = staging.join("artifacts");
            if self.docker {
                let dotenv = select_dotenv(&project)?;
                build_docker(&project, &manifest, platform, &staging, &artifacts, &dotenv)?;
            } else {
                build_native(
                    &project,
                    name,
                    platform,
                    &staging,
                    &artifacts,
                    self.features.as_deref(),
                )?;
            }
            validate_release(&artifacts, name, platform)?;
            publish(&artifacts, &destination, name)
        })();
        let cleanup = temporary.close();
        if cleanup.is_err() {
            eprintln!("build_staging_cleanup_failed: 自有临时目录清理失败");
        }
        result?;
        writeln!(std::io::stdout().lock(), "{}", destination.display())?;
        Ok(ExitCode::SUCCESS)
    }
}

/// 用 Cargo 或显式选用的 Docker 构建应用的源码命令。
pub struct BuildCommand;

impl BuildCommand {
    /// Artisan 命令名称。
    pub const NAME: &str = "build";
    /// 命令列表与帮助中的说明。
    pub const ABOUT: &str = "构建应用发布目录，默认使用本机 Cargo。";

    /// 在指定源码目录运行命令，无须启动应用或连接运行时服务。
    ///
    /// 帮助和参数错误按 Clap 规则输出，返回对应退出码。
    pub fn run_source(
        project: &Path,
        args: &[String],
        environment: Option<&str>,
    ) -> anyhow::Result<ExitCode> {
        match Self::parse_source(args) {
            Ok(mut options) => {
                options.project_dir = Some(project.to_path_buf());
                options.run_for_environment(environment)
            }
            Err(error) => {
                error.print()?;
                Ok(ExitCode::from(u8::try_from(error.exit_code())?))
            }
        }
    }

    fn source_clap() -> clap::Command {
        BuildArgs::augment_args(
            clap::Command::new(Self::NAME)
                .bin_name("artisan build")
                .about(Self::ABOUT)
                .help_template("{about-with-newline}用法：{usage}\n\n选项：\n{options}\n")
                .disable_help_flag(true)
                .arg(
                    Arg::new("help")
                        .short('h')
                        .long("help")
                        .action(ArgAction::HelpLong)
                        .help("显示帮助信息并退出"),
                ),
        )
    }

    fn parse_source(args: &[String]) -> clap::error::Result<BuildArgs> {
        let mut command = Self::source_clap();
        let matches = command.try_get_matches_from_mut(
            std::iter::once(Self::NAME).chain(args.iter().map(String::as_str)),
        );
        let matches = match matches {
            Err(error) if error.kind() == ErrorKind::DisplayHelp && args.len() != 1 => {
                return Err(command.error(ErrorKind::ArgumentConflict, "帮助选项须单独使用"));
            }
            result => result?,
        };
        let options = BuildArgs::from_arg_matches(&matches)?;
        if options.project_dir.is_some() {
            return Err(command.error(
                ErrorKind::UnknownArgument,
                "源码构建入口不接受 --project-dir",
            ));
        }
        Ok(options)
    }
}

impl ConsoleCommand for BuildCommand {
    fn name(&self) -> &'static str {
        Self::NAME
    }

    fn about(&self) -> &'static str {
        Self::ABOUT
    }

    fn accepts_args(&self) -> bool {
        true
    }

    fn render_help(&self) -> Option<String> {
        Self::parse_source(&["--help".to_owned()])
            .err()
            .map(|help| help.to_string())
    }

    fn run<'a>(&'a self, app: &'a Application, args: Vec<String>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let options = match Self::parse_source(&args) {
                Ok(options) => options,
                Err(error) if error.kind() == ErrorKind::DisplayHelp => {
                    return error
                        .print()
                        .map_err(|_| Error::custom("build_help_failed", "无法输出构建帮助"));
                }
                Err(error) => {
                    return Err(Error::custom("invalid_build_arguments", error.to_string()));
                }
            };
            let dotenv = app.dotenv_path().map(Path::to_path_buf);
            openrust::tokio::task::spawn_blocking(move || {
                options.build(|project| Ok(dotenv.unwrap_or_else(|| project.join(".env"))))
            })
            .await
            .map_err(|_| Error::custom("build_task_failed", "构建任务异常结束"))?
            .map_err(|error| Error::custom("build_failed", error.to_string()))?;
            Ok(())
        })
    }
}

/// 在源码应用的同一 Artisan 注册表中登记构建命令。
pub struct BuildServiceProvider;

impl ServiceProvider for BuildServiceProvider {
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            if is_source_project(Path::new(".")) {
                app.reserve_command(BuildCommand);
            }
            Ok(())
        })
    }
}

fn is_source_project(project: &Path) -> bool {
    project.join("Cargo.toml").is_file() && project.join("src/main.rs").is_file()
}

fn host_platform(os: &str, architecture: &str) -> anyhow::Result<&'static str> {
    match (os, architecture) {
        ("linux", "x86_64") => Ok("linux/amd64"),
        ("linux", "aarch64") => Ok("linux/arm64"),
        ("macos", "x86_64") => Ok("darwin/amd64"),
        ("macos", "aarch64") => Ok("darwin/arm64"),
        _ => bail!(
            "build_platform_required: 当前宿主不支持本机构建，可用 --docker --platform 显式选择 Linux 平台"
        ),
    }
}

fn read_manifest(project: &Path) -> anyhow::Result<toml_edit::DocumentMut> {
    let source =
        fs::read_to_string(project.join("Cargo.toml")).context("build_manifest_unreadable")?;
    source
        .parse::<toml_edit::DocumentMut>()
        .map_err(|_| anyhow::anyhow!("build_manifest_invalid"))
}

fn application_name(document: &toml_edit::DocumentMut) -> anyhow::Result<&str> {
    let name = document
        .get("package")
        .and_then(|package| package.get("name"))
        .and_then(toml_edit::Item::as_str)
        .context("build_package_name_missing")?;
    if name.is_empty()
        || !name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
    {
        bail!("build_package_name_invalid");
    }
    if PACKAGE_ENTRIES
        .iter()
        .any(|entry| name.eq_ignore_ascii_case(entry))
    {
        bail!("build_package_name_conflict: 项目名与发布目录的保留名称冲突");
    }
    Ok(name)
}

fn destination(
    project: &Path,
    name: &str,
    platform: &str,
    output: Option<&Path>,
) -> anyhow::Result<PathBuf> {
    let requested = if let Some(output) = output {
        project.join(output)
    } else {
        let dist = project.join("dist");
        match fs::create_dir(&dist) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                if !fs::symlink_metadata(&dist)?.file_type().is_dir() {
                    bail!("build_dist_invalid");
                }
            }
            Err(error) => return Err(error).context("build_dist_create_failed"),
        }
        dist.join(format!("{name}-{}", platform.replace('/', "-")))
    };
    if requested
        .components()
        .any(|part| matches!(part, Component::ParentDir))
    {
        bail!("build_output_parent_traversal: 输出路径不得含 ..");
    }
    let name = requested.file_name().context("build_output_name_missing")?;
    let parent = requested
        .parent()
        .context("build_output_parent_missing")?
        .canonicalize()
        .context("build_output_parent_missing")?;
    let result = parent.join(name);
    path_text(&result)?;
    Ok(result)
}

fn reject_existing(path: &Path) -> anyhow::Result<()> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Ok(_) => bail!("build_output_exists: 目标已存在，拒绝覆盖"),
        Err(error) => Err(error).context("build_output_unreadable"),
    }
}

fn path_text(path: &Path) -> anyhow::Result<&str> {
    let text = path.to_str().context("build_path_not_utf8")?;
    if text.chars().any(char::is_control) || text.contains(['*', '?', '[', ']']) {
        bail!("build_path_invalid: 路径不得含控制字符或授权通配符");
    }
    Ok(text)
}

fn docker(project: &Path) -> Command {
    let mut command = Command::new("docker");
    command
        .current_dir(project)
        .args(["buildx", "bake"])
        .env("BUILDX_BAKE_ENTITLEMENTS_FS", "1")
        .stdin(Stdio::null())
        .stderr(Stdio::null());
    command
}

fn bake_plan_command(project: &Path, temporary: &Path, dotenv: &Path) -> anyhow::Result<Command> {
    let working_directory = if dotenv == project.join(".env") {
        project
    } else {
        // Bake 从 cwd 读取 .env，绝对 Compose 路径保留原应用的构建路径基准。
        copy_dotenv(dotenv, &temporary.join(".env"))
            .map_err(|_| anyhow::anyhow!("build_dotenv_snapshot_failed: 无法复制构建环境文件"))?;
        temporary
    };
    let mut command = docker(working_directory);
    command
        .arg("-f")
        .arg(project.join("docker-compose.yml"))
        .args(["--print", "app"]);
    Ok(command)
}

fn copy_dotenv(source: &Path, snapshot: &Path) -> std::io::Result<()> {
    let mut input = File::open(source)?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut output = options.open(snapshot)?;
    std::io::copy(&mut input, &mut output)?;
    output.flush()
}

fn capture_plan(project: &Path, temporary: &Path, dotenv: &Path) -> anyhow::Result<Value> {
    let mut command = bake_plan_command(project, temporary, dotenv)?;
    let capture = temporary.join("capture.json");
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&capture)?;
    command.stdout(file);
    run_docker(&mut command, "build_plan_failed")?;
    let mut bytes = Vec::new();
    File::open(capture)?
        .take(PLAN_LIMIT + 1)
        .read_to_end(&mut bytes)?;
    if u64::try_from(bytes.len())? > PLAN_LIMIT {
        bail!("build_plan_too_large");
    }
    serde_json::from_slice(&bytes).map_err(|_| anyhow::anyhow!("build_plan_invalid"))
}

fn local_context(project: &Path, value: &str) -> anyhow::Result<PathBuf> {
    let result = project
        .join(value)
        .canonicalize()
        .context("build_context_invalid")?;
    if !result.is_dir() {
        bail!("build_context_not_directory");
    }
    path_text(&result)?;
    Ok(result)
}

fn freeze_plan(
    project: &Path,
    manifest: &toml_edit::DocumentMut,
    plan: &Value,
    platform: &str,
    artifacts: &Path,
) -> anyhow::Result<(Value, Option<PathBuf>)> {
    let mut target = plan
        .get("target")
        .and_then(|targets| targets.get("app"))
        .and_then(Value::as_object)
        .cloned()
        .context("build_app_target_missing")?;
    let context = target.get("context").and_then(Value::as_str).unwrap_or(".");
    if local_context(project, context)? != project {
        bail!("build_app_context_mismatch: 当前有限入口仅支持应用根 context");
    }
    // 只接收已知字段。Buildx 新增的凭据、远端导出和特权配置须单独接入。
    let supported = [
        "context",
        "contexts",
        "dockerfile",
        "args",
        "labels",
        "no-cache",
        "no-cache-filter",
        "pull",
        "target",
        "platforms",
        "output",
        "tags",
    ];
    if target.keys().any(|key| !supported.contains(&key.as_str())) {
        bail!("build_custom_target_unsupported: 自定义构建字段需单独配置验收");
    }
    let dockerfile = target
        .get("dockerfile")
        .and_then(Value::as_str)
        .unwrap_or("Dockerfile");
    let dockerfile = project
        .join(dockerfile)
        .canonicalize()
        .context("build_dockerfile_invalid")?;
    if !dockerfile.starts_with(project) || !dockerfile.is_file() {
        bail!("build_dockerfile_outside_project");
    }
    target.insert("context".into(), bake_path(project)?);
    target.insert("dockerfile".into(), bake_path(&dockerfile)?);
    let framework = if let Some(contexts) = target.get_mut("contexts") {
        let contexts = contexts.as_object_mut().context("build_contexts_invalid")?;
        if contexts.keys().any(|key| key != "openrust-source") {
            bail!("build_named_context_unsupported");
        }
        if let Some(value) = contexts.get_mut("openrust-source") {
            let path = local_context(project, value.as_str().context("build_context_invalid")?)?;
            validate_framework_context(project, manifest, &path)?;
            *value = bake_path(&path)?;
            Some(path)
        } else {
            None
        }
    } else {
        None
    };
    target.remove("tags");
    target.insert("target".into(), Value::from("artifacts"));
    target.insert("platforms".into(), serde_json::json!([platform]));
    target.insert(
        "output".into(),
        serde_json::json!([{"type": "local", "dest": bake_path(artifacts)?}]),
    );
    Ok((serde_json::json!({"target": {"app": target}}), framework))
}

fn bake_path(path: &Path) -> anyhow::Result<Value> {
    // Bake 的 JSON 重放按 HCL 求值。仅转义此处写入的路径，保留 Buildx 已编码的参数。
    Ok(Value::String(
        path_text(path)?.replace("${", "$${").replace("%{", "%%{"),
    ))
}

fn validate_framework_context(
    project: &Path,
    manifest: &toml_edit::DocumentMut,
    framework: &Path,
) -> anyhow::Result<()> {
    let dependency = manifest
        .get("workspace")
        .and_then(|workspace| workspace.get("dependencies"))
        .and_then(|dependencies| dependencies.get("openrust"))
        .and_then(|dependency| dependency.get("path"))
        .and_then(toml_edit::Item::as_str)
        .context("build_framework_dependency_missing")?;
    let dependency = local_context(project, dependency)?;
    let expected = framework.join("vendor/openrust/framework");
    if framework.parent().is_none()
        || dependency != expected
        || !expected.join("Cargo.toml").is_file()
        || !framework.join("Cargo.toml").is_file()
    {
        bail!("build_framework_context_mismatch: Compose 框架目录须与 Cargo 依赖一致");
    }
    Ok(())
}

fn run_docker(command: &mut Command, code: &'static str) -> anyhow::Result<()> {
    let mut child = command
        .spawn()
        .map_err(|_| anyhow::anyhow!("{code}: 无法启动 Docker Buildx"))?;
    let Ok(status) = child.wait() else {
        let _ = child.kill();
        let _ = child.wait();
        bail!("{code}: 等待 Docker Buildx 失败");
    };
    if !status.success() {
        bail!(
            "{code}: Docker Buildx 失败，退出码 {:?}；未输出构建配置或子进程日志",
            status.code()
        );
    }
    Ok(())
}

fn build_native(
    project: &Path,
    name: &str,
    platform: &str,
    staging: &Path,
    artifacts: &Path,
    features: Option<&str>,
) -> anyhow::Result<()> {
    let asset_status = Command::new("sh")
        .current_dir(project)
        .args([
            "-c",
            include_str!("../templates/deploy/package-assets.sh"),
            "package-assets",
        ])
        .arg(artifacts)
        .arg(name)
        .status()
        .context("build_assets_start_failed")?;
    if !asset_status.success() {
        bail!("build_assets_failed");
    }

    let manifest = project.join("Cargo.toml");
    let cargo = native_cargo_target(project, &manifest)?;
    // Cargo 按应用配置复用中间缓存，最终程序写入本次私有目录，避免其他构建替换。
    let target = staging.join("cargo-target");
    let mut command = Command::new("cargo");
    command
        .current_dir(project)
        .env("CARGO_BUILD_BUILD_DIR", cargo.build_dir)
        .arg("build")
        .arg("--target-dir")
        .arg(&target)
        .args([
            "--release",
            "--package",
            name,
            "--bin",
            &cargo.binary,
            "--target",
            "host-tuple",
            "--message-format=json-render-diagnostics",
        ])
        .stderr(Stdio::inherit());
    if cargo.locked {
        command.arg("--locked");
    }
    if let Some(features) = features {
        command.args(["--features", features]);
    }
    let output = command.output().context("build_cargo_start_failed")?;
    if !output.status.success() {
        bail!("build_cargo_failed: {}", output.status);
    }
    let executable = cargo_executable(&output.stdout, &manifest, &cargo.binary)?;
    validate_binary(&executable, platform)?;
    if !executable.canonicalize()?.starts_with(&target) {
        bail!("build_cargo_artifact_outside_staging");
    }
    copy_entry(&executable, &artifacts.join(name))
}

struct NativeCargoTarget {
    binary: String,
    build_dir: PathBuf,
    locked: bool,
}

fn native_cargo_target(project: &Path, manifest: &Path) -> anyhow::Result<NativeCargoTarget> {
    let output = Command::new("cargo")
        .current_dir(project)
        .args(["metadata", "--no-deps", "--format-version=1"])
        .stderr(Stdio::inherit())
        .output()
        .context("build_cargo_metadata_start_failed")?;
    if !output.status.success() {
        bail!("build_cargo_metadata_failed: {}", output.status);
    }
    let metadata: Value =
        serde_json::from_slice(&output.stdout).context("build_cargo_metadata_invalid")?;
    let build_dir = metadata["build_directory"]
        .as_str()
        .context("build_cargo_build_dir_missing")?;
    let workspace = metadata["workspace_root"]
        .as_str()
        .context("build_cargo_workspace_missing")?;
    let package = metadata["packages"]
        .as_array()
        .and_then(|packages| {
            packages
                .iter()
                .find(|p| p["manifest_path"].as_str().map(Path::new) == Some(manifest))
        })
        .context("build_cargo_package_missing")?;
    let binaries = package["targets"]
        .as_array()
        .context("build_cargo_targets_missing")?
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
        _ => bail!("build_cargo_binary_ambiguous: 应用须有唯一二进制或指定 Cargo default-run"),
    };
    Ok(NativeCargoTarget {
        binary: binary.to_owned(),
        build_dir: PathBuf::from(build_dir),
        locked: Path::new(workspace).join("Cargo.lock").is_file(),
    })
}

fn cargo_executable(output: &[u8], manifest: &Path, binary: &str) -> anyhow::Result<PathBuf> {
    let output = String::from_utf8_lossy(output);
    let mut executables = output
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|message| {
            message["reason"] == "compiler-artifact"
                && message["target"]["name"] == binary
                && message["manifest_path"].as_str().map(Path::new) == Some(manifest)
        })
        .filter_map(|message| message["executable"].as_str().map(PathBuf::from));
    let executable = executables
        .next()
        .context("build_cargo_artifact_missing: Cargo 未返回当前应用的可执行文件")?;
    if executables.next().is_some() {
        bail!("build_cargo_artifact_ambiguous: Cargo 返回了多个应用产物");
    }
    Ok(executable)
}

fn build_docker(
    project: &Path,
    manifest: &toml_edit::DocumentMut,
    platform: &str,
    temporary: &Path,
    artifacts: &Path,
    dotenv: &Path,
) -> anyhow::Result<()> {
    let plan = capture_plan(project, temporary, dotenv)?;
    let (plan, framework) = freeze_plan(project, manifest, &plan, platform, artifacts)?;
    if framework
        .as_ref()
        .is_some_and(|path| temporary.starts_with(path))
    {
        bail!("build_staging_in_context: 临时目录须在构建上下文外");
    }
    fs::create_dir(artifacts)?;
    let snapshot = temporary.join("build.json");
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&snapshot)?;
    serde_json::to_writer(&mut file, &plan).context("build_plan_snapshot_failed")?;
    file.flush()?;
    drop(file);
    let mut command = docker(project);
    command
        .arg("-f")
        .arg(&snapshot)
        .arg("--allow")
        .arg(format!("fs.read={}", path_text(project)?))
        .arg("--allow")
        .arg(format!("fs.write={}", path_text(artifacts)?));
    if let Some(framework) = framework {
        command
            .arg("--allow")
            .arg(format!("fs.read={}", path_text(&framework)?));
    }
    command
        .args(["--progress=quiet", "app"])
        .stdout(Stdio::null());
    run_docker(&mut command, "build_artifacts_failed")
}

fn validate_release(root: &Path, name: &str, platform: &str) -> anyhow::Result<()> {
    if validate_entries(root, Some(name))? != 1 {
        bail!("build_binary_count_invalid: 产物必须只有一个应用二进制");
    }
    validate_binary(&root.join(name), platform)?;
    for file in ["artisan", ".env.example"] {
        let metadata = fs::symlink_metadata(root.join(file))
            .with_context(|| format!("build_artifact_file_missing: {file}"))?;
        if !metadata.is_file() {
            bail!("build_artifact_not_regular_file: {file}");
        }
        #[cfg(unix)]
        if file == "artisan" {
            use std::os::unix::fs::PermissionsExt as _;
            if metadata.permissions().mode() & 0o111 == 0 {
                bail!("build_artisan_not_executable");
            }
        }
    }
    for directory in ["public", "resources", "storage", "bootstrap/cache"] {
        let path = root.join(directory);
        if !path.is_dir() {
            bail!("build_artifact_directory_missing: {directory}");
        }
        if matches!(directory, "storage" | "bootstrap/cache")
            && fs::read_dir(&path)?.next().is_some()
        {
            bail!("build_artifact_directory_not_empty: {directory}");
        }
    }
    Ok(())
}

fn validate_binary(path: &Path, platform: &str) -> anyhow::Result<()> {
    let metadata = fs::symlink_metadata(path).context("build_binary_missing")?;
    if !metadata.file_type().is_file() {
        bail!("build_binary_not_file");
    }
    let mut header = [0_u8; 64];
    File::open(path)?
        .read_exact(&mut header)
        .context("build_binary_header_invalid")?;
    let matches_platform = match platform {
        "linux/amd64" => elf_executable(&header, 62),
        "linux/arm64" => elf_executable(&header, 183),
        "darwin/amd64" => macho_executable(&header, 0x0100_0007),
        "darwin/arm64" => macho_executable(&header, 0x0100_000c),
        _ => bail!("build_platform_invalid"),
    };
    if !matches_platform {
        bail!("build_binary_platform_mismatch");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        if metadata.permissions().mode() & 0o111 == 0 {
            bail!("build_binary_not_executable");
        }
    }
    Ok(())
}

fn elf_executable(header: &[u8; 64], machine: u16) -> bool {
    &header[..7] == b"\x7fELF\x02\x01\x01"
        && matches!(u16::from_le_bytes([header[16], header[17]]), 2 | 3)
        && u16::from_le_bytes([header[52], header[53]]) == 64
        && u16::from_le_bytes([header[18], header[19]]) == machine
}

const MACHO_64_LE: [u8; 4] = [0xcf, 0xfa, 0xed, 0xfe];

fn macho_executable(header: &[u8; 64], machine: u32) -> bool {
    header[..4] == MACHO_64_LE
        && header[4..8] == machine.to_le_bytes()
        && header[12..16] == 2_u32.to_le_bytes()
}

fn validate_entries(root: &Path, application: Option<&str>) -> anyhow::Result<usize> {
    let mut binaries = 0;
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let name = entry.file_name();
        if name == INCOMPLETE {
            bail!("build_reserved_artifact_name");
        }
        let name = name.to_str().context("build_artifact_name_not_utf8")?;
        let is_application = application == Some(name);
        if application.is_some() && !is_application && !PACKAGE_ENTRIES.contains(&name) {
            bail!("build_unexpected_artifact");
        }
        let lower = name.to_ascii_lowercase();
        if !is_application
            && ((application.is_none() && lower.starts_with(".env."))
                || lower == ".env"
                || lower.contains(".sqlite")
                || matches!(lower.as_str(), "id_rsa" | "id_ed25519")
                || [".key", ".p12", ".pfx", ".db", ".sql"]
                    .iter()
                    .any(|suffix| lower.ends_with(suffix)))
        {
            bail!("build_sensitive_artifact");
        }
        let kind = entry.file_type()?;
        if kind.is_dir() {
            binaries += validate_entries(&entry.path(), None)?;
        } else if kind.is_file() {
            let mut prefix = Vec::new();
            File::open(entry.path())?
                .take(16)
                .read_to_end(&mut prefix)?;
            if prefix.starts_with(b"SQLite format 3\0") {
                bail!("build_sensitive_artifact");
            }
            binaries +=
                usize::from(prefix.starts_with(b"\x7fELF") || prefix.starts_with(&MACHO_64_LE));
        } else {
            bail!("build_artifact_special_file: 产物不得包含符号链接或特殊文件");
        }
    }
    Ok(binaries)
}

fn publish(source: &Path, destination: &Path, application: &str) -> anyhow::Result<()> {
    // create_dir 本身拒绝空目录和断链；不能使用可能替换既有空目录的 rename。
    fs::create_dir(destination).context("build_output_reservation_failed: 目标已存在或无法创建")?;
    let result = (|| -> anyhow::Result<()> {
        let marker = destination.join(INCOMPLETE);
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&marker)?;
        file.write_all(b"incomplete\n")?;
        drop(file);
        // 直接执行的包根程序须等运行资源复制完成，才赋予执行权限。
        copy_tree(source, destination, Some(application))?;
        fs::remove_file(marker)?;
        Ok(())
    })();
    result.context("build_publish_incomplete: 新建产物目录发布未完成；该目录已保留，禁止部署")
}

fn copy_tree(source: &Path, destination: &Path, application: Option<&str>) -> anyhow::Result<()> {
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let name = entry.file_name();
        if application.is_some_and(|application| name == application) {
            continue;
        }
        copy_entry(&entry.path(), &destination.join(name))?;
    }
    if let Some(application) = application {
        copy_entry(&source.join(application), &destination.join(application))?;
    }
    Ok(())
}

fn copy_entry(source: &Path, target: &Path) -> anyhow::Result<()> {
    let metadata = fs::symlink_metadata(source)?;
    if metadata.is_dir() {
        fs::create_dir(target)?;
        copy_tree(source, target, None)?;
    } else if metadata.is_file() {
        let mut input = File::open(source)?;
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(target)?;
        std::io::copy(&mut input, &mut output)?;
        output.flush()?;
    } else {
        bail!("build_artifact_special_file");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        // 文件归属是执行构建的用户；保普通 mode，不传播 setuid/setgid/sticky。
        fs::set_permissions(
            target,
            fs::Permissions::from_mode(metadata.permissions().mode() & 0o777),
        )?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "../tests/unit/release.rs"]
mod tests;
