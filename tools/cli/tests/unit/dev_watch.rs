//! 验证 Cargo 产物选择与真实 Unix 进程组回收。

use super::{ChildGroup, build_executable};
use openrust::tokio;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::process::Command;

#[test]
fn selects_only_the_application_executable_from_cargo_output() -> anyhow::Result<()> {
    let message = |manifest: &str, target: &str, executable: &str| {
        json!({"reason":"compiler-artifact","manifest_path":manifest,
            "target":{"name":target},"executable":executable})
    };
    let output = format!(
        "build script stdout\n{}\n{}\n{}\n",
        message("/other/Cargo.toml", "server", "/other/target/server"),
        message("/app/Cargo.toml", "tool", "/app/target/tool"),
        message(
            "/app/Cargo.toml",
            "server",
            "/configured target/debug/server"
        ),
    );
    assert_eq!(
        build_executable(output.as_bytes(), Path::new("/app/Cargo.toml"), "server")?,
        PathBuf::from("/configured target/debug/server")
    );
    assert!(build_executable(output.as_bytes(), Path::new("/app/Cargo.toml"), "missing").is_err());
    Ok(())
}

#[tokio::test]
async fn stopping_a_child_group_allows_the_term_trap_to_run_and_reaps_the_child()
-> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let receipt = directory.path().join("signal");
    let mut command = Command::new("/bin/sh");
    command
        .args([
            "-ec",
            r#"trap 'printf terminated >> "$1"; exit 0' TERM
printf ready > "$1"
while :; do sleep 0.05; done"#,
            "fixture",
        ])
        .arg(&receipt);
    let mut process = ChildGroup::spawn(&mut command)?;
    tokio::time::timeout(Duration::from_secs(3), async {
        while !receipt.is_file() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await?;
    process.stop(Duration::from_secs(3)).await?;
    assert_eq!(std::fs::read_to_string(receipt)?, "readyterminated");
    assert!(
        process
            .child
            .try_wait()?
            .is_some_and(|status| status.success())
    );
    Ok(())
}

#[tokio::test]
async fn reaps_remaining_group_children_when_the_leader_has_exited() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let receipt = directory.path().join("child");
    for naturally in [false, true] {
        let mut command = Command::new("/bin/sh");
        command
            .args([
                "-ec",
                r#"trap 'exit 0' TERM
(trap '' TERM; exec sleep 90) &
printf '%s' "$!" > "$1"
if [ "$2" = yes ]; then exit 0; fi
while :; do sleep 0.05; done"#,
                "fixture",
            ])
            .arg(&receipt)
            .arg(if naturally { "yes" } else { "no" });
        let mut process = ChildGroup::spawn(&mut command)?;
        let pid = tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if let Ok(text) = std::fs::read_to_string(&receipt)
                    && let Ok(raw) = text.parse::<i32>()
                    && let Some(pid) = rustix::process::Pid::from_raw(raw)
                {
                    break pid;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await?;
        if naturally {
            assert!(
                tokio::time::timeout(Duration::from_secs(3), process.wait())
                    .await??
                    .success()
            );
        } else {
            process.stop(Duration::from_secs(3)).await?;
        }
        tokio::time::timeout(Duration::from_secs(3), async {
            while rustix::process::test_kill_process(pid).is_ok() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await?;
        std::fs::remove_file(&receipt)?;
    }
    Ok(())
}
