//! 每个子进程单独读取 Session 环境变量，避免并行测试修改全局环境。
#![cfg(feature = "session")]

use openrust::config::{ConfigLoader, Environment, SessionConfig};
use serde::Deserialize;
use std::{process::Command, time::Duration};

#[derive(Deserialize)]
struct Loaded {
    session: SessionConfig,
}

#[test]
#[ignore = "由 session_environment_aliases_are_isolated_and_namespaced 驱动的子进程"]
fn session_environment_child() -> Result<(), Box<dyn std::error::Error>> {
    let case = std::env::var("OPENRUST_SESSION_ENV_CASE")?;
    let config = tempfile::tempdir()?;
    let source = ConfigLoader::new(config.path(), Environment::Test).figment()?;
    if case == "absent" {
        assert!(source.find_value("session").is_err());
        return Ok(());
    }
    let loaded: Loaded = source.extract()?;
    let value = loaded.session;
    match case.as_str() {
        "conventional" => {
            assert_eq!(value.driver.as_deref(), Some("array"));
            assert_eq!(value.cookie.as_deref(), Some("env_session"));
            assert_eq!(value.lifetime, 7);
            assert_eq!(value.path, "/env");
            assert_eq!(value.domain.as_deref(), Some("example.test"));
            assert_eq!(value.secure, Some(false));
            assert!(!value.http_only);
            assert_eq!(value.same_site.as_deref(), Some("strict"));
            assert!(value.expire_on_close && value.partitioned);
        }
        "namespace" => {
            assert_eq!(value.cookie.as_deref(), Some("namespaced_session"));
            assert_eq!(value.lifetime, 9);
            assert_eq!(value.secure, Some(true));
            assert_eq!(value.same_site.as_deref(), Some("lax"));
        }
        "nullable" | "namespaced_nullable" => {
            assert!(value.driver.is_none());
            assert!(value.domain.is_none());
            assert!(value.secure.is_none());
            assert!(value.same_site.is_none());
        }
        _ => return Err("未知 Session 环境子测试".into()),
    }
    Ok(())
}

#[test]
fn session_environment_aliases_are_isolated_and_namespaced()
-> Result<(), Box<dyn std::error::Error>> {
    for case in [
        "conventional",
        "namespace",
        "nullable",
        "namespaced_nullable",
        "absent",
    ] {
        let mut command = Command::new(std::env::current_exe()?);
        command
            .env_clear()
            .args(["--ignored", "--exact", "session_environment_child"])
            .env("OPENRUST_SESSION_ENV_CASE", case);
        if case != "absent" {
            for (key, value) in [
                ("SESSION_DRIVER", "array"),
                ("SESSION_COOKIE", "env_session"),
                ("SESSION_LIFETIME", "7"),
                ("SESSION_PATH", "/env"),
                ("SESSION_DOMAIN", "example.test"),
                ("SESSION_SECURE_COOKIE", "false"),
                ("SESSION_HTTP_ONLY", "false"),
                ("SESSION_SAME_SITE", "strict"),
                ("SESSION_EXPIRE_ON_CLOSE", "true"),
                ("SESSION_PARTITIONED_COOKIE", "true"),
            ] {
                command.env(key, value);
            }
        }
        if case == "namespace" {
            command
                .env("APP_SESSION_COOKIE", "namespaced_session")
                .env("APP_SESSION_LIFETIME", "9")
                .env("APP_SESSION_SECURE", "true")
                .env("APP_SESSION_SAME_SITE", "lax");
        } else if case == "nullable" {
            for key in [
                "SESSION_DRIVER",
                "SESSION_DOMAIN",
                "SESSION_SECURE_COOKIE",
                "SESSION_SAME_SITE",
            ] {
                command.env(key, "null");
            }
        }
        if case == "namespaced_nullable" {
            for key in [
                "APP_SESSION_DRIVER",
                "APP_SESSION_DOMAIN",
                "APP_SESSION_SECURE",
                "APP_SESSION_SAME_SITE",
            ] {
                command.env(key, "(NULL)");
            }
        }
        command.stdout(std::process::Stdio::piped());
        command.stderr(std::process::Stdio::piped());
        let mut child = command.spawn()?;
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break status;
            }
            if std::time::Instant::now() >= deadline {
                child.kill()?;
                child.wait()?;
                return Err("Session 环境子测试超时".into());
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        let output = child.wait_with_output()?;
        // 只报告固定场景；子进程不继承调用方环境或凭据。
        if !status.success() {
            return Err(format!(
                "Session 环境子测试失败: {case}; {}",
                String::from_utf8_lossy(&output.stderr)
            )
            .into());
        }
    }
    Ok(())
}
