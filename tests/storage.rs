#![cfg(unix)]

use std::{fs, os::unix::fs::symlink, path::PathBuf, process::Command};

struct Sandbox(PathBuf);

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn artisan_storage_commands_create_repair_and_remove_only_configured_links() {
    let path =
        std::env::temp_dir().join(format!("openrust-storage-artisan-{}", std::process::id()));
    fs::create_dir(&path).unwrap();
    let sandbox = Sandbox(path.canonicalize().unwrap());
    let public = sandbox.0.join("website");
    let storage = sandbox.0.join("files");
    fs::create_dir(&public).unwrap();
    fs::create_dir(&storage).unwrap();
    let link = public.join("storage");
    let run = |arguments: &[&str]| {
        let output = Command::new(env!("CARGO_BIN_EXE_main"))
            .arg("artisan")
            .args(arguments)
            .env("APP_BASE_PATH", &sandbox.0)
            .env("PUBLIC_PATH", "website")
            .env("PUBLIC_STORAGE_ROOT", "files")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        output
    };

    let output = run(&["storage:link"]);
    assert!(String::from_utf8_lossy(&output.stdout).contains("has been connected to"));
    assert_eq!(fs::read_link(&link).unwrap(), storage);
    let output = run(&["storage:link", "--relative"]);
    assert!(String::from_utf8_lossy(&output.stderr).contains("already exists"));
    assert_eq!(fs::read_link(&link).unwrap(), storage);
    run(&["storage:link", "--relative", "--force"]);
    assert_eq!(fs::read_link(&link).unwrap(), PathBuf::from("../files"));

    let output = run(&["storage:unlink"]);
    assert!(String::from_utf8_lossy(&output.stdout).contains("has been deleted"));
    assert!(fs::symlink_metadata(&link).is_err());
    assert!(storage.is_dir());
    symlink("missing", &link).unwrap();
    run(&["storage:unlink"]);
    assert_eq!(fs::read_link(&link).unwrap(), PathBuf::from("missing"));
    run(&["storage:link"]);
    assert_eq!(fs::read_link(&link).unwrap(), storage);

    fs::remove_file(&link).unwrap();
    fs::write(&link, "user file").unwrap();
    run(&["storage:link", "--force"]);
    run(&["storage:unlink"]);
    assert_eq!(fs::read_to_string(&link).unwrap(), "user file");
}
