use openrust::{
    config::{ConfigError, Environment, EnvironmentFile, EnvironmentFileError, Value},
    foundation::Application,
};
use std::{collections::BTreeMap, fs, path::Path, process::Command};

struct TestRoot(std::path::PathBuf);
impl TestRoot {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "environment-file-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}
impl Drop for TestRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn configuration(
    _: &Path,
    environment: &Environment,
) -> Result<BTreeMap<String, Value>, ConfigError> {
    Ok(BTreeMap::from([(
        "app".into(),
        serde_json::json!({
            "name": environment.get_utf8("EDITOR_APP_NAME")?.unwrap_or("initial")
        }),
    )]))
}

#[tokio::test]
async fn application_binding_snapshot_and_next_boot_are_independent() {
    let dir = TestRoot::new();
    let path = dir.path().join(".env");
    fs::write(&path, "\u{feff}EDITOR_APP_NAME=first\n").unwrap();
    let create = || {
        Application::configure(dir.path().to_owned())
            .with_config(configuration)
            .create()
            .unwrap()
    };
    let mut app = create();
    assert!(matches!(
        EnvironmentFile::for_application(&app),
        Err(EnvironmentFileError::Unbound)
    ));
    app.boot().await.unwrap();
    let file = EnvironmentFile::for_application(&app).unwrap();
    assert_eq!(file.path(), path.canonicalize().unwrap());
    let config = app
        .get::<dyn openrust::contracts::config::Repository>("config")
        .await
        .unwrap();
    let snapshot = app
        .get::<Environment>(openrust::container::ServiceKey::typed::<Environment>())
        .await
        .unwrap();
    file.set("EDITOR_APP_NAME", "second").unwrap();
    assert_eq!(snapshot.get_utf8("EDITOR_APP_NAME").unwrap(), Some("first"));
    assert_eq!(config.get("app.name"), Some(Value::from("first")));
    config.set("app.name", Value::from("runtime")).unwrap();
    assert_eq!(
        file.get("EDITOR_APP_NAME").unwrap().as_deref(),
        Some("second")
    );
    let mut next = create();
    next.boot().await.unwrap();
    assert_eq!(
        next.get::<dyn openrust::contracts::config::Repository>("config")
            .await
            .unwrap()
            .get("app.name"),
        Some(Value::from("second"))
    );
}

fn artisan(root: &Path, args: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_main"))
        .arg("artisan")
        .args(args)
        .env("APP_BASE_PATH", root)
        .env_remove("APP_NAME")
        .env_remove("APP_ENV")
        .env_remove("APP_CONFIG_CACHE")
        .env("APP_DISABLED_RUNNERS", "queue,schedule,grpc,reverb")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn real_artisan_selected_file_and_config_cache_require_explicit_refresh() {
    let dir = TestRoot::new();
    let root = dir.path();
    let file = EnvironmentFile::create(root.join(".env"), "APP_NAME=Original\nAPP_KEY=\n").unwrap();
    EnvironmentFile::create(root.join(".env.editor"), "APP_NAME=Selected\nAPP_KEY=\n").unwrap();
    assert!(artisan(root, &["config:show", "app.name"]).contains("Original"));
    artisan(root, &["config:cache"]);
    file.set("APP_NAME", "Changed").unwrap();
    assert!(artisan(root, &["config:show", "app.name"]).contains("Original"));
    artisan(root, &["config:clear"]);
    assert!(artisan(root, &["config:show", "app.name"]).contains("Changed"));
    artisan(
        root,
        &[
            "key:generate",
            "--env=editor",
            "--force",
            "--no-interaction",
        ],
    );
    assert_eq!(file.get("APP_KEY").unwrap().as_deref(), Some(""));
    let selected = EnvironmentFile::open(root.join(".env.editor")).unwrap();
    assert!(
        selected
            .get("APP_KEY")
            .unwrap()
            .unwrap()
            .starts_with("base64:")
    );
    assert!(artisan(root, &["config:show", "app.name", "--env=editor"]).contains("Selected"));
}
