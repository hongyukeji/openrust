use super::*;
use crate::{
    container::{ClassAttribute, ClassDescriptor, ConstructorParam, attributes::Log},
    foundation::{ProviderBuilder, ProviderRuntime},
};
use figment::{Figment, providers::Serialized};
use std::time::Duration;

struct ChannelPair(Arc<Logger>, Arc<Logger>);

async fn start(settings: Value) -> Result<ProviderRuntime> {
    let mut builder = ProviderBuilder::new();
    builder.provider(LogServiceProvider);
    builder.class_descriptors([ClassDescriptor::concrete::<ChannelPair>(
        "test::LogChannels",
        vec![
            ConstructorParam::class::<Logger>("default")
                .with_attribute(ClassAttribute::contextual(|| Ok(Log::default()))),
            ConstructorParam::class::<Logger>("named").with_attribute(ClassAttribute::contextual(
                || Ok(Log::new(" named \t").named("request")),
            )),
        ],
        |args| Ok(Arc::new(ChannelPair(args.service(0)?, args.service(1)?))),
    )]);
    builder
        .start(Figment::from(Serialized::defaults(json!({
            "app": {"env": "testing"},
            "logging": settings
        }))))
        .await
}

async fn shutdown(runtime: &ProviderRuntime) -> Result<()> {
    runtime
        .shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await
}

#[tokio::test]
async fn attributes_choose_stack_and_named_sink_without_renaming_cached_logger()
-> std::result::Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let single = directory.path().join("default.log");
    let named = directory.path().join("named.log");
    let runtime = start(json!({
        "channels": {
            "single": {"path": "default.log", "level": null},
            "named": {"driver": "single", "path": named, "level": "notice", "name": "configured"}
        }
    }))
    .await?;
    let manager = runtime.container().resolve::<LogManager>().await?;
    manager
        .config
        .set("application_root", json!(directory.path()))?;
    let log = runtime.container().resolve::<ChannelPair>().await?;
    let root = manager.channel(None).await?;
    assert!(Arc::ptr_eq(&log.0, &root));
    let named_logger = manager.channel(Some("named")).await?;
    assert_eq!(named_logger.name(), "configured");
    assert_eq!(log.1.name(), "request");
    assert!(!single.exists() && !named.exists());
    log.0
        .log(
            LogLevel::Info,
            "default {word}",
            Map::from_iter([("word".into(), json!("test"))]),
        )
        .await?;
    log.1.log(LogLevel::Info, "filtered", Map::new()).await?;
    assert!(!named.exists());
    log.1
        .log(LogLevel::Notice, "record\r\ncontent", Map::new())
        .await?;
    named_logger
        .log(LogLevel::Warning, "base logger", Map::new())
        .await?;
    let default_text = tokio::fs::read_to_string(single).await?;
    let named_text = tokio::fs::read_to_string(named).await?;
    assert!(default_text.contains("testing.INFO: default test"));
    assert!(named_text.contains("request.NOTICE:"));
    assert!(named_text.contains("configured.WARNING: base logger"));
    assert_eq!(named_text.lines().count(), 2);
    assert!(!named_text.contains("default") && !default_text.contains("base logger"));
    shutdown(&runtime).await?;
    Ok(())
}

#[tokio::test]
async fn failed_resolution_uses_emergency_and_retries_after_config_changes()
-> std::result::Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let fallback = directory.path().join("emergency.log");
    let repaired = directory.path().join("repaired.log");
    let runtime = start(json!({
        "default": null,
        "channels": {
            "emergency": {"path": fallback},
            "future": {"driver": "not-registered", "token": "never-print-this"}
        }
    }))
    .await?;
    let manager = runtime.container().resolve::<LogManager>().await?;
    let null = manager.channel(Some("null")).await?;
    assert!(Arc::ptr_eq(&null, &manager.channel(None).await?));
    assert!(!fallback.exists());

    let first = manager.channel(Some("future")).await?;
    assert_eq!(first.name(), "openrust");
    let report = tokio::fs::read_to_string(&fallback).await?;
    assert!(report.contains("openrust.EMERGENCY:"));
    assert!(report.contains("log_driver_unsupported") && !report.contains("never-print-this"));
    manager.extend("custom-fail", |_| {
        Err(Error::custom("sensitive-dynamic-code", "sensitive detail"))
    });
    manager
        .config
        .set("logging.channels.future.driver", json!("custom-fail"))?;
    manager.channel(Some("future")).await?;
    let report = tokio::fs::read_to_string(&fallback).await?;
    assert!(report.contains("log_custom_driver_failed") && !report.contains("sensitive"));

    manager.config.set(
        "logging.channels.future",
        json!({"driver":"single","path": repaired}),
    )?;
    let future = manager.channel(Some("future")).await?;
    future.log(LogLevel::Info, "restored", Map::new()).await?;
    assert!(
        tokio::fs::read_to_string(&repaired)
            .await?
            .contains("restored")
    );
    manager
        .config
        .set("logging.channels.future.name", json!("new-name"))?;
    assert!(Arc::ptr_eq(
        &future,
        &manager.channel(Some("future")).await?
    ));
    manager.forget_channel(Some(" future "))?;
    assert_eq!(manager.channel(Some("future")).await?.name(), "new-name");
    shutdown(&runtime).await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn forgetting_an_in_flight_stack_keeps_the_new_configuration()
