//! Artisan 生成与源码、部署分流的进程级回归。

use super::deployment_tests::assert_generated_deployment_artifacts;
use super::workspace_entry_tests::create_framework_fixture;
use super::{create_project, populate_project};
use anyhow::{Context, Result};
use std::fmt::Write as _;
use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const ARTISAN: &[u8] = include_bytes!("../../../../artisan");
const FAKE_APPLICATION: &[u8] = br#"#!/bin/sh
printf '%s\n' "$$" "${0##*/}" "$(pwd -P)" "${OPENRUST_ARTISAN-unset}" "$#"
for argument do
    printf '<%s>\n' "$argument"
done
exit 37
"#;

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Result<Self> {
        let mut random = [0_u8; 8];
        getrandom::fill(&mut random).map_err(|_| anyhow::anyhow!("无法生成测试目录名"))?;
        let root = std::env::temp_dir().join(format!(
            "openrust-artisan-{}-{:x}",
            std::process::id(),
            u64::from_ne_bytes(random)
        ));
        fs::create_dir(&root)?;
        let fixture = Self { root };
        fs::create_dir(fixture.root.join("bin"))?;
        write_executable(&fixture.root.join("bin/cargo"), FAKE_APPLICATION)?;
        Ok(fixture)
    }

    fn run(&self, project: &Path, args: &[&str]) -> Result<(u32, Output)> {
        let bin = self.root.join("bin");
        let path = std::env::join_paths([bin.as_path(), Path::new("/usr/bin"), Path::new("/bin")])?;
        let child = Command::new(project.join("artisan"))
            .current_dir(&self.root)
            .env("PATH", path)
            .env_remove("OPENRUST_ARTISAN")
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("启动 Artisan 入口")?;
        Ok((child.id(), child.wait_with_output()?))
    }

    fn assert_exec(
        &self,
        project: &Path,
        args: &[&str],
        program: &str,
        forwarded: &[&str],
    ) -> Result<()> {
        self.assert_exec_from(project, args, project, program, forwarded)
    }

    fn assert_exec_from(
        &self,
        project: &Path,
        args: &[&str],
        working_directory: &Path,
        program: &str,
        forwarded: &[&str],
    ) -> Result<()> {
        let (pid, output) = self.run(project, args)?;
        assert_eq!(
            output.status.code(),
            Some(37),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(output.stderr, b"");
        let mut expected = format!(
            "{pid}\n{program}\n{}\nunset\n{}\n",
            working_directory.canonicalize()?.display(),
            forwarded.len()
        );
        for argument in forwarded {
            writeln!(expected, "<{argument}>")?;
        }
        assert_eq!(String::from_utf8(output.stdout)?, expected);
        Ok(())
    }

    fn assert_deployment_errors(&self, project: &Path, program: &str) -> Result<()> {
        let target = format!("./{program}");
        write_executable(&project.join("app"), FAKE_APPLICATION)?;
        fs::create_dir(project.join("bin"))?;
        write_executable(&project.join("bin").join(program), FAKE_APPLICATION)?;
        let (_, missing) = self.run(project, &[])?;
        assert_deployment_error(missing, 127, &target)?;

        let binary = project.join(program);
        fs::write(&binary, FAKE_APPLICATION)?;
        fs::set_permissions(binary, fs::Permissions::from_mode(0o644))?;
        let (_, not_executable) = self.run(project, &[])?;
        assert_deployment_error(not_executable, 126, &target)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn write_executable(path: &Path, bytes: &[u8]) -> Result<()> {
    fs::write(path, bytes)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
    Ok(())
}

fn assert_deployment_error(output: Output, code: i32, target: &str) -> Result<()> {
    let diagnostic = String::from_utf8(output.stderr)?;
    assert_eq!(output.status.code(), Some(code), "{diagnostic}");
    assert_eq!(output.stdout, b"");
    assert!(
        diagnostic.contains("artisan") && diagnostic.contains(target),
        "{diagnostic}"
    );
    Ok(())
}

#[test]
fn generated_artisan_runs_source_and_relocated_deployments_by_project_name() -> Result<()> {
    let fixture = Fixture::new()?;
    let framework = fixture.root.join("framework source");
    let source = fixture.root.join("source projects");
    let deployment = fixture.root.join("original deployment");
    let relocated = fixture.root.join("relocated deployment with spaces");
    fs::create_dir_all(&framework)?;
    fs::create_dir(&source)?;
    fs::create_dir(&deployment)?;
    create_framework_fixture(&framework);
    let reserved = ["artisan", "public", "storage", "modules"];
    let publishable = ["blog", "bin"];
    for name in reserved.into_iter().chain(publishable) {
        let project = source.join(name);
        create_project(
            project.to_str().context("应用目录必须为 UTF-8")?,
            None,
            false,
            false,
            false,
            false,
            &framework,
        )?;
        assert_generated_deployment_artifacts(&project, name)?;
        let script = project.join("artisan");
        assert_eq!(fs::metadata(&script)?.permissions().mode() & 0o777, 0o755);
        if reserved.contains(&name) {
            fixture.assert_exec(
                &project,
                &["--env=testing", "queue:work", "two words", ""],
                "cargo",
                &[
                    "run",
                    "--quiet",
                    "--features",
                    "source-artisan",
                    "--",
                    "artisan",
                    "--env=testing",
                    "queue:work",
                    "two words",
                    "",
                ],
            )?;
            if name == "artisan" {
                let installed = deployment.join(name);
                fs::create_dir(&installed)?;
                fs::copy(&script, installed.join("artisan"))?;
                let (_, collision) = fixture.run(&installed, &[])?;
                assert_deployment_error(collision, 126, "Artisan 启动脚本冲突")?;
            }
            continue;
        }
        let installed = deployment.join(name);
        fs::create_dir(&installed)?;
        fs::copy(&script, installed.join("artisan"))?;
        write_executable(&installed.join(name), FAKE_APPLICATION)?;
    }

    fs::rename(&deployment, &relocated)?;
    let args = ["schedule:run", "artisan", "two words", ""];
    for name in publishable {
        fixture.assert_exec(
            &relocated.join(name),
            &args,
            name,
            &["artisan", "schedule:run", "artisan", "two words", ""],
        )?;
    }
    Ok(())
}

#[test]
fn source_artisan_dev_execs_cli_with_selected_environment() -> Result<()> {
    let fixture = Fixture::new()?;
    let project = fixture.root.join("source project");
    populate_project(
        &project,
        "artisan-dev-fixture",
        "ArtisanDevFixture",
        None,
        false,
        false,
        false,
        false,
    )?;
    fs::create_dir_all(project.join("tools/cli"))?;
    fs::write(project.join("tools/cli/Cargo.toml"), "")?;
    fixture.assert_exec(
        &project,
        &["--env", "local", "dev"],
        "cargo",
        &[
            "run",
            "--quiet",
            "--manifest-path",
            "tools/cli/Cargo.toml",
            "--bin",
            "openrust",
            "--",
            "dev",
            "--watch",
            "--env",
            "local",
        ],
    )
}

#[test]
fn source_artisan_discovery_uses_cli_without_building_the_application() -> Result<()> {
    let fixture = Fixture::new()?;
    let project = fixture.root.join("source project");
    fs::create_dir_all(project.join("src"))?;
    fs::write(project.join("Cargo.toml"), "")?;
    fs::write(project.join("src/main.rs"), "")?;
    write_executable(&project.join("artisan"), ARTISAN)?;
    write_executable(&fixture.root.join("bin/openrust"), FAKE_APPLICATION)?;
    let args = ["package:discover", "--check", "--features", "with-provider"];
    fixture.assert_exec(&project, &args, "openrust", &args)?;

    fs::create_dir_all(project.join("tools/cli"))?;
    fs::write(project.join("tools/cli/Cargo.toml"), "")?;
    let project_directory = project.canonicalize()?;
    fixture.assert_exec(
        &project,
        &args,
        "cargo",
        &[
            "run",
            "--quiet",
            "--manifest-path",
            "tools/cli/Cargo.toml",
            "--bin",
            "openrust",
            "--locked",
            "--",
            "package:discover",
            "--project-dir",
            project_directory.to_str().context("应用目录必须为 UTF-8")?,
            "--check",
            "--features",
            "with-provider",
        ],
    )
}

#[test]
fn source_artisan_keeps_build_in_the_application_command_table() -> Result<()> {
    let fixture = Fixture::new()?;
    let project = fixture.root.join("source project without cli");
    fs::create_dir(&project)?;
    fs::write(project.join("Cargo.toml"), "")?;
    fs::write(project.join("main.rs"), "")?;
    write_executable(&project.join("artisan"), ARTISAN)?;
    write_executable(&fixture.root.join("bin/openrust"), FAKE_APPLICATION)?;
    fs::create_dir_all(project.join("tools/cli"))?;
    fs::write(project.join("tools/cli/Cargo.toml"), "")?;
    fixture.assert_exec(
        &project,
        &["--env", "testing", "build", "two words", ""],
        "cargo",
        &[
            "run",
            "--quiet",
            "--features",
            "source-artisan",
            "--",
            "artisan",
            "--env",
            "testing",
            "build",
            "two words",
            "",
        ],
    )
}

#[test]
fn deployed_root_artisan_execs_openrust_without_cargo() -> Result<()> {
    let fixture = Fixture::new()?;
    let project = fixture.root.join("deployed project");
    fs::create_dir(&project)?;
    fs::write(project.join("Cargo.toml"), "")?;
    write_executable(&project.join("artisan"), ARTISAN)?;
    write_executable(&project.join("openrust"), FAKE_APPLICATION)?;
    fixture.assert_exec(&project, &[], "openrust", &["artisan"])?;
    fixture.assert_exec(
        &project,
        &["schedule:run", "two words", ""],
        "openrust",
        &["artisan", "schedule:run", "two words", ""],
    )
}

#[test]
fn deployed_root_artisan_reports_missing_or_non_executable_openrust() -> Result<()> {
    let fixture = Fixture::new()?;
    let project = fixture.root.join("incomplete project");
    fs::create_dir_all(project.join("src"))?;
    fs::write(project.join("src/main.rs"), "")?;
    write_executable(&project.join("artisan"), ARTISAN)?;
    fixture.assert_deployment_errors(&project, "openrust")
}

#[test]
fn deployed_artisan_refuses_incomplete_output_before_executing_migration() -> Result<()> {
    let fixture = Fixture::new()?;
    let project = fixture.root.join("incomplete deployment");
    fs::create_dir(&project)?;
    write_executable(&project.join("artisan"), ARTISAN)?;
    write_executable(&project.join("openrust"), FAKE_APPLICATION)?;
    fs::write(project.join(".openrust-build-incomplete"), b"incomplete\n")?;
    let (_, output) = fixture.run(&project, &["migrate", "--force"])?;
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(output.stdout, [] as [u8; 0]);
    assert!(String::from_utf8(output.stderr)?.contains("发布目录尚未完成"));
    Ok(())
}
