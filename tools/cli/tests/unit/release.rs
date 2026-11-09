use super::{
    BuildArgs, application_name, bake_path, bake_plan_command, destination, freeze_plan,
    host_platform, path_text, publish, read_manifest, reject_existing,
};
#[cfg(unix)]
use super::{INCOMPLETE, validate_binary, validate_entries, validate_release};
use crate::{Builtin, Cli};
#[cfg(unix)]
use anyhow::Context as _;
use clap::Parser as _;
use openrust::config::{ConfigLoader, ENV_PREFIX};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn fixture() -> anyhow::Result<(tempfile::TempDir, PathBuf)> {
    let temporary = tempfile::tempdir()?;
    let project = temporary.path().join("app with spaces,comma");
    fs::create_dir(&project)?;
    fs::write(project.join("Cargo.toml"), "[package]\nname = \"blog\"\n")?;
    fs::write(project.join("Dockerfile"), "FROM scratch AS artifacts\n")?;
    Ok((temporary, project.canonicalize()?))
}

#[test]
fn platform_selection_preserves_native_os_and_cpu() -> anyhow::Result<()> {
    assert_eq!(host_platform("linux", "x86_64")?, "linux/amd64");
    assert_eq!(host_platform("linux", "aarch64")?, "linux/arm64");
    assert_eq!(host_platform("macos", "x86_64")?, "darwin/amd64");
    assert_eq!(host_platform("macos", "aarch64")?, "darwin/arm64");
    assert!(host_platform("linux", "riscv64").is_err());
    Ok(())
}

#[test]
fn package_name_and_output_are_separate_from_cargo_binary_target() -> anyhow::Result<()> {
    let (_temporary, project) = fixture()?;
    let manifest = read_manifest(&project)?;
    let name = application_name(&manifest)?;
    assert_eq!(name, "blog");
    assert_eq!(
        destination(&project, name, "linux/arm64", None)?,
        project.join("dist/blog-linux-arm64")
    );
    assert!(destination(&project, name, "linux/amd64", Some(Path::new("../escape"))).is_err());
    Ok(())
}

