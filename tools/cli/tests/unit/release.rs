#[cfg(unix)]
use super::{INCOMPLETE, validate_binary, validate_release, validate_tree};
use super::{
    application_name, bake_path, destination, freeze_plan, host_platform, path_text, publish,
    read_manifest, reject_existing,
};
#[cfg(unix)]
use anyhow::Context as _;
use anyhow::bail;
use std::fs;
use std::path::{Path, PathBuf};

fn fixture() -> anyhow::Result<(tempfile::TempDir, PathBuf)> {
    let temporary = tempfile::tempdir()?;
    let project = temporary.path().join("app with spaces,comma");
    fs::create_dir(&project)?;
    fs::write(
        project.join("Cargo.toml"),
        "[package]\nname = \"artisan\"\n",
    )?;
    fs::write(project.join("Dockerfile"), "FROM scratch AS artifacts\n")?;
    Ok((temporary, project.canonicalize()?))
}

#[test]
fn source_launcher_and_generated_launcher_route_build_to_cli() {
    for source in [
        include_str!("../../../../artisan").to_owned(),
        crate::deployment::artisan_source("artisan"),
    ] {
        assert_eq!(
            source
                .matches("build|package:discover|package:list)")
                .count(),
            2
        );
        assert!(source.contains("--project-dir \"$application\""));
        assert!(source.contains("exec \"$application\" \"$@\""));
    }
}

#[test]
fn platform_selection_maps_cpu_without_choosing_host_os() -> anyhow::Result<()> {
    assert_eq!(host_platform("x86_64")?, "linux/amd64");
    assert_eq!(host_platform("aarch64")?, "linux/arm64");
    assert!(host_platform("riscv64").is_err());
    Ok(())
}

#[test]
fn package_name_and_output_are_separate_from_cargo_binary_target() -> anyhow::Result<()> {
    let (_temporary, project) = fixture()?;
    let manifest = read_manifest(&project)?;
    let name = application_name(&manifest)?;
    assert_eq!(name, "artisan");
    assert_eq!(
        destination(&project, name, "linux/arm64", None)?,
        project.join("dist/artisan-linux-arm64")
    );
    assert!(destination(&project, name, "linux/amd64", Some(Path::new("../escape"))).is_err());
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
        assert!(publish(&source, &output).is_err());
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
        assert!(publish(&source, &output).is_err());
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
    fs::create_dir_all(source.join("bin"))?;
    fs::write(source.join("bin/artisan"), "application")?;
    fs::set_permissions(
        source.join("bin/artisan"),
        fs::Permissions::from_mode(0o4751),
    )?;
    let output = temporary.path().join("new-release");
    publish(&source, &output)?;
    assert_eq!(
        fs::read_to_string(output.join("bin/artisan"))?,
        "application"
    );
    assert_eq!(
        fs::metadata(output.join("bin/artisan"))?
            .permissions()
            .mode()
            & 0o7777,
        0o751
    );
    assert!(!output.join(INCOMPLETE).exists());
    Ok(())
}

#[cfg(unix)]
#[test]
fn copy_failure_is_incomplete_and_never_follows_exported_symlink() -> anyhow::Result<()> {
    use std::os::unix::fs::symlink;
    let temporary = tempfile::tempdir()?;
    let source = temporary.path().join("source");
    fs::create_dir(&source)?;
    let private = temporary.path().join("private");
    fs::write(&private, "private sentinel")?;
    symlink(&private, source.join("link"))?;
    assert!(validate_tree(&source).is_err());
    let output = temporary.path().join("partial");
    let error = publish(&source, &output)
        .err()
        .context("expected incomplete error")?;
    assert!(error.to_string().starts_with("build_publish_incomplete:"));
    assert_eq!(fs::read_to_string(output.join(INCOMPLETE))?, "incomplete\n");
    assert!(!output.join("link").exists());
    assert_eq!(fs::read_to_string(private)?, "private sentinel");
    Ok(())
}

#[test]
fn public_build_parser_accepts_only_one_linux_platform() -> anyhow::Result<()> {
    use clap::Parser as _;
    let parsed = crate::Cli::try_parse_from([
        "openrust",
        "build",
        "--platform",
        "linux/arm64",
        "--output",
        "/tmp/with spaces,comma",
    ])?;
    let Some(crate::Builtin::Build(options)) = parsed.command else {
        bail!("build did not select its own command");
    };
    assert_eq!(options.platform.as_deref(), Some("linux/arm64"));
    assert_eq!(
        options.output.as_deref(),
        Some(Path::new("/tmp/with spaces,comma"))
    );
    assert!(
        crate::Cli::try_parse_from(["openrust", "build", "--platform", "linux/amd64,linux/arm64"])
            .is_err()
    );
    assert!(
        crate::Cli::try_parse_from(["openrust", "build", "--platform", "darwin/arm64"]).is_err()
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn exported_binary_must_match_platform_and_artifacts_have_only_one_elf() -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    let temporary = tempfile::tempdir()?;
    let root = temporary.path();
    for directory in ["bin", "public", "resources", "storage", "bootstrap/cache"] {
        fs::create_dir_all(root.join(directory))?;
    }
    let binary = root.join("bin/application");
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
    validate_release(root, "application", "linux/arm64")?;
    let data = root.join("storage/data");
    fs::write(&data, "old data")?;
    assert!(validate_release(root, "application", "linux/arm64").is_err());
    fs::remove_file(data)?;
    let hidden_database = root.join("resources/opaque");
    fs::write(&hidden_database, b"SQLite format 3\0")?;
    assert!(validate_release(root, "application", "linux/arm64").is_err());
    fs::remove_file(hidden_database)?;
    fs::write(root.join("bin/another"), header)?;
    assert_eq!(validate_tree(root)?, 2);
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o644))?;
    assert!(validate_binary(&binary, "linux/arm64").is_err());
    Ok(())
}
