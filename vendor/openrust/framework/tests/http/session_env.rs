//! 每个子进程单独读取 Session 环境变量，避免并行测试修改全局环境。
#![cfg(feature = "session")]

use openrust::config::{ConfigError, ConfigLoader, Environment, SessionConfig};
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
    let config_loader = ConfigLoader::new(config.path(), Environment::Test);
    if matches!(case.as_str(), "table_null" | "namespaced_table_null") {
        let error = config_loader
            .load::<Loaded>()
            .err()
            .ok_or("显式null Session table不得回退sessions")?;
        assert!(matches!(
            error,
            ConfigError::Source(_) | ConfigError::Invalid(_) | ConfigError::Diagnostic(_)
        ));
        assert!(
            error.to_string().contains("table"),
            "错误必须标识Session表配置"
        );
        return Ok(());
    }
    let source = if case == "process_over_dotenv" {
        std::fs::write(
            config.path().join(".env"),
            "SESSION_DRIVER=array\nSESSION_CONNECTION=dotenv_connection\nSESSION_TABLE=dotenv_sessions\n",
        )?;
        ConfigLoader::application_sources(config.path(), "APP_")?
            .figment()
            .clone()
    } else {
        config_loader.figment()?
    };
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
            assert_eq!(value.connection.as_deref(), Some("reporting"));
            assert_eq!(value.table, "conventional_sessions");
        }
        "namespace" => {
            assert_eq!(value.cookie.as_deref(), Some("namespaced_session"));
            assert_eq!(value.lifetime, 9);
            assert_eq!(value.secure, Some(true));
            assert_eq!(value.same_site.as_deref(), Some("lax"));
            assert_eq!(value.connection.as_deref(), Some("namespace_reporting"));
            assert_eq!(value.table, "namespace_sessions");
        }
        "nullable" | "namespaced_nullable" => {
            assert!(value.driver.is_none());
            assert!(value.domain.is_none());
            assert!(value.secure.is_none());
            assert!(value.same_site.is_none());
            assert_eq!(value.connection, None);
            assert_eq!(value.table, "sessions");
        }
        "database_defaults" | "connection_parenthesized" => {
            assert_eq!(value.connection, None);
            assert_eq!(value.table, "sessions");
        }
        "connection_quoted_null" | "namespaced_quoted_null" => {
            assert_eq!(value.connection.as_deref(), Some("null"));
            assert_eq!(value.table, "sessions");
        }
        "quoted_reporting" => {
            assert_eq!(value.connection.as_deref(), Some("reporting"));
            assert_eq!(value.table, "quoted_sessions");
        }
        "process_over_dotenv" => {
            assert_eq!(value.connection.as_deref(), Some("process_reporting"));
            assert_eq!(value.table, "process_sessions");
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
        "database_defaults",
        "connection_parenthesized",
        "connection_quoted_null",
        "namespaced_quoted_null",
        "quoted_reporting",
        "table_null",
        "namespaced_table_null",
        "process_over_dotenv",
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
        database_environment(&mut command, case);
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

fn database_environment(command: &mut Command, case: &str) {
    match case {
        "conventional" => {
            command
                .env("SESSION_CONNECTION", "reporting")
                .env("SESSION_TABLE", "conventional_sessions");
        }
        "namespace" => {
            command
                .env("SESSION_CONNECTION", "conventional_reporting")
                .env("SESSION_TABLE", "conventional_sessions")
                .env("APP_SESSION_CONNECTION", "namespace_reporting")
                .env("APP_SESSION_TABLE", "namespace_sessions");
        }
        "nullable" => {
            command.env("SESSION_CONNECTION", "null");
        }
        "namespaced_nullable" => {
            command
                .env("SESSION_CONNECTION", "conventional_reporting")
                .env("APP_SESSION_CONNECTION", "(NULL)");
        }
        "connection_parenthesized" => {
            command.env("SESSION_CONNECTION", "(null)");
        }
        "connection_quoted_null" => {
            command.env("SESSION_CONNECTION", "'null'");
        }
        "namespaced_quoted_null" => {
            command
                .env("SESSION_CONNECTION", "reporting")
                .env("APP_SESSION_CONNECTION", "\"null\"");
        }
        "quoted_reporting" => {
            command
                .env("SESSION_CONNECTION", "\"reporting\"")
                .env("SESSION_TABLE", "'quoted_sessions'");
        }
        "table_null" => {
            command.env("SESSION_TABLE", "null");
        }
        "namespaced_table_null" => {
            command
                .env("SESSION_TABLE", "valid_sessions")
                .env("APP_SESSION_TABLE", "(NULL)");
        }
        "process_over_dotenv" => {
            command
                .env("SESSION_CONNECTION", "process_reporting")
                .env("SESSION_TABLE", "process_sessions");
        }
        _ => {}
    }
}
