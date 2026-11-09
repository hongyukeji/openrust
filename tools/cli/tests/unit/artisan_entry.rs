//! Artisan 生成与源码、部署分流的进程级回归。

use super::populate_project;
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
        let (pid, output) = self.run(project, args)?;
        assert_eq!(
            output.status.code(),
            Some(37),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(output.stderr, b"");
        let mut expected = format!(
            "{pid}\n{program}\n{}\n1\n{}\n",
            project.canonicalize()?.display(),
            forwarded.len()
        );
        for argument in forwarded {
            writeln!(expected, "<{argument}>")?;
        }
        assert_eq!(String::from_utf8(output.stdout)?, expected);
        Ok(())
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

#[test]
fn generated_artisan_is_executable_and_runs_source_through_cargo() -> Result<()> {
    let fixture = Fixture::new()?;
    let project = fixture.root.join("source project");
    populate_project(
        &project,
        "artisan-fixture",
        "ArtisanFixture",
        None,
        false,
        false,
        false,
        false,
    )?;
    let script = project.join("artisan");
    assert_eq!(fs::read(&script)?, ARTISAN);
    assert_eq!(fs::metadata(&script)?.permissions().mode() & 0o777, 0o755);
    fixture.assert_exec(
        &project,
        &["--env=testing", "queue:work", "two words", ""],
        "cargo",
        &[
            "run",
            "--quiet",
            "--",
            "--env=testing",
            "queue:work",
            "two words",
            "",
        ],
    )
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
            "--check",
            "--features",
            "with-provider",
        ],
    )
}

#[test]
fn deployed_artisan_execs_adjacent_app_without_cargo() -> Result<()> {
    let fixture = Fixture::new()?;
    let project = fixture.root.join("deployed project");
    fs::create_dir(&project)?;
    fs::write(project.join("Cargo.toml"), "")?;
    write_executable(&project.join("artisan"), ARTISAN)?;
    write_executable(&project.join("app"), FAKE_APPLICATION)?;
    fixture.assert_exec(
        &project,
        &["schedule:run", "two words", ""],
        "app",
        &["schedule:run", "two words", ""],
    )
}

#[test]
fn deployed_artisan_reports_missing_app_without_cargo() -> Result<()> {
    let fixture = Fixture::new()?;
    let project = fixture.root.join("incomplete project");
    fs::create_dir_all(project.join("src"))?;
    fs::write(project.join("src/main.rs"), "")?;
    write_executable(&project.join("artisan"), ARTISAN)?;

    let (_, output) = fixture.run(&project, &[])?;
    assert_eq!(output.status.code(), Some(127));
    assert_eq!(output.stdout, b"");
    let diagnostic = String::from_utf8(output.stderr)?;
    assert!(diagnostic.contains("artisan") && diagnostic.contains("./app"));
    Ok(())
}
