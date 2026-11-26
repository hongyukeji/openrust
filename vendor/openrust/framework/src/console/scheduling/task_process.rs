use std::{ffi::OsString, io, path::Path, process::Output};
use tokio::process::Command;

use super::task_output::Destination;

pub(super) fn application_environment(root: &Path) -> OsString {
    let mut value = OsString::from("APP_BASE_PATH=");
    value.push(root);
    value
}

pub(super) async fn effective_user_is(user: &str) -> io::Result<bool> {
    let (option, expected) = match user.strip_prefix('#') {
        Some(uid) => ("-u", uid),
        None => ("-un", user),
    };
    let result = Command::new("/usr/bin/id").arg(option).output().await?;
    if !result.status.success() {
        return Err(io::Error::other("无法读取调度进程的系统用户"));
    }
    Ok(String::from_utf8_lossy(&result.stdout).trim_end_matches(['\r', '\n']) == expected)
}

pub(super) async fn run_as_user(
    user: &str,
    root: &Path,
    invocation: &[OsString],
    destination: Option<&Destination>,
) -> io::Result<Output> {
    let mut command = sudo(user, root);
    // 重定向在目标用户下打开文件，路径和参数由子进程读取。
    let script = match destination {
        Some(destination) if destination.append => r#"file=$1; shift; exec "$@" >> "$file" 2>&1"#,
        Some(_) => r#"file=$1; shift; exec "$@" > "$file" 2>&1"#,
        None => r#"exec "$@""#,
    };
    command.args(["/bin/sh", "-c", script, "schedule-output"]);
    if let Some(destination) = destination {
        command.arg(root.join(&destination.path));
    }
    command.args(invocation).output().await
}

pub(super) async fn spawn_background_as_user(
    user: &str,
    root: &Path,
    executable: &Path,
    args: &[String],
) -> io::Result<()> {
    // 等待 sudo 返回授权或启动结果，后台 Artisan 自己完成任务并释放锁。
    let script = r#""$@" </dev/null >/dev/null 2>&1 &"#;
    let result = sudo(user, root)
        .args([
            "/bin/sh",
            "-c",
            script,
            "schedule-background",
            "/usr/bin/env",
        ])
        .arg(application_environment(root))
        .arg(executable)
        .args(args)
        .output()
        .await?;
    if !result.status.success() {
        return Err(io::Error::other(format!(
            "无法以 {user} 启动后台调度任务：{}",
            String::from_utf8_lossy(&result.stderr).trim()
        )));
    }
    Ok(())
}

fn sudo(user: &str, root: &Path) -> Command {
    let mut command = Command::new("sudo");
    command
        .args(["-n", "-u", user, "--"])
        .current_dir(root)
        .kill_on_drop(true);
    command
}
