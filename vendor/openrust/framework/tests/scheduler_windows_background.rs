#![cfg(windows)]

use std::{path::PathBuf, process::Stdio, time::Duration};

use tokio::{process::Command, time::timeout};

#[path = "../src/console/scheduling/windows_background.rs"]
mod windows_background;

const ARGUMENT: &str = "space \"quote\" \\末尾\\";

#[tokio::test]
async fn parent_output_closes_while_the_background_process_continues() {
    let root = tempfile::Builder::new()
        .prefix("openrust schedule 空间 ")
        .tempdir()
        .unwrap();
    let output = timeout(
        Duration::from_secs(5),
        Command::new(std::env::current_exe().unwrap())
            .args(["--ignored", "--exact", "--nocapture", "parent_fixture"])
            .env("OPENRUST_TEST_SCHEDULE_BACKGROUND_ROOT", root.path())
            .env("app_base_path", "superseded")
            .env("OPENRUST_TEST_SCHEDULE_BACKGROUND_SENTINEL", "继承")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap()
            .wait_with_output(),
    )
    .await
    .expect("the child kept the parent's output pipes open")
    .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stdout).contains("parent stdout"));
    assert!(String::from_utf8_lossy(&output.stderr).contains("parent stderr"));

    for name in ["started", "completed"] {
        timeout(Duration::from_secs(5), async {
            while !root.path().join(name).exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("background process did not write {name}"));
        if name == "started" {
            assert!(!root.path().join("completed").exists());
            std::fs::write(root.path().join("release"), []).unwrap();
        }
    }
}

#[test]
#[ignore = "Spawned by the Windows background process test"]
fn parent_fixture() {
    let root = PathBuf::from(std::env::var_os("OPENRUST_TEST_SCHEDULE_BACKGROUND_ROOT").unwrap())
        .canonicalize()
        .unwrap();
    let args = [
        "--ignored",
        "--exact",
        "--nocapture",
        "child_fixture",
        ARGUMENT,
    ]
    .map(str::to_owned);
    windows_background::spawn(&std::env::current_exe().unwrap(), &root, &args).unwrap();
    println!("parent stdout");
    eprintln!("parent stderr");
}

#[test]
#[ignore = "Spawned by the Windows background process test"]
fn child_fixture() {
    let root = PathBuf::from(std::env::var_os("OPENRUST_TEST_SCHEDULE_BACKGROUND_ROOT").unwrap())
        .canonicalize()
        .unwrap();
    assert_eq!(
        std::env::current_dir().unwrap().canonicalize().unwrap(),
        root
    );
    assert_eq!(
        PathBuf::from(std::env::var_os("APP_BASE_PATH").unwrap()),
        root
    );
    assert_eq!(
        std::env::var("OPENRUST_TEST_SCHEDULE_BACKGROUND_SENTINEL").unwrap(),
        "继承"
    );
    assert_eq!(std::env::args().next_back().unwrap(), ARGUMENT);
    std::fs::write(root.join("started"), []).unwrap();
    let start = std::time::Instant::now();
    while !root.join("release").exists() {
        if !root.is_dir() || start.elapsed() >= Duration::from_secs(15) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    std::fs::write(root.join("completed"), []).unwrap();
}
