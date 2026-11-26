use openrust::{
    config::Repository,
    log::{
        LogContext, LogManager,
        formatters::{HtmlFormatter, LineFormatter},
        processors::{MemoryUsageProcessor, PsrLogMessageProcessor},
    },
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc};
fn manager(root: &std::path::Path) -> LogManager {
    LogManager::new(Arc::new(Repository::new(BTreeMap::new())), root)
}
#[test]
fn html_and_psr_public_types_match_pinned_vectors() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("reference/log-components.json")).unwrap();
    for (case, level) in cases.into_iter().zip([
        openrust::log::Level::Debug,
        openrust::log::Level::Info,
        openrust::log::Level::Notice,
        openrust::log::Level::Warning,
        openrust::log::Level::Error,
        openrust::log::Level::Critical,
        openrust::log::Level::Alert,
        openrust::log::Level::Emergency,
    ]) {
        let dir = tempfile::tempdir().unwrap();
        let manager = manager(dir.path());
        let expected = case.clone();
        manager.register_processor("fixed", |_| {
            Ok(Arc::new(|record| {
                record.at = jiff::Timestamp::from_second(1795482123).unwrap();
                record.extra.to_mut().insert("trace".into(), "x".into());
                Ok(())
            }))
        });
        manager.register_formatter("verify", move |_| {
            let expected = expected.clone();
            Ok(Arc::new(move |record| {
                assert_eq!(
                    HtmlFormatter::new(Some("Y-m-d H:i:s"))
                        .format(record)
                        .unwrap(),
                    expected["html"]
                );
                assert_eq!(
                    HtmlFormatter::new(None).format(record).unwrap(),
                    expected["html_default"]
                );
                assert_eq!(HtmlFormatter::new(None).format_batch(&[]).unwrap(), "");
                assert_eq!(
                    HtmlFormatter::new(None)
                        .format_batch(std::slice::from_ref(record))
                        .unwrap(),
                    expected["html_default"]
                );
                assert_eq!(LineFormatter::new(Some("%message%|%context.id%|%context%|%extra.trace%|%extra%|%context.absent%|%channel%|%level_name%|%level%|%datetime%"),Some("Y-m-d H:i:s")).format(record).unwrap(),expected["line"]);
                assert_eq!(LineFormatter::new(None,None).format(record).unwrap(),expected["line_default"]);
                assert_eq!(LineFormatter::new(None,None).format_batch(std::slice::from_ref(record)).unwrap(),expected["line_default"]);
                Ok(expected["html"].as_str().unwrap().to_owned())
            }))
        });
        let logger=manager.build(&json!({"driver":"single","path":"formatted.html","name":"tutorial","level":"debug","processors":["fixed"],"formatter":"verify"})).unwrap();
        let context: LogContext = serde_json::from_value(case["input_context"].clone()).unwrap();
        let input_message = case["input_message"].as_str().unwrap().to_owned();
        logger.log(level, &input_message, &context).unwrap();
        manager.register_processor("verify_psr", move |_| {
            let expected = case.clone();
            Ok(Arc::new(move |record| {
                PsrLogMessageProcessor::new(true).process(record)?;
                assert_eq!(record.message, expected["message"].as_str().unwrap());
                assert_eq!(
                    serde_json::to_string(record.context.as_ref()).unwrap(),
                    serde_json::to_string(&expected["context"]).unwrap()
                );
                Ok(())
            }))
        });
        let logger = manager
            .build(&json!({"driver":"single","path":"psr.log","processors":["verify_psr"]}))
            .unwrap();
        logger.info(&input_message, &context).unwrap();
    }
}
#[test]
fn configured_components_and_custom_overrides_use_the_actual_channel() {
    let dir = tempfile::tempdir().unwrap();
    let manager = manager(dir.path());
    let logger=manager.build(&json!({"driver":"single","path":"audit.html","formatter":"html","formatter_with":{"dateFormat":"Y"},"processors":[{"processor":"psr_log_message","with":{"removeUsedContextFields":true}},{"processor":"memory_usage","with":{"useFormatting":false}}]})).unwrap();
    logger
        .info(
            "Created {id}",
            &LogContext::from_iter([("id".into(), 42.into())]),
        )
        .unwrap();
    let text = std::fs::read_to_string(dir.path().join("audit.html")).unwrap();
    assert!(
        text.contains("Created 42")
            && text.contains("memory_usage")
            && !text.contains("<pre>id</pre>")
    );
    assert!(MemoryUsageProcessor::new(false, true).is_err());
    manager.register_processor("memory_usage", |_| {
        Ok(Arc::new(|record| {
            record.extra.to_mut().insert("custom".into(), true.into());
            Ok(())
        }))
    });
    manager.register_formatter("html", |_| {
        Ok(Arc::new(|record| {
            assert_eq!(record.extra["custom"], true);
            Ok("OVERRIDE".into())
        }))
    });
    manager.build(&json!({"driver":"single","path":"override","formatter":"html","processors":["memory_usage"]})).unwrap().info("test",&LogContext::new()).unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.path().join("override")).unwrap(),
        "OVERRIDE"
    );
}
#[test]
fn public_memory_processor_records_numeric_and_formatted_usage() {
    let dir = tempfile::tempdir().unwrap();
    let manager = manager(dir.path());
    manager.register_processor("native_memory", |_| {
        Ok(Arc::new(|record| {
            MemoryUsageProcessor::new(true, false)?.process(record)?;
            assert!(record.extra["memory_usage"].as_u64().unwrap() > 0);
            MemoryUsageProcessor::new(true, true)?.process(record)?;
            assert!(
                record.extra["memory_usage"]
                    .as_str()
                    .unwrap()
                    .ends_with(" MB")
            );
            Ok(())
        }))
    });
    manager
        .build(&json!({"driver":"single","path":"memory","processors":["native_memory"]}))
        .unwrap()
        .info("memory", &LogContext::new())
        .unwrap();
}
#[test]
fn configured_line_formatter_preserves_exact_separators_and_native_builder_options() {
    let root = tempfile::tempdir().unwrap();
    let manager = manager(root.path());
    for (inline, expected) in [
        (false, "first second|value "),
        (true, "first\r\nsecond|value\n"),
    ] {
        let file = if inline { "inline" } else { "flat" };
        manager.build(&json!({"driver":"single","path":file,"formatter":"line","formatter_with":{"format":"%message%|%context.key%%context%%extra%","allowInlineLineBreaks":inline,"ignoreEmptyContextAndExtra":true}})).unwrap().info("first\r\nsecond",&LogContext::from_iter([("key".into(),"value\n".into())])).unwrap();
        assert_eq!(
            std::fs::read_to_string(root.path().join(file)).unwrap(),
            expected
        );
    }
    let mut formatter = LineFormatter::new(None, None);
    formatter
        .allow_inline_line_breaks(true)
        .ignore_empty_context_and_extra(true);
    assert_eq!(
        formatter.stringify(&json!({"line":"a\nb","literal":"a\\nb"})),
        "{\"line\":\"a\nb\",\"literal\":\"a\\\\nb\"}"
    );
    assert_eq!(formatter.stringify(&Value::Null), "NULL");
    assert_eq!(formatter.stringify(&json!(true)), "true");
    assert_eq!(formatter.format_batch(&[]).unwrap(), "");
}
#[test]
fn omitted_formatter_and_handler_owned_formatter_are_distinct_real_stream_outputs() {
    let root = tempfile::tempdir().unwrap();
    let manager = manager(root.path());
    manager.register_processor("fixed", |_| {
        Ok(Arc::new(|record| {
            record.at = jiff::Timestamp::from_second(1795482123).unwrap();
            Ok(())
        }))
    });
    for (driver, handler) in [("single", None), ("monolog", Some("stream"))] {
        for own in [false, true] {
            let path = format!("{driver}-{own}");
            let mut config =
                json!({"driver":driver,"path":path,"name":"tutorial","processors":["fixed"]});
            if let Some(handler) = handler {
                config["handler"] = handler.into();
                config["handler_with"] = json!({"stream":path});
            }
            if own {
                config["formatter"] = "default".into();
            }
            manager
                .build(&config)
                .unwrap()
                .info("owned", &LogContext::new())
                .unwrap();
            let text = std::fs::read_to_string(root.path().join(path)).unwrap();
            if own {
                assert_eq!(
                    text,
                    "[2026-11-24T01:02:03+00:00] tutorial.INFO: owned [] []\n"
                );
            } else {
                assert_eq!(text, "[2026-11-24 01:02:03] tutorial.INFO: owned\n");
            }
        }
    }
}
fn memory_config(
    _: &std::path::Path,
    _: &openrust::config::Environment,
) -> Result<BTreeMap<String, Value>, openrust::config::ConfigError> {
    Ok(BTreeMap::from([
        ("app".into(), json!({"env":"production"})),
        (
            "logging".into(),
            json!({"default":"memory","channels":{"memory":{"driver":"monolog","handler":"stream","handler_with":{"stream":"php://stderr"},"processors":["memory_usage",{"processor":"psr_log_message","with":{"removeUsedContextFields":true}}]}}}),
        ),
    ]))
}
fn memory_commands() -> Result<(), openrust::console::ConsoleError> {
    openrust::support::facades::Artisan::command("logs:memory", |_| {
        openrust::support::facades::Log::info(
            "Order {id}",
            &LogContext::from_iter([("id".into(), 42.into())]),
        )
        .unwrap();
    })?
    .purpose("验收处理器")
}
#[tokio::test]
async fn actual_artisan_uses_simple_and_configured_processors_on_the_real_stderr_handler() {
    if let Ok(root) = std::env::var("MEMORY_LOG_ROOT") {
        let app = openrust::foundation::Application::configure(root.into())
            .with_config(memory_config)
            .with_routing(
                openrust::foundation::configuration::Routing::default().commands(memory_commands),
            )
            .create()
            .unwrap();
        assert_eq!(
            app.handle_command(&["logs:memory".into()]).await.unwrap(),
            0
        );
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "actual_artisan_uses_simple_and_configured_processors_on_the_real_stderr_handler",
            "--nocapture",
        ])
        .env("MEMORY_LOG_ROOT", root.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stderr).unwrap();
    assert!(
        text.contains("Order 42") && text.contains("memory_usage") && text.contains(" MB"),
        "{text}"
    );
    assert!(!text.contains("\"id\":42"));
    assert!(!root.path().join("storage/logs/app.log").exists());
}
#[test]
fn native_handler_borrows_customize_actual_targets_without_changing_shared_logger_configuration() {
    let root = tempfile::tempdir().unwrap();
    let manager = manager(root.path());
    let logger=manager.build(&json!({"driver":"single","path":"shared.log","formatter":"line","formatter_with":{"format":"ORIGINAL:%message%\n"}})).unwrap();
    let original = logger.get_logger();
    let mut customized = (*original).clone();
    let mut handlers = customized.get_handlers();
    assert_eq!(handlers.len(), 1);
    handlers[0].set_formatter(Arc::new(|record| {
        Ok(format!("CUSTOM:{}\n", record.message()))
    }));
    drop(handlers);
    customized.info("modified", &LogContext::new()).unwrap();
    original.info("original", &LogContext::new()).unwrap();
    assert_eq!(
        std::fs::read_to_string(root.path().join("shared.log")).unwrap(),
        "CUSTOM:modified\nORIGINAL:original\n"
    );
    let fallback = manager
        .build(&json!({"driver":"undefined"}))
        .unwrap()
        .get_logger();
    let mut fallback = (*fallback).clone();
    assert!(fallback.get_handlers().is_empty());
    fallback
        .info("native emergency still works", &LogContext::new())
        .unwrap();
    assert!(
        std::fs::read_to_string(root.path().join("storage/logs/app.log"))
            .unwrap()
            .contains("native emergency still works")
    );
}
