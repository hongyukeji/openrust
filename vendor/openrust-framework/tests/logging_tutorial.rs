use openrust::{
    config::{ConfigError, Environment, Value},
    foundation::{Application, configuration::Routing},
    log::LogContext,
    support::facades::{Artisan, Log},
};
use serde_json::json;
use std::{collections::BTreeMap, path::Path};
fn configuration(_: &Path, env: &Environment) -> Result<BTreeMap<String, Value>, ConfigError> {
    let mode = env
        .get_utf8("TEST_ERRORLOG_MODE")?
        .unwrap_or("0")
        .parse::<u8>()
        .unwrap();
    Ok(BTreeMap::from([
        ("app".into(), json!({"env":"tutorial"})),
        (
            "logging".into(),
            json!({"default":"errorlog","channels":{"errorlog":{"driver":"errorlog","type":mode,"name":"native-errors","level":"notice","replace_placeholders":true,"bubble":false},"after":{"driver":"single","path":"after.log"},"emergency":{"path":"fallback.log"}}}),
        ),
    ]))
}
fn commands() -> Result<(), openrust::console::ConsoleError> {
    Artisan::command("logs:tutorial", |_| {
        Log::stack(&["errorlog", "after"], None)
            .unwrap()
            .notice("stack-bubble", &LogContext::new())
            .unwrap();
        Log::debug("below-threshold", &LogContext::new()).unwrap();
        Log::notice(
            "native-errorlog {id}",
            &LogContext::from_iter([("id".into(), 42.into())]),
        )
        .unwrap();
        use openrust::support::facades::Facade as _;
        Log::get_facade_root()
            .unwrap()
            .register_formatter("flat", |_| {
                Ok(std::sync::Arc::new(|record| {
                    Ok(record.message().into_owned())
                }))
            });
        let mode = std::env::var("TEST_ERRORLOG_MODE")
            .unwrap()
            .parse::<u8>()
            .unwrap();
        let flat =
            Log::build(&json!({"driver":"errorlog","type":mode,"formatter":"flat"})).unwrap();
        flat.info("one", &LogContext::new()).unwrap();
        flat.info("two\n", &LogContext::new()).unwrap();
    })?
    .purpose("验收进程错误日志")
}
#[tokio::test]
async fn configured_errorlog_is_a_real_artisan_stderr_channel_in_both_output_modes() {
    if let Some(root) = std::env::var_os("TEST_ERRORLOG_ROOT") {
        let app = Application::configure(root.into())
            .with_config(configuration)
            .with_routing(Routing::default().commands(commands))
            .create()
            .unwrap();
        assert_eq!(
            app.handle_command(&["logs:tutorial".into()]).await.unwrap(),
            0
        );
        return;
    }
    for mode in ["0", "4"] {
        let root = tempfile::tempdir().unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "configured_errorlog_is_a_real_artisan_stderr_channel_in_both_output_modes",
                "--nocapture",
            ])
            .env("TEST_ERRORLOG_ROOT", root.path())
            .env("TEST_ERRORLOG_MODE", mode)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let text = String::from_utf8(output.stderr).unwrap();
        assert!(
            text.contains("native-errors.NOTICE: native-errorlog 42"),
            "{text}"
        );
        assert!(!text.contains("below-threshold"));
        assert!(text.contains("one\ntwo\n\n"), "{text}");
        assert!(!text.contains("EMERGENCY"));
        assert!(!root.path().join("fallback.log").exists());
        assert!(
            std::fs::read_to_string(root.path().join("after.log"))
                .unwrap()
                .contains("stack-bubble")
        );
    }
}
#[test]
fn unsupported_errorlog_modes_use_the_existing_emergency_recovery() {
    use openrust::{config::Repository, log::LogManager};
    use std::sync::Arc;
    let root = tempfile::tempdir().unwrap();
    let manager = LogManager::new(Arc::new(Repository::new(BTreeMap::new())), root.path());
    for mode in [1, 3, 8] {
        manager
            .build(&json!({"driver":"errorlog","type":mode}))
            .unwrap()
            .info(&format!("recovered-mode-{mode}"), &LogContext::new())
            .unwrap();
    }
    let text = std::fs::read_to_string(root.path().join("storage/logs/app.log")).unwrap();
    assert_eq!(
        text.matches("Unable to create configured logger").count(),
        3
    );
    for mode in [1, 3, 8] {
        assert!(text.contains(&format!("recovered-mode-{mode}")));
    }
}
