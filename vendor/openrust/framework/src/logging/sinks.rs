//! 单文件、标准错误、空输出和顺序组合的内置日志写入端。

use super::{LogLevel, LogRecord, LogSink};
use crate::support::BoxFuture;
use crate::{Error, Result};
use serde_json::Value;
use std::{io::Write as _, path::PathBuf, sync::Arc};
use tokio::{
    fs::{File, OpenOptions, create_dir_all},
    io::AsyncWriteExt as _,
    sync::Mutex,
};

pub(super) fn single(
    path: PathBuf,
    level: LogLevel,
    replace_placeholders: bool,
) -> Arc<dyn LogSink> {
    Arc::new(Single {
        path,
        level,
        replace_placeholders,
        file: Mutex::new(None),
    })
}

pub(super) fn stderr(level: LogLevel, replace_placeholders: bool) -> Arc<dyn LogSink> {
    Arc::new(Stderr {
        level,
        replace_placeholders,
    })
}

pub(super) fn null() -> Arc<dyn LogSink> {
    Arc::new(Null)
}

pub(super) fn stack(sinks: Arc<[Arc<dyn LogSink>]>, ignore_exceptions: bool) -> Arc<dyn LogSink> {
    Arc::new(Stack {
        sinks,
        ignore_exceptions,
    })
}

struct Single {
    path: PathBuf,
    level: LogLevel,
    replace_placeholders: bool,
    file: Mutex<Option<File>>,
}

impl Single {
    async fn open(&self) -> Result<File> {
        if let Some(directory) = self
            .path
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
        {
            create_dir_all(directory)
                .await
                .map_err(|_| write_failed())?;
        }
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .await
            .map_err(|_| write_failed())
    }
}

impl LogSink for Single {
    fn write<'a>(&'a self, record: &'a LogRecord) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            if record.level < self.level {
                return Ok(());
            }
            let line = format_line(record, self.replace_placeholders)?;
            let mut cached = self.file.lock().await;
            let mut file = if let Some(file) = cached.take() {
                file
            } else {
                self.open().await?
            };
            // 写入失败或中途取消时，下一条记录会重新打开文件。
            file.write_all(line.as_bytes())
                .await
                .map_err(|_| write_failed())?;
            file.flush().await.map_err(|_| write_failed())?;
            *cached = Some(file);
            Ok(())
        })
    }
}

struct Stderr {
    level: LogLevel,
    replace_placeholders: bool,
}

impl LogSink for Stderr {
    fn write<'a>(&'a self, record: &'a LogRecord) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            if record.level < self.level {
                return Ok(());
            }
            let line = format_line(record, self.replace_placeholders)?;
            tokio::task::spawn_blocking(move || {
                let mut stream = std::io::stderr().lock();
                stream
                    .write_all(line.as_bytes())
                    .and_then(|()| stream.flush())
                    .map_err(|_| write_failed())
            })
            .await
            .map_err(|_| write_failed())?
        })
    }
}

struct Null;

impl LogSink for Null {
    fn write<'a>(&'a self, _record: &'a LogRecord) -> BoxFuture<'a, Result<()>> {
        Box::pin(async { Ok(()) })
    }
}

struct Stack {
    sinks: Arc<[Arc<dyn LogSink>]>,
    ignore_exceptions: bool,
}

impl LogSink for Stack {
    fn write<'a>(&'a self, record: &'a LogRecord) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            for sink in self.sinks.iter() {
                let result = sink.write(record).await;
                if !self.ignore_exceptions {
                    result?;
                }
            }
            Ok(())
        })
    }
}

fn write_failed() -> Error {
    Error::custom("log_write_failed", "无法写入日志记录")
}

fn format_line(record: &LogRecord, replace_placeholders: bool) -> Result<String> {
    let mut line = format!("[{}] ", record.at.format("%Y-%m-%d %H:%M:%S"));
    append_single_line(&mut line, &record.channel);
    line.push('.');
    line.push_str(&record.level.to_string());
    line.push_str(": ");
    if replace_placeholders && record.message.contains('{') && !record.context.is_empty() {
        append_single_line(&mut line, &interpolate(record));
    } else {
        append_single_line(&mut line, &record.message);
    }
    if !record.context.is_empty() {
        line.push(' ');
        let context = serde_json::to_string(&record.context).map_err(|_| write_failed())?;
        for character in context.chars() {
            match character {
                '\u{0085}' => line.push_str(r"\u0085"),
                '\u{2028}' => line.push_str(r"\u2028"),
                '\u{2029}' => line.push_str(r"\u2029"),
                _ => line.push(character),
            }
        }
    }
    line.push('\n');
    Ok(line)
}

fn append_single_line(line: &mut String, text: &str) {
    for character in text.chars() {
        if character.is_control() || matches!(character, '\u{2028}' | '\u{2029}') {
            line.extend(character.escape_default());
        } else {
            line.push(character);
        }
    }
}

fn interpolate(record: &LogRecord) -> String {
    let mut message = String::with_capacity(record.message.len());
    let mut remaining = record.message.as_str();
    while let Some(open) = remaining.find('{') {
        message.push_str(&remaining[..open]);
        remaining = &remaining[open + 1..];
        let Some(end) = remaining.find(['{', '}']) else {
            message.push('{');
            break;
        };
        let key = &remaining[..end];
        if remaining.as_bytes()[end] == b'{' {
            message.push('{');
            message.push_str(key);
            remaining = &remaining[end..];
            continue;
        }
        match record.context.get(key) {
            Some(Value::Null | Value::Bool(false)) => {}
            Some(Value::Bool(true)) => message.push('1'),
            Some(Value::String(value)) => message.push_str(value),
            Some(value @ (Value::Array(_) | Value::Object(_))) => {
                message.push_str("array");
                message.push_str(&value.to_string());
            }
            Some(Value::Number(value)) => message.push_str(&value.to_string()),
            None => {
                message.push('{');
                message.push_str(key);
                message.push('}');
            }
        }
        remaining = &remaining[end + 1..];
    }
    message.push_str(remaining);
    message
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;

    fn record(level: LogLevel) -> LogRecord {
        LogRecord {
            at: chrono::DateTime::UNIX_EPOCH,
            channel: Arc::from("example"),
            level,
            message: "请求 {id}\n{missing}".to_owned(),
            context: serde_json::Map::from_iter([("id".to_owned(), Value::from(42))]),
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
        let error =
            sink.write(&entry).await.err().ok_or_else(|| {
                Error::custom("test_expected_write_error", "以目录为日志文件应失败")
            })?;
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
}
