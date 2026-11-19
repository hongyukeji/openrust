use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

struct TestProject(std::path::PathBuf);

impl TestProject {
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestProject {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn project() -> TestProject {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let sequence = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let name = format!("openrust-init-{}-{nonce}-{sequence}", std::process::id());
    let root = TestProject(std::env::temp_dir().join(name));
    fs::create_dir(root.path()).unwrap();
    for (name, content) in [
        (
            "Cargo.toml",
            "[package]\nname = \"fixture\"\nversion = \"0.0.0\"\n",
        ),
        ("bootstrap/app.rs", "custom bootstrap\n"),
        (
            "routes/mod.rs",
            "pub(crate) mod console;\npub(crate) mod web;\n",
        ),
        ("config/mod.rs", "custom config\n"),
        (
            ".env",
            "APP_ENV=local\nAPP_KEY=01234567890123456789012345678901\nAPI_PREFIX=api/admin\n",
        ),
    ] {
        let path = root.path().join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }
    root
}

fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_main"))
        .arg("artisan")
        .args(args)
        .env("APP_BASE_PATH", root)
        .output()
        .unwrap()
}

#[test]
fn init_plan_is_read_only() {
    let root = project();
    let result = run(
        root.path(),
        &[
            "init",
            "--dry-run",
            "--force",
            "--without-migration",
            "--no-interaction",
        ],
    );
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let output = String::from_utf8_lossy(&result.stdout);
    assert!(output.contains("API / Sanctum"));
    assert!(output.contains("Dry run: no files"));
    assert!(output.contains("broadcasting channels"));
    assert_eq!(
        fs::read_to_string(root.path().join(".env")).unwrap(),
        "APP_ENV=local\nAPP_KEY=01234567890123456789012345678901\nAPI_PREFIX=api/admin\n"
    );
    assert!(!root.path().join("tmp").exists());
    assert!(!root.path().join("routes/api.rs").exists());
    assert!(!root.path().join("database").exists());
}

#[test]
fn init_connection_failure_preserves_existing_configuration() {
    let root = project();
    let original = "APP_ENV=local\nDB_CONNECTION=mysql\nDB_HOST=127.0.0.1\nDB_PORT=1\nDB_DATABASE=missing\nDB_USERNAME=test\nAPP_KEY=01234567890123456789012345678901\n";
    fs::write(root.path().join(".env"), original).unwrap();
    let result = run(root.path(), &["init", "--no-interaction"]);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("Database connection"));
    assert_eq!(
        fs::read_to_string(root.path().join(".env")).unwrap(),
        original
    );
    assert!(!root.path().join("routes/api.rs").exists());
}

#[test]
fn init_validates_project_and_exposes_help() {
    let root = project();
    fs::write(root.path().join("Cargo.toml"), "[broken").unwrap();
    let result = run(root.path(), &["init", "--dry-run"]);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("Invalid Cargo.toml"));
    let help = run(root.path(), &["help", "init"]);
    assert!(help.status.success());
    let output = String::from_utf8_lossy(&help.stdout);
    for flag in ["--force", "--dry-run", "--without-migration"] {
        assert!(output.contains(flag));
    }
}

#[test]
fn individual_installer_rejects_concurrent_source_mutation() {
    let root = project();
    fs::create_dir(root.path().join("tmp")).unwrap();
    let lock = fs::File::create(root.path().join("tmp/artisan-source.lock")).unwrap();
    lock.lock().unwrap();
    let result = run(root.path(), &["install:grpc"]);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("Cannot acquire installation lock"));
    assert!(!root.path().join("routes/grpc.rs").exists());
    drop(lock);
    fs::write(
        root.path().join("bootstrap/app.rs"),
        ".withMiddleware(|_| {})\n",
    )
    .unwrap();
    let retry = run(root.path(), &["install:grpc"]);
    assert!(
        retry.status.success(),
        "{}",
        String::from_utf8_lossy(&retry.stderr)
    );
    assert!(root.path().join("routes/grpc.rs").exists());
}

#[test]
fn production_init_requires_explicit_force_and_removed_alias_is_unknown() {
    let root = project();
    fs::write(
        root.path().join(".env"),
        "APP_ENV=production\nDB_CONNECTION=mysql\nDB_HOST=127.0.0.1\nDB_PORT=1\n",
    )
    .unwrap();
    let result = run(root.path(), &["init", "--no-interaction"]);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("production migrations require"));
    assert!(!root.path().join("tmp").exists());
    let removed = run(root.path(), &["install:full", "--dry-run"]);
    assert!(!removed.status.success());
}

#[test]
fn invalid_existing_key_is_rejected_without_rotation() {
    let root = project();
    let text = "APP_ENV=local\nAPP_KEY=do-not-rotate-me\n";
    fs::write(root.path().join(".env"), text).unwrap();
    let result = run(root.path(), &["init", "--no-interaction"]);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("Existing keys were preserved"));
    assert_eq!(fs::read_to_string(root.path().join(".env")).unwrap(), text);
    assert!(!root.path().join("tmp").exists());
}