-> std::result::Result<(), Box<dyn std::error::Error>> {
    let runtime = start(json!({
        "channels": {
            "stack": {"channels": ["child"], "name": "original"},
            "child": {"driver": "test-driver"}
        }
    }))
    .await?;
    let manager = runtime.container().resolve::<LogManager>().await?;
    let (entered, observe) = tokio::sync::oneshot::channel();
    let entered = Mutex::new(Some(entered));
    let (release, resume) = std::sync::mpsc::channel();
    let resume = Mutex::new(resume);
    manager.extend("test-driver", move |_| {
        if let Some(entered) = lock(&entered).take() {
            let _ = entered.send(());
        }
        lock(&resume)
            .recv_timeout(Duration::from_secs(5))
            .map_err(Error::internal)?;
        Ok(sinks::null())
    });
    let constructing = {
        let manager = Arc::clone(&manager);
        tokio::spawn(async move { manager.channel(None).await })
    };
    tokio::time::timeout(Duration::from_secs(2), observe).await??;
    manager.config.set(
        "logging.channels.stack",
        json!({
            "driver": "stack", "channels": ["null"], "name": "replacement"
        }),
    )?;
    manager.forget_channel(None)?;
    let replacement = manager.channel(None).await?;
    release.send(())?;
    assert_eq!(constructing.await??.name(), "original");
    assert_eq!(replacement.name(), "replacement");
    assert!(Arc::ptr_eq(&replacement, &manager.channel(None).await?));
    shutdown(&runtime).await?;
    Ok(())
}

struct BrokenSink;

impl LogSink for BrokenSink {
    fn write<'a>(&'a self, _record: &'a super::super::LogRecord) -> BoxFuture<'a, Result<()>> {
        Box::pin(async { Err(Error::custom("test_sink_failure", "测试写入失败")) })
    }
}

#[tokio::test]
async fn outer_ignore_keeps_handlers_after_failure_in_nested_stack()
-> std::result::Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("nested.log");
    let runtime = start(json!({
        "channels": {
            "stack": {"channels": ["nested"], "ignore_exceptions": true, "name": "outer"},
            "nested": {"driver": "stack", "channels": ["fail", "file"]},
            "fail": {"driver": "fail-write"},
            "file": {"driver": "single", "path": path}
        }
    }))
    .await?;
    let manager = runtime.container().resolve::<LogManager>().await?;
    manager.extend("fail-write", |_| Ok(Arc::new(BrokenSink)));
    let root = manager.channel(None).await?;
    root.log(LogLevel::Info, "nested", Map::new()).await?;
    assert!(
        tokio::fs::read_to_string(path)
            .await?
            .contains("outer.INFO: nested")
    );
    shutdown(&runtime).await?;
    Ok(())
}

#[tokio::test]
async fn cyclic_stacks_return_emergency_without_waiting_for_each_other()
-> std::result::Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let fallback = directory.path().join("emergency.log");
    let runtime = start(json!({
        "channels": {
            "a": {"driver": "stack", "channels": ["b"]},
            "b": {"driver": "stack", "channels": "a"},
            "emergency": {"path": fallback}
        }
    }))
    .await?;
    let manager = runtime.container().resolve::<LogManager>().await?;
    let (a, b) = tokio::time::timeout(Duration::from_secs(2), async {
        tokio::join!(manager.channel(Some("a")), manager.channel(Some("b")))
    })
    .await?;
    assert_eq!(a?.name(), "openrust");
    assert_eq!(b?.name(), "openrust");
    let text = tokio::fs::read_to_string(fallback).await?;
    assert_eq!(
        text.lines()
            .filter(|line| line.contains("log_channel_cycle"))
            .count(),
        2
    );
    shutdown(&runtime).await?;
    Ok(())
}

#[tokio::test]
async fn default_and_emergency_logs_share_the_openrust_path()
-> std::result::Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let logs = directory.path().join("storage/logs");
    std::fs::create_dir_all(&logs)?;
    for previous in ["application.log", "laravel.log"] {
        std::fs::write(logs.join(previous), "历史记录\n")?;
    }
    let path = logs.join("openrust.log");
    let runtime = start(json!({})).await?;
    let manager = runtime.container().resolve::<LogManager>().await?;
    manager
        .config
        .set("application_root", json!(directory.path()))?;

    let default = manager.channel(None).await?;
    assert_eq!(default.name(), "testing");
    default.log(LogLevel::Info, "默认频道", Map::new()).await?;

    let emergency = manager.channel(Some("unknown")).await?;
    assert_eq!(emergency.name(), "openrust");
    manager
        .config
        .set("logging.channels.emergency.path", Value::Null)?;
    manager.channel(Some("unknown")).await?;

    let contents = std::fs::read_to_string(path)?;
    assert!(contents.contains("testing.INFO: 默认频道"));
    assert_eq!(contents.matches("openrust.EMERGENCY:").count(), 2);
    for previous in ["application.log", "laravel.log"] {
        assert_eq!(std::fs::read_to_string(logs.join(previous))?, "历史记录\n");
    }
    shutdown(&runtime).await?;
    Ok(())
}
