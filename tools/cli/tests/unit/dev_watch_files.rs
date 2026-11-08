use super::{Change, Files};
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

struct Fixture {
    root: PathBuf,
    app: PathBuf,
    workspace: PathBuf,
    dependency: PathBuf,
    dependency_workspace: PathBuf,
    unrelated: PathBuf,
    target: PathBuf,
}

impl Fixture {
    fn new() -> anyhow::Result<Self> {
        static ID: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "openrust-watch-files-{}-{}",
            std::process::id(),
            ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root)?;
        let root = root.canonicalize()?;
        let workspace = root.join("workspace");
        let dependency_workspace = root.join("source [copy]");
        let fixture = Self {
            root,
            app: workspace.join("tmp/app"),
            dependency: dependency_workspace.join("crates/library"),
            unrelated: workspace.join("unrelated"),
            target: workspace.join("custom-build"),
            workspace,
            dependency_workspace,
        };
        for directory in [
            &fixture.app,
            &fixture.dependency,
            &fixture.unrelated,
            &fixture.workspace.join(".cargo"),
        ] {
            fs::create_dir_all(directory)?;
        }
        for package in [&fixture.app, &fixture.dependency, &fixture.unrelated] {
            fs::write(
                package.join("Cargo.toml"),
                "[package]\nname = \"fixture\"\n",
            )?;
        }
        for workspace in [&fixture.workspace, &fixture.dependency_workspace] {
            fs::write(workspace.join("Cargo.toml"), "[workspace]\n")?;
        }
        Ok(fixture)
    }

    fn metadata(&self) -> Value {
        json!({
            "target_directory": self.target,
            "packages": [
                {"id": "app", "manifest_path": self.app.join("Cargo.toml"), "source": null},
                {"id": "local", "manifest_path": self.dependency.join("Cargo.toml"), "source": null},
                {"id": "unrelated", "manifest_path": self.unrelated.join("Cargo.toml"), "source": null},
                {"id": "registry", "manifest_path": "/unwatched/registry/Cargo.toml", "source": "registry+https://example.test"}
            ],
            "resolve": {"nodes": [
                {"id": "app", "dependencies": ["registry"]},
                {"id": "registry", "dependencies": ["local"]},
                {"id": "local", "dependencies": []},
                {"id": "unrelated", "dependencies": []}
            ]}
        })
    }

    fn files(&self, environment: Option<&str>) -> anyhow::Result<Files> {
        Files::new(
            &self.metadata(),
            &self.app,
            &self.workspace,
            "OPENRUST_WATCH_FIXTURE_",
            environment,
        )
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn event(paths: &[&Path]) -> String {
    let mut tags = vec![
        json!({"kind": "source", "source": "filesystem"}),
        json!({"kind": "fs", "simple": "modify"}),
    ];
    tags.extend(
        paths
            .iter()
            .map(|path| json!({"kind": "path", "absolute": path})),
    );
    json!({"tags": tags}).to_string()
}

#[test]
fn watches_app_and_reachable_local_dependency_without_recursing_workspace() -> anyhow::Result<()> {
    let fixture = Fixture::new()?;
    let mut files = fixture.files(None)?;
    let args = files.watch_args();
    let has_watch = |flag, directory: &Path| {
        args.windows(2)
            .any(|pair| pair[0] == flag && pair[1] == directory.as_os_str())
    };
    assert!(has_watch("--watch", &fixture.app));
    assert!(has_watch("--watch", &fixture.dependency));
    assert!(!has_watch("--watch", &fixture.unrelated));
    assert!(!has_watch("--watch", &fixture.workspace));
    assert!(has_watch("--watch-non-recursive", &fixture.workspace));
    assert!(has_watch(
        "--watch-non-recursive",
        &fixture.workspace.join(".cargo")
    ));
    assert!(has_watch(
        "--watch-non-recursive",
        &fixture.dependency_workspace
    ));
    for (path, expected) in [
        (fixture.app.join("routes/web.rs"), Change::Source),
        (
            fixture.app.join("modules/news/resources/a.blade.php"),
            Change::Source,
        ),
        (fixture.dependency.join("README.md"), Change::Source),
        (
            fixture.dependency_workspace.join("Cargo.toml"),
            Change::Manifest,
        ),
        (
            fixture.workspace.join(".cargo/config.toml"),
            Change::Manifest,
        ),
        (fixture.workspace.join("Cargo.lock"), Change::Manifest),
        (fixture.unrelated.join("src/lib.rs"), Change::None),
    ] {
        assert_eq!(
            files.classify(&event(&[&path]))?,
            expected,
            "{}",
            path.display()
        );
    }
    Ok(())
}

#[test]
fn output_paths_do_not_hide_sources_under_a_parent_tmp_or_win_over_manifest() -> anyhow::Result<()>
{
    let fixture = Fixture::new()?;
    let mut files = fixture.files(None)?;
    for path in [
        fixture.target.join("debug/generated.rs"),
        fixture.app.join("target/generated.rs"),
        fixture.app.join("storage/logs/application.log"),
        fixture.app.join("bootstrap/cache/config.json"),
        fixture.app.join("database/database.sqlite-wal"),
        fixture.app.join("database/.openrust-migration-run.lock"),
    ] {
        assert_eq!(files.classify(&event(&[&path]))?, Change::None);
    }
    let source = fixture.app.join("config/app.toml");
    let manifest = fixture.app.join("Cargo.toml");
    assert_eq!(
        files.classify(&event(&[&source, &manifest]))?,
        Change::Manifest
    );
    assert_eq!(files.classify(&event(&[&source]))?, Change::Source);
    assert_eq!(files.classify(r#"{"tags":[]}"#)?, Change::None);
    assert!(files.classify("not json").is_err());
    Ok(())
}

#[test]
fn dotenv_changes_only_restart_when_the_selected_file_changes() -> anyhow::Result<()> {
    let fixture = Fixture::new()?;
    let plain = fixture.app.join(".env");
    let chosen = fixture.app.join(".env.development");
    let other = fixture.app.join(".env.unused");
    fs::write(&plain, "FIXTURE=true\n")?;
    let mut files = fixture.files(Some("development"))?;
    assert_eq!(files.classify(&event(&[&other]))?, Change::None);
    assert_eq!(files.classify(&event(&[&plain]))?, Change::Source);
    fs::write(&chosen, "FIXTURE=false\n")?;
    assert_eq!(files.classify(&event(&[&chosen]))?, Change::Source);
    assert_eq!(files.classify(&event(&[&plain]))?, Change::None);
    fs::remove_file(&chosen)?;
    assert_eq!(files.classify(&event(&[&chosen]))?, Change::Source);
    assert_eq!(files.classify(&event(&[&plain]))?, Change::Source);
    Ok(())
}
