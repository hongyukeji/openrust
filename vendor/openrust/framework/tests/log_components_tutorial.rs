use openrust::{
    config::Repository,
    log::{
        LogContext, LogManager,
        formatters::HtmlFormatter,
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
                    serde_json::to_value(record.context.as_ref()).unwrap(),
                    expected["context"]
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