#[test]
fn build_rejects_reserved_package_names_before_creating_the_output() -> anyhow::Result<()> {
    let (_temporary, project) = fixture()?;
    for name in [
        "artisan",
        "PUBLIC",
        "storage",
        "Bootstrap",
        "resources",
        "lang",
        "modules",
    ] {
        fs::write(
            project.join("Cargo.toml"),
            format!("[package]\nname = {name:?}\n"),
        )?;
        let result = BuildArgs {
            project_dir: Some(project.clone()),
            docker: true,
            platform: Some("linux/arm64".to_owned()),
            env: None,
            features: None,
            output: None,
        }
        .run();
        assert!(
            result.is_err_and(|error| error.to_string().contains("build_package_name_conflict")),
            "{name}"
        );
        assert!(!project.join("dist").exists(), "{name}");
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn docker_packager_rejects_reserved_names_and_an_occupied_output() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let packager = include_str!("../../templates/deploy/package-assets.sh");
    for name in [
        "Artisan",
        "public",
        "storage",
        "bootstrap",
        "resources",
        "lang",
        "modules",
    ] {
        let output = temp.path().join("output");
        let result = std::process::Command::new("sh")
            .args(["-c", packager, "package-assets"])
            .arg(&output)
            .arg(name)
            .current_dir(temp.path())
            .output()?;
        assert!(!result.status.success(), "{name}");
        assert!(
            String::from_utf8(result.stderr)?.contains("build_package_name_conflict"),
            "{name}"
        );
        assert!(!output.exists(), "{name}");
    }
    fs::write(temp.path().join(".env.example"), "")?;
    fs::write(temp.path().join("artisan"), "new launcher")?;
    fs::create_dir(temp.path().join("public"))?;
    fs::create_dir(temp.path().join("resources"))?;
    let output = temp.path().join("occupied");
    fs::create_dir(&output)?;
    fs::write(output.join("artisan"), "keep launcher")?;
    let result = std::process::Command::new("sh")
        .args(["-c", packager, "package-assets"])
        .arg(&output)
        .arg("blog")
        .current_dir(temp.path())
        .output()?;
    assert!(!result.status.success());
    assert_eq!(fs::read_to_string(output.join("artisan"))?, "keep launcher");
    assert!(!output.join("public").exists());
    Ok(())
}

#[test]
fn bake_capture_uses_a_private_copy_only_for_the_selected_environment() -> anyhow::Result<()> {
    use std::ffi::{OsStr, OsString};

    let (_temporary, project) = fixture()?;
    let staging = tempfile::tempdir()?;
    let default_dotenv = project.join(".env");
    let selected_dotenv = project.join(".env.candidate");
    let args = [
        OsString::from("buildx"),
        OsString::from("bake"),
        OsString::from("-f"),
        project.join("docker-compose.yml").into_os_string(),
        OsString::from("--print"),
        OsString::from("app"),
    ];
    let args = args
        .iter()
        .map(OsString::as_os_str)
        .collect::<Vec<&OsStr>>();

    fs::write(&default_dotenv, b"OPENRUST_BUILD_FEATURES=default\n")?;
    let dotenv = ConfigLoader::application_dotenv_path(&project, ENV_PREFIX, Some("missing"))?;
    let fallback = bake_plan_command(&project, staging.path(), &dotenv)?;
    assert_eq!(fallback.get_current_dir(), Some(project.as_path()));
    assert_eq!(fallback.get_args().collect::<Vec<_>>(), args);
    assert!(!staging.path().join(".env").exists());

    let selected_bytes = b"OPENRUST_BUILD_FEATURES=candidate\nPRIVATE_FIXTURE=opaque\n";
    fs::write(&selected_dotenv, selected_bytes)?;
    let dotenv = ConfigLoader::application_dotenv_path(&project, ENV_PREFIX, Some("candidate"))?;
    let selected = bake_plan_command(&project, staging.path(), &dotenv)?;
    assert_eq!(selected.get_current_dir(), Some(staging.path()));
    assert_eq!(selected.get_args().collect::<Vec<_>>(), args);
    assert_eq!(
        selected.get_envs().collect::<Vec<_>>(),
        fallback.get_envs().collect::<Vec<_>>(),
    );
    let snapshot = staging.path().join(".env");
    assert_eq!(fs::read(&snapshot)?, selected_bytes);
    assert_eq!(fs::read(&selected_dotenv)?, selected_bytes);
    assert_eq!(
        fs::read(&default_dotenv)?,
        b"OPENRUST_BUILD_FEATURES=default\n"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        assert_eq!(fs::metadata(&snapshot)?.permissions().mode() & 0o077, 0);
    }
    let second_copy = bake_plan_command(&project, staging.path(), &dotenv);
    assert!(
        second_copy.is_err_and(|error| error.to_string().contains("build_dotenv_snapshot_failed"))
    );
    assert_eq!(fs::read(snapshot)?, selected_bytes);
    Ok(())
}

#[test]
fn bake_paths_keep_punctuation_without_template_expansion() -> anyhow::Result<()> {
    assert_eq!(
        bake_path(Path::new(r#"/tmp/a b,c"d/${app}/%{plan}"#))?,
        r#"/tmp/a b,c"d/$${app}/%%{plan}"#
    );
    assert!(path_text(Path::new("/tmp/allow*everything")).is_err());
    assert!(path_text(Path::new("/tmp/new\nline")).is_err());
    Ok(())
}

#[test]
fn bake_snapshot_keeps_args_and_exact_named_context() -> anyhow::Result<()> {
    let (temporary, project) = fixture()?;
    let framework = temporary.path().join("framework, with spaces");
    let dependency = framework.join("vendor/openrust/framework");
    fs::create_dir_all(&dependency)?;
    fs::write(framework.join("Cargo.toml"), "[workspace]")?;
    fs::write(dependency.join("Cargo.toml"), "[package]")?;
    let manifest = r#"[workspace.dependencies]
openrust = { path = "../framework, with spaces/vendor/openrust/framework" }
"#
    .parse()?;
    let framework = framework.canonicalize()?;
    let args = serde_json::json!({"APP_FEATURES": "db-mysql,queue-redis", "OPENRUST_APP_LAYOUT": "app", "LITERAL": "$${value}"});
    let source = serde_json::json!({"target": {"app": {
        "context": ".", "dockerfile": "Dockerfile", "args": args,
        "contexts": {"openrust-source": "../framework, with spaces"},
        "target": "runtime", "platforms": ["linux/amd64"],
        "output": [{"type": "registry"}], "tags": ["remote/image:test"]
    }}});
    let artifacts = Path::new("/tmp/exact-stage");
    let (frozen, allowed) = freeze_plan(&project, &manifest, &source, "linux/arm64", artifacts)?;
    assert_eq!(frozen["target"]["app"]["target"], "artifacts");
    assert_eq!(frozen["target"]["app"]["output"][0]["type"], "local");
    assert!(frozen["target"]["app"].get("tags").is_none());
    assert_eq!(allowed, Some(framework.clone()));
    assert_eq!(frozen["target"]["app"]["args"], args);
    assert_eq!(
        frozen["target"]["app"]["contexts"]["openrust-source"],
        path_text(&framework)?
    );
    assert_eq!(frozen["target"]["app"]["context"], path_text(&project)?);
    assert_eq!(
        frozen["target"]["app"]["platforms"],
        serde_json::json!(["linux/arm64"])
    );
    assert_eq!(
        frozen["target"]["app"]["output"][0]["dest"],
        "/tmp/exact-stage"
    );
    Ok(())
}

#[test]
fn unsupported_contexts_and_privileged_fields_fail_before_build() -> anyhow::Result<()> {
    let (_temporary, project) = fixture()?;
    let manifest = read_manifest(&project)?;
    for extra in [
        serde_json::json!({"contexts": {"other": "."}}),
        serde_json::json!({"contexts": {"openrust-source": "target:other"}}),
        serde_json::json!({"contexts": {"openrust-source": "/"}}),
        serde_json::json!({"secrets": [{"id": "private"}]}),
        serde_json::json!({"cache-to": [{"type": "local", "dest": "/private"}]}),
        serde_json::json!({"entitlements": ["security.insecure"]}),
    ] {
        assert!(
            freeze_plan(
                &project,
                &manifest,
                &serde_json::json!({"target": {"app": extra}}),
                "linux/arm64",
                Path::new("/tmp/output")
            )
            .is_err()
        );
    }
    Ok(())
}

#[test]
fn publication_refuses_existing_empty_and_nonempty_directories_and_files() -> anyhow::Result<()> {
    let temporary = tempfile::tempdir()?;
    let source = temporary.path().join("source");
    fs::create_dir(&source)?;
    fs::write(source.join("new"), "new")?;
    for kind in ["empty", "nonempty", "file"] {
        let output = temporary.path().join(kind);
        if kind == "file" {
            fs::write(&output, "old")?;
        } else {
            fs::create_dir(&output)?;
            if kind == "nonempty" {
                fs::write(output.join("sentinel"), "old")?;
            }
        }
        assert!(reject_existing(&output).is_err());
        assert!(publish(&source, &output, "blog").is_err());
        assert!(!output.join("new").exists());
        match kind {
            "nonempty" => assert_eq!(fs::read_to_string(output.join("sentinel"))?, "old"),
            "file" => assert_eq!(fs::read_to_string(&output)?, "old"),
            _ => assert_eq!(fs::read_dir(&output)?.count(), 0),
        }
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn publication_refuses_existing_and_dangling_symlinks() -> anyhow::Result<()> {
    use std::os::unix::fs::symlink;
    let temporary = tempfile::tempdir()?;
    let source = temporary.path().join("source");
    fs::create_dir(&source)?;
    fs::write(source.join("new"), "new")?;
    for exists in [false, true] {
        let victim = temporary.path().join(format!("victim-{exists}"));
        if exists {
            fs::create_dir(&victim)?;
        }
        let output = temporary.path().join(format!("output-{exists}"));
        symlink(&victim, &output)?;
        assert!(reject_existing(&output).is_err());
        assert!(publish(&source, &output, "blog").is_err());
        assert_eq!(fs::read_link(&output)?, victim);
        assert_eq!(victim.exists(), exists);
        assert!(!victim.join("new").exists());
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn copied_modes_keep_executable_bits_and_no_special_bits() -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    let temporary = tempfile::tempdir()?;
    let source = temporary.path().join("source");
    fs::create_dir(&source)?;
    fs::write(source.join("blog"), "application")?;
    fs::set_permissions(source.join("blog"), fs::Permissions::from_mode(0o4751))?;
    let output = temporary.path().join("new-release");
    publish(&source, &output, "blog")?;
    assert_eq!(fs::read_to_string(output.join("blog"))?, "application");
    assert_eq!(
        fs::metadata(output.join("blog"))?.permissions().mode() & 0o7777,
        0o751
    );
    assert!(!output.join(INCOMPLETE).exists());
    Ok(())
}

#[cfg(unix)]
#[test]
fn resource_copy_failure_keeps_the_marker_and_never_exposes_the_binary() -> anyhow::Result<()> {
    use std::os::unix::fs::{PermissionsExt as _, symlink};
    let temporary = tempfile::tempdir()?;
    let source = temporary.path().join("source");
    fs::create_dir(&source)?;
    let executable = source.join("blog");
    fs::write(&executable, b"application")?;
    fs::set_permissions(executable, fs::Permissions::from_mode(0o755))?;
    fs::create_dir(source.join("resources"))?;
    let private = temporary.path().join("private");
    fs::write(&private, "private sentinel")?;
    symlink(&private, source.join("resources/link"))?;
    assert!(validate_entries(&source, Some("blog")).is_err());
    let output = temporary.path().join("partial");
    let error = publish(&source, &output, "blog")
        .err()
        .context("expected incomplete error")?;
    assert!(error.to_string().starts_with("build_publish_incomplete:"));
    assert_eq!(fs::read_to_string(output.join(INCOMPLETE))?, "incomplete\n");
    assert!(!output.join("resources/link").exists());
    assert!(!output.join("blog").exists());
    assert_eq!(fs::read_to_string(private)?, "private sentinel");
    Ok(())
}

fn parse_build(args: &[&str]) -> anyhow::Result<BuildArgs> {
    let cli = Cli::try_parse_from(
        ["openrust", "build"]
            .into_iter()
            .chain(args.iter().copied()),
    )?;
    let Some(Builtin::Build(options)) = cli.command else {
        anyhow::bail!("应分派到独立发布构建");
    };
    Ok(options)
}

#[test]
fn public_build_parser_requires_explicit_docker_for_linux_platform() -> anyhow::Result<()> {
    assert!(!parse_build(&[])?.docker);
    let options = parse_build(&[
        "--docker",
        "--platform",
        "linux/arm64",
        "--env",
        "production",
        "--output",
        "/tmp/with spaces,comma",
    ])?;
    assert!(options.docker);
    assert_eq!(options.platform.as_deref(), Some("linux/arm64"));
    assert_eq!(options.env.as_deref(), Some("production"));
    assert_eq!(
        options.output.as_deref(),
        Some(Path::new("/tmp/with spaces,comma"))
    );
    for args in [
        &["--platform", "linux/arm64"][..],
        &["--env", "production"][..],
        &["--docker", "--platform", "linux/amd64,linux/arm64"][..],
        &["--docker", "--platform", "darwin/arm64"][..],
        &["--docker", "--features", "view"][..],
    ] {
        assert!(parse_build(args).is_err(), "{args:?}");
    }
    Ok(())
}

#[cfg(all(
    any(target_os = "linux", target_os = "macos"),
    any(target_arch = "aarch64", target_arch = "x86_64")
))]
#[test]
fn default_build_uses_cargo_without_compose_and_runs_the_named_package() -> anyhow::Result<()> {
    let (_temporary, project) = fixture()?;
    fs::remove_file(project.join("Dockerfile"))?;
    for directory in ["src/bin", "public", "resources", ".cargo"] {
        fs::create_dir_all(project.join(directory))?;
    }
    fs::write(
        project.join("Cargo.toml"),
        "[package]\nname = \"blog\"\nversion = \"1.0.0\"\nedition = \"2024\"\ndefault-run = \"server\"\n\
         [[bin]]\nname = \"server\"\npath = \"src/main.rs\"\n[features]\nexample = []\n",
    )?;
    fs::write(
        project.join("src/main.rs"),
        r#"fn main() { print!("{}", if cfg!(feature = "example") { "with feature" } else { "release app" }); }"#,
    )?;
    fs::write(
        project.join("src/bin/other.rs"),
        "compile_error!(\"wrong binary\");",
    )?;
    fs::write(
        project.join(".cargo/config.toml"),
        "[build]\ntarget = \"wasm32-unknown-unknown\"\n",
    )?;
    fs::write(
        project.join("artisan"),
        "#!/bin/sh\nexec ./blog artisan \"$@\"\n",
    )?;
    fs::write(project.join(".env.example"), "APP_KEY=\n")?;
    fs::write(project.join(".env"), "APP_KEY=local-only\n")?;
    fs::write(project.join("public/index.html"), "public resource")?;

    let platform = host_platform(std::env::consts::OS, std::env::consts::ARCH)?;
    let release = project
        .join("dist")
        .join(format!("blog-{}", platform.replace('/', "-")));
    let build = |args: &[&str]| {
        let project = project.to_str().context("测试应用路径必须为 UTF-8")?;
        let args = ["--project-dir", project]
            .into_iter()
            .chain(args.iter().copied());
        parse_build(&args.collect::<Vec<_>>())?.run()
    };
    assert_eq!(build(&[])?, ExitCode::SUCCESS);
    let output = std::process::Command::new(release.join("blog"))
        .current_dir(&release)
        .output()?;
    assert!(output.status.success());
    assert_eq!(output.stdout, b"release app");
    let lock = fs::read(project.join("Cargo.lock"))?;
    assert!(!release.join(".env").exists());
    assert!(!release.join("bin").exists());
    assert_eq!(
        fs::read_to_string(release.join("public/index.html"))?,
        "public resource"
    );
    assert!(build(&[]).is_err());
    assert_eq!(
        fs::read_to_string(release.join(".env.example"))?,
        "APP_KEY=\n"
    );

    let feature_build = ["--features", "example", "--output", "dist/feature"];
    assert_eq!(build(&feature_build)?, ExitCode::SUCCESS);
    let output = std::process::Command::new(project.join("dist/feature/blog")).output()?;
    assert!(output.status.success());
    assert_eq!(output.stdout, b"with feature");
    assert_eq!(fs::read(project.join("Cargo.lock"))?, lock);

    fs::write(project.join("src/main.rs"), "invalid Rust")?;
    let failed = ["--output", "dist/failed"];
    assert!(build(&failed).is_err());
    assert!(!project.join("dist/failed").exists());
    Ok(())
}

#[test]
fn build_help_names_the_cli_and_documents_all_public_options() -> anyhow::Result<()> {
    let Err(error) = Cli::try_parse_from(["openrust", "build", "--help"]) else {
        anyhow::bail!("帮助参数应提前退出");
    };
    assert_eq!(error.kind(), clap::error::ErrorKind::DisplayHelp);
    let help = error.to_string();
    for expected in [
        "openrust build",
        "--project-dir",
        "--docker",
        "--env",
        "--features",
        "--platform",
        "--output",
    ] {
        assert!(help.contains(expected), "{help}");
    }
    assert!(parse_build(&["--unexpected"]).is_err());
    Ok(())
}

#[cfg(unix)]
#[test]
fn exported_binary_must_match_platform_and_artifacts_have_only_one_elf() -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    let temporary = tempfile::tempdir()?;
    let root = temporary.path();
    for directory in ["public", "resources", "storage", "bootstrap/cache"] {
        fs::create_dir_all(root.join(directory))?;
    }
    let binary = root.join("application");
    let mut header = [0_u8; 64];
    header[..4].copy_from_slice(b"\x7fELF");
    header[4] = 2;
    header[5] = 1;
    header[6] = 1;
    header[16] = 3;
    header[18] = 183;
    header[52] = 64;
    fs::write(&binary, header)?;
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o755))?;
    validate_binary(&binary, "linux/arm64")?;
    assert!(validate_binary(&binary, "linux/amd64").is_err());
    assert!(validate_release(root, "application", "linux/arm64").is_err());
    let launcher = root.join("artisan");
    fs::write(&launcher, "#!/bin/sh\n")?;
    fs::write(root.join(".env.example"), "APP_KEY=\n")?;
    fs::set_permissions(&launcher, fs::Permissions::from_mode(0o644))?;
    assert!(validate_release(root, "application", "linux/arm64").is_err());
    fs::set_permissions(&launcher, fs::Permissions::from_mode(0o755))?;
    validate_release(root, "application", "linux/arm64")?;
    let data = root.join("storage/data");
    fs::write(&data, "old data")?;
    assert!(validate_release(root, "application", "linux/arm64").is_err());
    fs::remove_file(data)?;
    let hidden_database = root.join("resources/opaque");
    fs::write(&hidden_database, b"SQLite format 3\0")?;
    assert!(validate_release(root, "application", "linux/arm64").is_err());
    fs::remove_file(hidden_database)?;
    fs::create_dir(root.join("bin"))?;
    fs::rename(&binary, root.join("bin/application"))?;
    assert!(validate_release(root, "application", "linux/arm64").is_err());
    fs::rename(root.join("bin/application"), &binary)?;
    fs::remove_dir(root.join("bin"))?;
    fs::write(root.join("resources/another"), header)?;
    assert_eq!(validate_entries(root, Some("application"))?, 2);
    assert!(validate_release(root, "application", "linux/arm64").is_err());
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o644))?;
    assert!(validate_binary(&binary, "linux/arm64").is_err());
    Ok(())
}
