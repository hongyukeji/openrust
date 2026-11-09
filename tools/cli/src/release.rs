//! 复用 Compose 构建定义，导出不覆盖既有目录的 Linux 应用产物。

use std::fs::{self, File, OpenOptions};
use std::io::{Read as _, Write as _};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

use anyhow::{Context as _, bail};
use serde_json::Value;

const INCOMPLETE: &str = ".openrust-build-incomplete";
const PLAN_LIMIT: u64 = 8 * 1024 * 1024;

#[derive(Debug, clap::Args)]
pub(crate) struct BuildArgs {
    #[arg(long, hide = true)]
    project_dir: Option<PathBuf>,
    /// 单一 Linux 平台，默认按当前 CLI 宿主 CPU 选择。
    #[arg(long, value_parser = ["linux/amd64", "linux/arm64"])]
    platform: Option<String>,
    /// 全新产物目录；显式路径的父目录须已存在。
    #[arg(long)]
    output: Option<PathBuf>,
}

impl BuildArgs {
    pub(crate) fn run(self) -> anyhow::Result<ExitCode> {
        let project = self.project_dir.map_or_else(std::env::current_dir, Ok)?;
        let project = project.canonicalize().context("build_project_invalid")?;
        path_text(&project)?;
        let platform = self.platform.map_or_else(
            || host_platform(std::env::consts::ARCH).map(str::to_owned),
            Ok,
        )?;
        let manifest = read_manifest(&project)?;
        let name = application_name(&manifest)?;
        let destination = destination(&project, name, &platform, self.output.as_deref())?;
        reject_existing(&destination)?;
        let mut staging = tempfile::Builder::new();
        staging.prefix("openrust-release-");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            staging.permissions(fs::Permissions::from_mode(0o700));
        }
        let temporary = staging.tempdir().context("build_staging_create_failed")?;
        let result = build_and_publish(
            &project,
            &manifest,
            &platform,
            name,
            &destination,
            temporary.path(),
        );
        let cleanup = temporary.close();
        if cleanup.is_err() {
            eprintln!("build_staging_cleanup_failed: 自有临时目录清理失败");
        }
        result?;
        writeln!(std::io::stdout().lock(), "{}", destination.display())?;
        Ok(ExitCode::SUCCESS)
    }
}

fn host_platform(architecture: &str) -> anyhow::Result<&'static str> {
    match architecture {
        "x86_64" => Ok("linux/amd64"),
        "aarch64" => Ok("linux/arm64"),
        _ => bail!("build_platform_required: 请显式选择 linux/amd64 或 linux/arm64"),
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

fn capture_plan(project: &Path, capture: &Path) -> anyhow::Result<Value> {
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(capture)?;
    let mut command = docker(project);
    command
        .args(["-f", "docker-compose.yml", "--print", "app"])
        .stdout(file);
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

fn build_and_publish(
    project: &Path,
    manifest: &toml_edit::DocumentMut,
    platform: &str,
    name: &str,
    destination: &Path,
    temporary: &Path,
) -> anyhow::Result<()> {
    let temporary = temporary.canonicalize()?;
    if temporary.starts_with(project) {
        bail!("build_staging_in_context: 临时目录须在构建上下文外");
    }
    let artifacts = temporary.join("artifacts");
    let plan = capture_plan(project, &temporary.join("capture.json"))?;
    let (plan, framework) = freeze_plan(project, manifest, &plan, platform, &artifacts)?;
    if framework
        .as_ref()
        .is_some_and(|path| temporary.starts_with(path))
    {
        bail!("build_staging_in_context: 临时目录须在构建上下文外");
    }
    fs::create_dir(&artifacts)?;
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
        .arg(format!("fs.write={}", path_text(&artifacts)?));
    if let Some(framework) = framework {
        command
            .arg("--allow")
            .arg(format!("fs.read={}", path_text(&framework)?));
    }
    command
        .args(["--progress=quiet", "app"])
        .stdout(Stdio::null());
    run_docker(&mut command, "build_artifacts_failed")?;
    validate_release(&artifacts, name, platform)?;
    publish(&artifacts, destination)
}

fn validate_release(root: &Path, name: &str, platform: &str) -> anyhow::Result<()> {
    if validate_tree(root)? != 1 {
        bail!("build_elf_count_invalid: 产物必须只有一个 ELF 应用");
    }
    validate_binary(&root.join("bin").join(name), platform)?;
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
        .context("build_binary_not_elf")?;
    let expected_machine = match platform {
        "linux/amd64" => 62,
        "linux/arm64" => 183,
        _ => bail!("build_platform_invalid"),
    };
    if &header[..4] != b"\x7fELF"
        || header[4] != 2
        || header[5] != 1
        || header[6] != 1
        || !matches!(u16::from_le_bytes([header[16], header[17]]), 2 | 3)
        || u16::from_le_bytes([header[52], header[53]]) != 64
        || u16::from_le_bytes([header[18], header[19]]) != expected_machine
    {
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

fn validate_tree(root: &Path) -> anyhow::Result<usize> {
    validate_entries(root, true)
}

fn validate_entries(root: &Path, top: bool) -> anyhow::Result<usize> {
    let mut binaries = 0;
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let name = entry.file_name();
        if name == INCOMPLETE {
            bail!("build_reserved_artifact_name");
        }
        let name = name.to_str().context("build_artifact_name_not_utf8")?;
        if top
            && !matches!(
                name,
                "bin"
                    | "artisan"
                    | ".env.example"
                    | "public"
                    | "resources"
                    | "lang"
                    | "modules"
                    | "storage"
                    | "bootstrap"
            )
        {
            bail!("build_unexpected_artifact");
        }
        let lower = name.to_ascii_lowercase();
        if (!top && lower.starts_with(".env."))
            || lower == ".env"
            || lower.contains(".sqlite")
            || matches!(lower.as_str(), "id_rsa" | "id_ed25519")
            || [".key", ".p12", ".pfx", ".db", ".sql"]
                .iter()
                .any(|suffix| lower.ends_with(suffix))
        {
            bail!("build_sensitive_artifact");
        }
        let kind = entry.file_type()?;
        if kind.is_dir() {
            binaries += validate_entries(&entry.path(), false)?;
        } else if kind.is_file() {
            let mut prefix = Vec::new();
            File::open(entry.path())?
                .take(16)
                .read_to_end(&mut prefix)?;
            if prefix.starts_with(b"SQLite format 3\0") {
                bail!("build_sensitive_artifact");
            }
            binaries += usize::from(prefix.starts_with(b"\x7fELF"));
        } else {
            bail!("build_artifact_special_file: 产物不得包含符号链接或特殊文件");
        }
    }
    Ok(binaries)
}

fn publish(source: &Path, destination: &Path) -> anyhow::Result<()> {
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
        copy_tree(source, destination)?;
        fs::remove_file(marker)?;
        Ok(())
    })();
    result.context("build_publish_incomplete: 新建产物目录发布未完成；该目录已保留，禁止部署")
}

fn copy_tree(source: &Path, destination: &Path) -> anyhow::Result<()> {
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let target = destination.join(entry.file_name());
        let metadata = fs::symlink_metadata(entry.path())?;
        if metadata.is_dir() {
            fs::create_dir(&target)?;
            copy_tree(&entry.path(), &target)?;
        } else if metadata.is_file() {
            let mut input = File::open(entry.path())?;
            let mut output = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&target)?;
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
                &target,
                fs::Permissions::from_mode(metadata.permissions().mode() & 0o777),
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "../tests/unit/release.rs"]
mod tests;
