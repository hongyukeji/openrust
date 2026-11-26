use openrust::{
    contracts::{container::Container as _, foundation::Application as _},
    foundation::Application,
    log::{LogContext, LogManager},
    support::facades::{App, Config, Facade, Log},
};
use serde_json::json;
use std::sync::Arc;
fn settings(
    _: &std::path::Path,
    _: &openrust::config::Environment,
) -> Result<std::collections::BTreeMap<String, serde_json::Value>, openrust::config::ConfigError> {
    Ok(std::collections::BTreeMap::from([
        ("app".into(), json!({"env":"production"})),
        (
            "logging".into(),
            json!({"default":"first","channels":{"first":{"driver":"single","path":"first.log"},"other":{"driver":"single","path":"other.log"}}}),
        ),
    ]))
}
#[tokio::test]
async fn facade_management_uses_the_same_manager_cache_and_preserves_other_applications() {
    let left = tempfile::tempdir().unwrap();
    let right = tempfile::tempdir().unwrap();
    let mut first = Application::configure(left.path().into())
        .with_config(settings)
        .create()
        .unwrap();
    first.boot().await.unwrap();
    let mut second = Application::configure(right.path().into())
        .with_config(settings)
        .create()
        .unwrap();
    second.boot().await.unwrap();
    App::set_facade_application(Some(&first), async {
        assert_eq!(Log::get_default_driver().unwrap().as_deref(), Some("first"));
        let cached = Log::driver(None).unwrap();
        assert!(Arc::ptr_eq(&cached, &Log::channel(None).unwrap()));
        Log::set_default_driver("other").unwrap();
        Log::info("second selected", &LogContext::new()).unwrap();
        assert_eq!(
            first
                .container()
                .make::<LogManager>()
                .unwrap()
                .get_default_driver()
                .as_deref(),
            Some("other")
        );
        Log::set_default_driver("first").unwrap();
        Config::set("logging.channels.first.path", "reloaded.log".into()).unwrap();
        cached.info("old handle", &LogContext::new()).unwrap();
        Log::forget_channel(None).unwrap();
        let reloaded = Log::driver(None).unwrap();
        assert!(!Arc::ptr_eq(&cached, &reloaded));
        reloaded.info("new handle", &LogContext::new()).unwrap();
        Log::extend("compiled_native", |manager, _| {
            Ok(manager.channel(Some("other"))?.get_logger()
                as Arc<dyn openrust::contracts::log::Logger>)
        })
        .unwrap();
        Log::build(&json!({"driver":"compiled_native"}))
            .unwrap()
            .warning("extended driver", &LogContext::new())
            .unwrap();
    })
    .await;
    App::set_facade_application(Some(&second), async {
        assert_eq!(Log::get_default_driver().unwrap().as_deref(), Some("first"));
        Log::driver(None)
            .unwrap()
            .info("right app", &LogContext::new())
            .unwrap();
    })
    .await;
    assert!(
        std::fs::read_to_string(left.path().join("first.log"))
            .unwrap()
            .contains("old handle")
    );
    assert!(
        std::fs::read_to_string(left.path().join("reloaded.log"))
            .unwrap()
            .contains("new handle")
    );
    let other = std::fs::read_to_string(left.path().join("other.log")).unwrap();
    assert!(other.contains("second selected") && other.contains("extended driver"));
    assert!(
        std::fs::read_to_string(right.path().join("first.log"))
            .unwrap()
            .contains("right app")
    );
    assert!(!right.path().join("reloaded.log").exists());
    assert!(Log::driver(None).is_err());
    assert!(Log::get_default_driver().is_err());
    assert!(Log::forget_channel(None).is_err());
    assert!(Log::set_default_driver("first").is_err());
}
#[tokio::test]
async fn eight_named_levels_are_callable_on_facade_manager_logger_and_native_handle() {
    use openrust::log::{LogResult, Logger, NativeLogger};
    type InstanceWrite<T> = fn(&T, &str, &LogContext) -> LogResult<()>;
    let root = tempfile::tempdir().unwrap();
    let mut app = Application::configure(root.path().into())
        .with_config(settings)
        .create()
        .unwrap();
    app.boot().await.unwrap();
    App::set_facade_application(Some(&app), async {
        Config::set("logging.channels.first.formatter", "json".into()).unwrap();
        let manager = app.container().make::<LogManager>().unwrap();
        let logger = Log::channel(None).unwrap();
        let native = logger.get_logger();
        let context = LogContext::new();
        let facade: [fn(&str, &LogContext) -> LogResult<()>; 8] = [
            Log::emergency,
            Log::alert,
            Log::critical,
            Log::error,
            Log::warning,
            Log::notice,
            Log::info,
            Log::debug,
        ];
        for write in facade {
            write("facade", &context).unwrap();
        }
        let managers: [InstanceWrite<LogManager>; 8] = [
            LogManager::emergency,
            LogManager::alert,
            LogManager::critical,
            LogManager::error,
            LogManager::warning,
            LogManager::notice,
            LogManager::info,
            LogManager::debug,
        ];
        for write in managers {
            write(&manager, "manager", &context).unwrap();
        }
        let loggers: [InstanceWrite<Logger>; 8] = [
            Logger::emergency,
            Logger::alert,
            Logger::critical,
            Logger::error,
            Logger::warning,
            Logger::notice,
            Logger::info,
            Logger::debug,
        ];
        for write in loggers {
            write(&logger, "logger", &context).unwrap();
        }
        let natives: [InstanceWrite<NativeLogger>; 8] = [
            NativeLogger::emergency,
            NativeLogger::alert,
            NativeLogger::critical,
            NativeLogger::error,
            NativeLogger::warning,
            NativeLogger::notice,
            NativeLogger::info,
            NativeLogger::debug,
        ];
        for write in natives {
            write(&native, "native", &context).unwrap();
        }
    })
    .await;
    let text = std::fs::read_to_string(root.path().join("first.log")).unwrap();
    let rows = text
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(rows.len(), 32);
    for (i, surface) in ["facade", "manager", "logger", "native"].iter().enumerate() {
        for (j, level) in [
            "EMERGENCY",
            "ALERT",
            "CRITICAL",
            "ERROR",
            "WARNING",
            "NOTICE",
            "INFO",
            "DEBUG",
        ]
        .iter()
        .enumerate()
        {
            assert_eq!(rows[i * 8 + j]["message"], *surface);
            assert_eq!(rows[i * 8 + j]["level_name"], *level);
        }
    }
}