#[test]
fn sms_credentials_are_required_without_writing_configuration() {
    for (driver, key) in [
        ("aliyun", "SMS_ALIYUN_ACCESS_KEY_ID"),
        ("tencent", "SMS_TENCENT_SECRET_ID"),
        ("twilio", "SMS_TWILIO_ACCOUNT_SID"),
        ("vonage", "VONAGE_KEY"),
        ("smsbao", "SMS_SMSBAO_USERNAME"),
        ("yunpian", "SMS_YUNPIAN_API_KEY"),
    ] {
        let root = project();
        let original = format!("APP_ENV=local\nSMS_DRIVER={driver}\n");
        fs::write(root.path().join(".env"), &original).unwrap();
        let result = run(root.path(), &["init", "--no-interaction"]);
        assert!(!result.status.success());
        assert!(String::from_utf8_lossy(&result.stderr).contains(key));
        assert_eq!(
            fs::read_to_string(root.path().join(".env")).unwrap(),
            original
        );
        assert!(!root.path().join("tmp").exists());
    }
}

#[test]
fn init_reads_selected_environment_without_touching_the_default_file() {
    let root = project();
    let default = fs::read_to_string(root.path().join(".env")).unwrap();
    let selected = "APP_ENV=local\nDB_CONNECTION=pgsql\nAPP_KEY=01234567890123456789012345678901\n";
    fs::write(root.path().join(".env.acceptance"), selected).unwrap();
    let result = run(root.path(), &["init", "--dry-run", "--env=acceptance"]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(String::from_utf8_lossy(&result.stdout).contains("database=pgsql"));
    assert!(String::from_utf8_lossy(&result.stdout).contains(".env.acceptance:"));
    assert_eq!(
        fs::read_to_string(root.path().join(".env")).unwrap(),
        default
    );
    assert_eq!(
        fs::read_to_string(root.path().join(".env.acceptance")).unwrap(),
        selected
    );
    assert!(!root.path().join("tmp").exists());
}

#[test]
fn init_selects_extensions_and_adds_reverb_channel_dependency() {
    let root = project();
    let result = run(
        root.path(),
        &["init", "--dry-run", "--extensions=grpc,grpc"],
    );
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let output = String::from_utf8_lossy(&result.stdout);
    assert!(output.contains("Selected extensions: grpc."));
    assert!(output.contains("routes/grpc.rs:"));
    assert!(!output.contains("routes/api.rs:"));
    assert!(!output.contains("routes/channels.rs:"));
    let result = run(root.path(), &["init", "--dry-run", "--extensions=reverb"]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let output = String::from_utf8_lossy(&result.stdout);
    assert!(output.contains("Selected extensions: reverb,broadcasting."));
    assert!(output.contains("routes/channels.rs:"));
    assert!(!output.contains("routes/api.rs:"));
    assert!(!root.path().join("tmp").exists());
}

#[test]
fn init_rejects_unready_or_unknown_extensions_before_mutating_files() {
    let root = project();
    let original = fs::read(root.path().join(".env")).unwrap();
    for choice in [
        "all",
        "passport",
        "api,fortify",
        "passkeys",
        "socialite",
        "precognition",
        "oidc",
        "sso",
        "typo",
        "api,",
        "all,api",
        "",
    ] {
        let result = run(
            root.path(),
            &[
                "init",
                "--no-interaction",
                &format!("--extensions={choice}"),
            ],
        );
        assert!(!result.status.success(), "unexpected success for {choice}");
        let error = String::from_utf8_lossy(&result.stderr);
        assert!(
            error.contains("not ready")
                || error.contains("Unknown initialization extension")
                || error.contains("on its own"),
            "{error}"
        );
        assert_eq!(fs::read(root.path().join(".env")).unwrap(), original);
        assert!(!root.path().join("tmp").exists());
        assert!(!root.path().join("routes/api.rs").exists());
    }
}

#[test]
fn init_requires_reverb_selection_for_active_reverb_configuration() {
    let root = project();
    for setting in ["REVERB_ENABLED=true", "BROADCAST_CONNECTION=reverb"] {
        let original = format!("APP_ENV=local\n{setting}\n");
        fs::write(root.path().join(".env"), &original).unwrap();
        let result = run(root.path(), &["init", "--dry-run", "--extensions=api"]);
        assert!(!result.status.success());
        assert!(
            String::from_utf8_lossy(&result.stderr)
                .contains("Reverb is configured but was not selected")
        );
        assert_eq!(
            fs::read_to_string(root.path().join(".env")).unwrap(),
            original
        );
        assert!(!root.path().join("tmp").exists());
    }
}
