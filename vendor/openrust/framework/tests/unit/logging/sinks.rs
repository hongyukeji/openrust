use super::*;
use std::sync::Mutex as StdMutex;

fn record(level: LogLevel) -> LogRecord {
    LogRecord {
        at: chrono::DateTime::UNIX_EPOCH,
        channel: Arc::from("example"),
        level,
        message: "请求 {id}\n{missing}".to_owned(),
        context: serde_json::Map::from_iter([("id".to_owned(), Value::from(42))]),
        extra: serde_json::Map::new(),
    }
}

#[test]
fn structured_context_escapes_unicode_line_separators() -> Result<()> {
    let mut record = record(LogLevel::Info);
    record.context.insert(
        "key".to_owned(),
        Value::from("a\u{0085}b\u{2028}c\u{2029}d"),
    );
    let line = format_line(&record, true)?;
    assert!(!line.contains(['\u{0085}', '\u{2028}', '\u{2029}']));
    let (_, context) = line.split_once(" {\"id\"").ok_or_else(write_failed)?;
    let decoded: Value =
        serde_json::from_str(&format!("{{\"id\"{context}")).map_err(Error::internal)?;
    assert_eq!(decoded["key"], record.context["key"]);
    Ok(())
}

#[test]
fn formats_dense_numeric_extra_as_a_list_and_mixed_extra_as_an_object() -> Result<()> {
    let mut record = record(LogLevel::Info);
    record.context.clear();
    record.extra.insert("0".into(), Value::from("first"));
    record.extra.insert("1".into(), Value::from("second"));
    assert!(format_line(&record, false)?.ends_with(" [\"first\",\"second\"]\n"));
    record.extra.insert("label".into(), Value::from("third"));
    assert!(
        format_line(&record, false)?
            .ends_with(" {\"0\":\"first\",\"1\":\"second\",\"label\":\"third\"}\n")
    );
    Ok(())
}

#[tokio::test]
async fn single_filters_before_opening_and_appends_one_line_per_record() -> Result<()> {
    let directory = tempfile::tempdir().map_err(Error::internal)?;
    let nested = directory.path().join("nested");
    let path = nested.join("example.log");
    let sink = single(path.clone(), LogLevel::Info, true);
    sink.write(&record(LogLevel::Debug)).await?;
    assert!(
        !tokio::fs::try_exists(nested)
            .await
            .map_err(Error::internal)?
    );
    sink.write(&record(LogLevel::Info)).await?;
    single(path.clone(), LogLevel::Debug, false)
        .write(&record(LogLevel::Error))
        .await?;
    let content = tokio::fs::read_to_string(path)
        .await
        .map_err(Error::internal)?;
    assert_eq!(
        content.lines().collect::<Vec<_>>(),
        [
            r#"[1970-01-01 00:00:00] example.INFO: 请求 42\n{missing} {"id":42}"#,
            r#"[1970-01-01 00:00:00] example.ERROR: 请求 {id}\n{missing} {"id":42}"#,
        ]
    );
    Ok(())
}

#[tokio::test]
async fn single_write_failure_keeps_path_and_record_private() -> Result<()> {
    let directory = tempfile::tempdir().map_err(Error::internal)?;
    let path = directory.path().join("private-path");
    tokio::fs::create_dir(&path)
        .await
        .map_err(Error::internal)?;
    let sink = single(path, LogLevel::Debug, false);
    let mut entry = record(LogLevel::Info);
    entry.message = "private-message".to_owned();
    entry
        .context
        .insert("field".to_owned(), Value::from("private-context"));
    let error = sink
        .write(&entry)
        .await
        .err()
        .ok_or_else(|| Error::custom("test_expected_write_error", "以目录为日志文件应失败"))?;
    assert_eq!(error.code(), "log_write_failed");
    let diagnostic = format!("{error} {error:?}");
    for value in ["private-path", "private-message", "private-context"] {
        assert!(!diagnostic.contains(value));
    }
    Ok(())
}

struct Probe {
    label: &'static str,
    visits: Arc<StdMutex<Vec<&'static str>>>,
    fail: bool,
}

impl LogSink for Probe {
    fn write<'a>(&'a self, _record: &'a LogRecord) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            self.visits
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(self.label);
            if self.fail {
                return Err(Error::custom("test_sink_failure", "探测 sink 写入失败"));
            }
            Ok(())
        })
    }
}

#[tokio::test]
async fn stack_obeys_order_and_configured_failure_handling() -> Result<()> {
    for (ignore, expected) in [
        (false, &["first", "failed"][..]),
        (true, &["first", "failed", "last"][..]),
    ] {
        let visits = Arc::new(StdMutex::new(Vec::new()));
        let probe = |label, fail| -> Arc<dyn LogSink> {
            Arc::new(Probe {
                label,
                fail,
                visits: Arc::clone(&visits),
            })
        };
        let sink = stack(
            vec![
                probe("first", false),
                null(),
                probe("failed", true),
                probe("last", false),
            ]
            .into(),
            ignore,
        );
        let result = sink.write(&record(LogLevel::Warning)).await;
        assert_eq!(
            result.as_ref().err().map(Error::code),
            (!ignore).then_some("test_sink_failure")
        );
        assert_eq!(
            visits
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .as_slice(),
            expected
        );
    }
    Ok(())
}
