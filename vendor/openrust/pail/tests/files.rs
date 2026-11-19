use openrust::log::{Level, LogContext};
use openrust_pail::{File, Files};
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    fs,
    path::PathBuf,
    process::Command,
    time::{Duration, SystemTime},
};

struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("openrust-pail-files-{:032x}", fastrand::u128(..)));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn sessions_preserve_records_expire_and_do_not_reappear_after_removal() {
    let root = Root::new();
    let files = Files::new(root.0.join("sessions"));
    assert!(files.all().unwrap().is_empty());
    let file = File::new(root.0.join("sessions/a.pail"));
    file.create().unwrap();
    assert_eq!(file.to_string(), file.path().display().to_string());
    assert_eq!(
        fs::read_to_string(root.0.join("sessions/.gitignore")).unwrap(),
        "*\n!.gitignore\n"
    );
    file.log(
        Level::Info,
        "line\nwith newline",
        &LogContext::from_iter([("id".into(), json!(7))]),
    )
    .unwrap();
    file.create().unwrap();
    let bytes = fs::read_to_string(file.path()).unwrap();
    assert_eq!(bytes.lines().count(), 1);
    let record: Value = serde_json::from_str(&bytes).unwrap();
    assert_eq!(record["message"], "line\nwith newline");
    assert_eq!(record["level"], 200);
    assert_eq!(record["level_name"], "INFO");
    assert_eq!(record["context"]["id"], 7);
    assert_eq!(record["channel"], "pail");
    assert!(record["datetime"].as_str().unwrap().ends_with("+00:00"));
    fs::write(root.0.join("sessions/ignored.log"), "ignored").unwrap();
    fs::write(root.0.join("sessions/.hidden.pail"), "ignored").unwrap();
    assert_eq!(files.all().unwrap().len(), 1);
    fs::File::options()
        .write(true)
        .open(file.path())
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(SystemTime::now() - Duration::from_secs(3601)))
        .unwrap();
    file.log(Level::Warning, "expired", &LogContext::new())
        .unwrap();
    assert!(!file.exists());
    file.log(Level::Error, "removed", &LogContext::new())
        .unwrap();
    file.destroy().unwrap();
    assert!(!file.exists());
    let directory = File::new(root.0.join("bad.pail"));
    fs::create_dir(directory.path()).unwrap();
    assert!(
        directory
            .log(Level::Info, "invalid target", &LogContext::new())
            .is_err()
    );
}

#[test]
fn file_append_process_child() {
    let Some(path) = std::env::var_os("PAIL_TEST_CHILD_PATH") else {
        return;
    };
    let child = std::env::var("PAIL_TEST_CHILD_ID").unwrap();
    let file = File::new(PathBuf::from(path));
    for index in 0..100 {
        file.log(
            Level::Info,
            &"payload".repeat(1024),
            &LogContext::from_iter([
                ("child".into(), json!(child)),
                ("index".into(), json!(index)),
            ]),
        )
        .unwrap();
    }
}

#[test]
fn independent_processes_append_complete_json_records() {
    let root = Root::new();
    let file = File::new(root.0.join("concurrent.pail"));
    file.create().unwrap();
    let mut children = (0..4)
        .map(|id| {
            Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "file_append_process_child", "--nocapture"])
                .env("PAIL_TEST_CHILD_PATH", file.path())
                .env("PAIL_TEST_CHILD_ID", id.to_string())
                .spawn()
                .unwrap()
        })
        .collect::<Vec<_>>();
    for child in &mut children {
        assert!(child.wait().unwrap().success());
    }
    let content = fs::read_to_string(file.path()).unwrap();
    let records = content
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(records.len(), 400);
    let keys = records
        .iter()
        .map(|record| {
            (
                record["context"]["child"].to_string(),
                record["context"]["index"].as_u64().unwrap(),
            )
        })
        .collect::<HashSet<_>>();
    assert_eq!(keys.len(), 400);
}

#[test]
fn capture_write_errors_propagate_after_the_original_log_record() {
    use openrust::{
        config::Repository,
        log::{LogError, LogManager},
    };
    use openrust_pail::Handler;
    use std::sync::{Arc, Weak};
    let root = Root::new();
    fs::create_dir(root.0.join("invalid.pail")).unwrap();
    let handler = Handler::new(Arc::new(Files::new(&root.0)), Weak::new());
    let manager = LogManager::new(Arc::new(Repository::new(Default::default())), &root.0);
    manager.listen(move |event| handler.log(event)).unwrap();
    assert!(matches!(
        manager.info("written before capture failure", &LogContext::new()),
        Err(LogError::WriteFailed(_))
    ));
    assert!(
        fs::read_to_string(root.0.join("storage/logs/app.log"))
            .unwrap()
            .contains("written before capture failure")
    );
}
