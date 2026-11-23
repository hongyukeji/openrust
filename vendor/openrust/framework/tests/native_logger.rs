use openrust::{
    config::Repository,
    contracts::log::Logger as LoggerContract,
    log::{Level, LogContext, LogError, LogManager, NativeLogger},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
};

struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("native-log-{:016x}", fastrand::u64(..)));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn manager(&self) -> LogManager {
        LogManager::new(
            Arc::new(Repository::new(BTreeMap::from([
                ("app".into(), json!({"env":"testing"})),
                (
                    "logging".into(),
                    json!({"default":"file","channels":{
                        "file":{"driver":"single","path":"native.log","name":"native","level":"info","formatter":"json","processors":["mark"]},
                        "broken":{"driver":"single","path":"occupied/log","name":"broken"},
                        "failed":{"driver":"single","path":"failed.log","processors":["fail"]},
                        "emergency":{"path":"emergency.log"}
                    }}),
                ),
            ]))),
            &self.0,
        )
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
fn context(name: &str) -> LogContext {
    LogContext::from_iter([("call".into(), json!(name))])
}

#[test]
fn native_access_uses_the_actual_channel_and_matches_fixed_reference() {
    let reference: Value =
        serde_json::from_str(include_str!("reference/native-logger.json")).unwrap();
    let root = Root::new();
    let manager = root.manager();
    manager.register_processor("mark", |_| {
        Ok(Arc::new(|record| {
            record
                .extra
                .to_mut()
                .insert("processed".into(), json!(true));
            Ok(())
        }))
    });
    assert!(manager.get_channels().is_empty());
    let logger = manager.channel(None).unwrap();
    let cached = manager.channel(None).unwrap();
    let native: Arc<NativeLogger> = logger.get_logger();
    assert!(Arc::ptr_eq(&native, &cached.get_logger()));
    assert_eq!(native.name(), "native");
    assert!(Arc::ptr_eq(&native, &manager.get_logger().unwrap()));
    assert_eq!(reference["same"], true);
    assert_eq!(manager.get_channels().len(), 1);
    logger.with_context(LogContext::from_iter([(
        "shared".into(),
        json!("application"),
    )]));
    let events = Arc::new(Mutex::new(Vec::new()));
    let captured = events.clone();
    logger.listen(move |event| { captured.lock().unwrap().push(json!({"level":format!("{:?}",event.level).to_lowercase(),"message":event.message,"context":event.context}));Ok(()) });
    logger.info("wrapped", &context("one")).unwrap();
    let contract: &dyn LoggerContract = native.as_ref();
    contract
        .log(Level::Info, "native", &context("two"))
        .unwrap();
    logger.debug("filtered", &context("three")).unwrap();
    native.debug("native filtered", &context("four")).unwrap();
    let records: Vec<_>=fs::read_to_string(root.0.join("native.log")).unwrap().lines().map(|line| {
        let row: Value=serde_json::from_str(line).unwrap();
        json!({"level":row["level_name"].as_str().unwrap().to_lowercase(),"message":row["message"],"context":row["context"],"extra":row["extra"],"channel":row["channel"]})
    }).collect();
    assert_eq!(json!(records), reference["records"]);
    assert_eq!(json!(*events.lock().unwrap()), reference["events"]);
    let renamed = logger.with_name("renamed");
    assert_eq!(renamed.name(), "renamed");
    assert_eq!(renamed.get_logger().name(), "renamed");
    assert_eq!(logger.get_logger().name(), "native");
    assert_eq!(manager.get_channels().len(), 1);
}

#[test]
fn native_failures_propagate_and_framework_recovery_and_context_remain_separate() {
    let root = Root::new();
    let manager = root.manager();
    manager.register_processor("mark", |_| Ok(Arc::new(|_| Ok(()))));
    fs::write(root.0.join("occupied"), "file").unwrap();
    manager.register_processor("fail", |_| Ok(Arc::new(|_| Err(LogError::FactoryFailed))));
    let events = Arc::new(Mutex::new(0));
    let captured = events.clone();
    manager
        .listen(move |_| {
            *captured.lock().unwrap() += 1;
            Ok(())
        })
        .unwrap();
    let broken = manager.channel(Some("broken")).unwrap();
    assert!(matches!(
        broken.get_logger().error("raw failure", &LogContext::new()),
        Err(LogError::WriteFailed(_))
    ));
    assert_eq!(*events.lock().unwrap(), 0);
    assert!(!root.0.join("emergency.log").exists());
    broken
        .error("wrapper recovery", &LogContext::new())
        .unwrap();
    assert!(
        fs::read_to_string(root.0.join("emergency.log"))
            .unwrap()
            .contains("wrapper recovery")
    );
    assert_eq!(*events.lock().unwrap(), 2);
    let failed = manager.channel(Some("failed")).unwrap();
    assert!(matches!(
        failed
            .get_logger()
            .info("processor failure", &LogContext::new()),
        Err(LogError::FactoryFailed)
    ));
    assert!(!root.0.join("failed.log").exists());
    let missing = manager.channel(Some("missing")).unwrap();
    let before = *events.lock().unwrap();
    missing
        .get_logger()
        .warning("raw emergency", &LogContext::new())
        .unwrap();
    assert_eq!(*events.lock().unwrap(), before);
    assert!(
        fs::read_to_string(root.0.join("emergency.log"))
            .unwrap()
            .contains("raw emergency")
    );
}

#[tokio::test]
async fn facade_and_manager_return_the_same_default_native_instance() {
    use openrust::{
        contracts::{container::Container as _, foundation::Application as _},
        foundation::Application,
        support::facades::Log,
    };
    fn configuration(
        _: &std::path::Path,
        _: &openrust::config::Environment,
    ) -> Result<BTreeMap<String, Value>, openrust::config::ConfigError> {
        Ok(BTreeMap::from([(
            "logging".into(),
            json!({"default":"file","channels":{"file":{"driver":"single","path":"facade.log","name":"native-facade"},"unused":{"driver":"single","path":"unused.log"}}}),
        )]))
    }
    let root = Root::new();
    let resolved = Arc::new(Mutex::new(None));
    let captured = resolved.clone();
    let mut app = Application::configure(root.0.clone())
        .with_config(configuration)
        .booted(move |app| {
            let manager = app.container().make::<LogManager>().unwrap();
            assert!(manager.get_channels().is_empty());
            let direct = manager.get_logger().unwrap();
            let facade = Log::get_logger().unwrap();
            let channel = Log::channel(None).unwrap().get_logger();
            assert!(Arc::ptr_eq(&direct, &facade));
            assert!(Arc::ptr_eq(&direct, &channel));
            facade.info("native facade", &LogContext::new()).unwrap();
            assert_eq!(Log::get_channels().unwrap().len(), 1);
            *captured.lock().unwrap() = Some((manager, facade));
            Ok(())
        })
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let (manager, held) = resolved.lock().unwrap().take().unwrap();
    manager.forget_channel(None);
    held.info("held after forget", &LogContext::new()).unwrap();
    assert!(!Arc::ptr_eq(&held, &manager.get_logger().unwrap()));
    let output = fs::read_to_string(root.0.join("facade.log")).unwrap();
    assert!(output.contains("native facade") && output.contains("held after forget"));
    assert!(!root.0.join("unused.log").exists());
}
