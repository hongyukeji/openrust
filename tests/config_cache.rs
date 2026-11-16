use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicUsize, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

struct AppRoot(PathBuf);

impl AppRoot {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("openrust-config-{}-{now}-{id}", std::process::id()));
        fs::create_dir(&root).unwrap();
        Self(root)
    }

    fn run(&self, arguments: &[&str], override_path: Option<&Path>) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_main"));
        command
            .arg("artisan")
            .args(arguments)
            .env("APP_BASE_PATH", &self.0)
            .env_remove("APP_ENV")
            .env_remove("APP_NAME")
            .env_remove("APP_CONFIG_CACHE");
        if let Some(path) = override_path {
            command.env("APP_CONFIG_CACHE", path);
        }
        command.output().unwrap()
    }

    fn cache(&self) -> PathBuf {
        self.0.join("storage/framework/config.json")
    }
}

impl Drop for AppRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn success(result: Output) -> String {
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    String::from_utf8(result.stdout).unwrap()
}

fn failed(result: Output) -> String {
    assert_eq!(result.status.code(), Some(1));
    String::from_utf8(result.stderr).unwrap()
}

#[test]
fn artisan_freezes_configuration_until_cleared_and_bypasses_dotenv_on_cache_hit() {
    let root = AppRoot::new();
    let base = root.0.join(".env");
    fs::write(&base, "APP_NAME=Base\nAPP_ENV=local\n").unwrap();
    fs::write(
        root.0.join(".env.staging"),
        "APP_NAME=Staging\nAPP_ENV=staging\n",
    )
    .unwrap();

    let cached = success(root.run(&["config:cache", "--env=staging"], None));
    assert!(cached.contains("Configuration cached successfully"));
    let cache: serde_json::Value =
        serde_json::from_slice(&fs::read(root.cache()).unwrap()).unwrap();
    assert_eq!(cache["app"]["name"], "Staging");
    assert!(!root.0.join("bootstrap/cache").exists());

    fs::write(&base, "APP_ENV=\"unterminated\n").unwrap();
    assert!(success(root.run(&["env"], None)).contains("staging"));
    success(root.run(&["config:clear"], None));
    assert!(!root.cache().exists());
    assert!(failed(root.run(&["env"], None)).contains("invalid environment file syntax"));

    fs::write(&base, "APP_ENV=production\nAPP_NAME=Updated\n").unwrap();
    success(root.run(&["config:clear"], None));
    assert!(success(root.run(&["env"], None)).contains("production"));
}

#[test]
fn artisan_honors_the_selected_cache_path_without_removing_other_caches() {
    let root = AppRoot::new();
    let base = root.0.join(".env");
    let relative = Path::new("custom/config.json");
    let fallback = root.0.join(relative);
    let override_path = root.0.join("other/settings.json");
    fs::write(
        &base,
        "APP_CONFIG_CACHE=custom/config.json\nAPP_ENV=custom\n",
    )
    .unwrap();

    success(root.run(&["config:cache"], None));
    assert!(fallback.is_file());
    assert!(!root.cache().exists());
    assert!(success(root.run(&["env"], Some(relative))).contains("custom"));
    success(root.run(&["config:cache"], Some(&override_path)));
    assert!(override_path.is_file());
    success(root.run(&["config:clear"], Some(&override_path)));
    assert!(!override_path.exists());
    assert!(fallback.is_file());

    fs::write(
        &base,
        "APP_CONFIG_CACHE=custom/config.json\nAPP_ENV=updated\n",
    )
    .unwrap();
    assert!(success(root.run(&["env"], None)).contains("custom"));
    success(root.run(&["config:clear"], None));
    assert!(success(root.run(&["env"], None)).contains("updated"));
}

#[test]
fn artisan_does_not_publish_a_partial_snapshot_or_silently_clear_a_corrupt_cache() {
    let root = AppRoot::new();
    let source = root.0.join(".env");
    fs::write(&source, "APP_ENV=local\n").unwrap();
    success(root.run(&["config:cache"], None));
    fs::write(&source, "APP_ENV=\"unterminated\n").unwrap();

    assert!(
        failed(root.run(&["config:cache"], None))
            .contains("Unable to build the fresh configuration cache")
    );
    assert!(!root.cache().exists());
    let parent = root.cache().parent().unwrap().to_owned();
    assert!(fs::read_dir(parent).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".config-cache-")
    }));

    let corrupt = "redact-this-invalid-payload";
    fs::write(root.cache(), corrupt).unwrap();
    let error = failed(root.run(&["config:clear"], None));
    assert!(error.contains("invalid configuration cache"));
    assert!(!error.contains(corrupt));
    assert_eq!(fs::read_to_string(root.cache()).unwrap(), corrupt);
}
