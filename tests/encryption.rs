use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
};

struct Sandbox(PathBuf);

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn artisan_generates_keys_and_protects_the_selected_environment_file() {
    let path = std::env::temp_dir().join(format!("openrust-key-test-{}", std::process::id()));
    fs::create_dir(&path).unwrap();
    let root = Sandbox(path);
    let default = root.0.join(".env");
    let selected = root.0.join(".env.testing");
    let template = "# APP_KEY=ignored\r\nAPP_NAME=Demo\r\nAPP_KEY=\r\nAPP_PREVIOUS_KEYS=old\r\n";
    fs::write(&default, template).unwrap();
    fs::write(&selected, template).unwrap();
    let run = |arguments: &[&str], environment: &str, external_key: Option<&str>| -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_main"));
        command
            .args(["artisan", "key:generate"])
            .args(arguments)
            .env("APP_BASE_PATH", &root.0)
            .env("APP_ENV", environment)
            .env_remove("APP_KEY");
        if let Some(key) = external_key {
            command.env("APP_KEY", key);
        }
        command.output().unwrap()
    };

    let preview = run(&["--show", "--no-ansi"], "production", None);
    assert!(preview.status.success());
    let shown = String::from_utf8(preview.stdout).unwrap();
    assert!(shown.trim().starts_with("base64:"));
    assert_eq!(shown.trim().len(), 51);
    assert_eq!(fs::read_to_string(&default).unwrap(), template);

    let result = run(&["--env=testing"], "local", None);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(fs::read_to_string(&default).unwrap(), template);
    let selected_text = fs::read_to_string(&selected).unwrap();
    assert!(selected_text.contains("APP_KEY=base64:"));
    assert!(selected_text.contains("\r\nAPP_PREVIOUS_KEYS=old\r\n"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        assert_eq!(
            fs::metadata(&selected).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    fs::write(&default, &selected_text).unwrap();
    let declined = run(&["--no-interaction"], "production", None);
    assert!(declined.status.success());
    assert!(String::from_utf8_lossy(&declined.stderr).contains("cancelled"));
    assert_eq!(fs::read_to_string(&default).unwrap(), selected_text);

    let forced = run(&["--force"], "production", None);
    assert!(forced.status.success());
    let changed = fs::read_to_string(&default).unwrap();
    assert_ne!(changed, selected_text);
    assert!(changed.contains("\r\nAPP_PREVIOUS_KEYS=old\r\n"));
    assert!(!String::from_utf8_lossy(&forced.stdout).contains("base64:"));
    let external = run(&["--force"], "local", Some("external-value"));
    assert!(!external.status.success());
    assert_eq!(fs::read_to_string(&default).unwrap(), changed);

    fs::write(&default, "APP_NAME=Demo\n").unwrap();
    let missing = run(&[], "local", None);
    assert!(!missing.status.success());
    assert_eq!(fs::read_to_string(&default).unwrap(), "APP_NAME=Demo\n");
    fs::write(&default, "APP_KEY=\nAPP_KEY=\n").unwrap();
    assert!(!run(&[], "local", None).status.success());
    assert_eq!(
        fs::read_to_string(&default).unwrap(),
        "APP_KEY=\nAPP_KEY=\n"
    );
    assert!(fs::read_dir(&root.0).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".key-generate-")
    }));
}
