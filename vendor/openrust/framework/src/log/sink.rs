use std::{
    cell::OnceCell,
    fs::{self, File, OpenOptions},
    io::{self, Write as _},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use std::borrow::Cow;

use super::{Formatter, Level, LogRecord, rotating::RotatingFile};
use crate::contracts::log::Logger;

pub(super) type Handlers = Arc<[Arc<Handler>]>;

/// Receives a structured log record and its formatted output.
pub type RecordHandler = dyn Fn(&LogRecord<'_>, &str) -> super::LogResult<()> + Send + Sync;

pub(super) enum Handler {
    File {
        options: FileOptions,
        file: Mutex<Option<File>>,
        level: Level,
        bubble: bool,
    },
    Rotating {
        file: RotatingFile,
        level: Level,
        bubble: bool,
    },
    Custom {
        logger: Arc<dyn Logger>,
        level: Level,
        bubble: bool,
    },
    Stderr {
        level: Level,
        bubble: bool,
    },
    Stdout {
        level: Level,
        bubble: bool,
    },
    Registered {
        write: Arc<RecordHandler>,
        level: Level,
        bubble: bool,
    },
    Buffered(Buffer),
    Null(Level),
    IgnoreFailures(Handlers),
    Formatted {
        handler: Arc<Handler>,
        formatter: Arc<Formatter>,
    },
}

pub(super) struct Buffer {
    target: Arc<Handler>,
    action_level: Level,
    stop_buffering: bool,
    state: Mutex<BufferState>,
}

struct BufferState {
    buffering: bool,
    records: Vec<BufferedRecord>,
}

type BufferedRecord = (LogRecord<'static>, Option<Arc<Formatter>>);

#[derive(Default)]
struct RecordText<'a> {
    message: OnceCell<Cow<'a, str>>,
    line: OnceCell<String>,
}

impl<'a> RecordText<'a> {
    fn message(&self, record: &'a LogRecord<'_>) -> &str {
        self.message.get_or_init(|| record.message())
    }

    fn line(&self, record: &'a LogRecord<'_>) -> &str {
        self.line
            .get_or_init(|| record.format_line(self.message(record)))
    }
}

impl Buffer {
    fn accept(
        &self,
        record: &LogRecord<'_>,
        formatter: Option<&Arc<Formatter>>,
    ) -> Option<Vec<BufferedRecord>> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !state.buffering {
            return None;
        }
        state.records.push((record.owned(), formatter.cloned()));
        if record.level < self.action_level {
            return Some(Vec::new());
        }
        state.buffering = !self.stop_buffering;
        Some(std::mem::take(&mut state.records))
    }
}

pub(super) struct FileOptions {
    pub path: PathBuf,
    pub permission: Option<u32>,
    pub locking: bool,
}

impl FileOptions {
    fn open(&self, path: &Path) -> io::Result<File> {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent)?;
        }
        let mut options = OpenOptions::new();
        options.create(true);
        if cfg!(windows) && self.locking {
            options.write(true);
        } else {
            options.append(true);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(self.permission.unwrap_or(0o644));
        }
        let file = options.open(path)?;
        #[cfg(not(unix))]
        let _ = self.permission;
        #[cfg(unix)]
        if let Some(permission) = self.permission {
            use std::os::unix::fs::PermissionsExt as _;
            file.set_permissions(fs::Permissions::from_mode(permission))?;
        }
        Ok(file)
    }

    pub(super) fn write(
        &self,
        path: &Path,
        cached: &mut Option<File>,
        bytes: &[u8],
    ) -> io::Result<()> {
        let mut file = match cached.take() {
            Some(file) => file,
            None => self.open(path)?,
        };
        if self.locking {
            file.lock()?;
            #[cfg(windows)]
            {
                use std::io::{Seek as _, SeekFrom};
                // Windows cannot lock an append-only handle.
                file.seek(SeekFrom::End(0))?;
            }
        }
        file.write_all(bytes)?;
        if self.locking {
            file.unlock()?;
        }
        *cached = Some(file);
        Ok(())
    }
}

impl Handler {
    fn accepts(&self, level: Level) -> bool {
        match self {
            Self::File { level: minimum, .. }
            | Self::Rotating { level: minimum, .. }
            | Self::Custom { level: minimum, .. }
            | Self::Stderr { level: minimum, .. }
            | Self::Stdout { level: minimum, .. }
            | Self::Registered { level: minimum, .. }
            | Self::Null(minimum) => level >= *minimum,
            Self::Buffered(_) => true,
            Self::IgnoreFailures(handlers) => accepts(handlers, level),
            Self::Formatted { handler, .. } => handler.accepts(level),
        }
    }

    pub(super) fn buffered(self, action_level: Level, stop_buffering: bool) -> Self {
        Self::Buffered(Buffer {
            target: Arc::new(self),
            action_level,
            stop_buffering,
            state: Mutex::new(BufferState {
                buffering: true,
                records: Vec::new(),
            }),
        })
    }

    pub fn file(
        path: PathBuf,
        permission: Option<u32>,
        locking: bool,
        level: Level,
        bubble: bool,
    ) -> Self {
        Self::File {
            options: FileOptions {
                path,
                permission,
                locking,
            },
            file: Mutex::default(),
            level,
            bubble,
        }
    }

    fn write<'a>(
        &self,
        record: &'a LogRecord<'_>,
        text: &RecordText<'a>,
        formatter: Option<&Arc<Formatter>>,
    ) -> io::Result<bool> {
        let level = record.level;
        let output = || match formatter {
            Some(formatter) => formatter(record).map(Cow::Owned).map_err(io::Error::other),
            None => Ok(Cow::Borrowed(text.line(record))),
        };
        match self {
            Self::File {
                options,
                file,
                level: minimum,
                bubble,
            } if level >= *minimum => {
                let output = output()?;
                let mut file = file
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                options.write(&options.path, &mut file, output.as_bytes())?;
                Ok(!bubble)
            }
            Self::Rotating {
                file,
                level: minimum,
                bubble,
            } if level >= *minimum => {
                file.write(
                    record.at.to_zoned(jiff::tz::TimeZone::UTC).date(),
                    output()?.as_bytes(),
                )?;
                Ok(!bubble)
            }
            Self::Custom {
                logger,
                level: minimum,
                bubble,
            } if level >= *minimum => {
                let message = text.message(record);
                let result = match &record.exception {
                    Some(error) => logger.logWithException(
                        level,
                        message,
                        &record.context,
                        Some(&record.extra),
                        error,
                    ),
                    None => logger.logWithContext(level, message, &record.context, &record.extra),
                };
                result.map_err(io::Error::other)?;
                Ok(!bubble)
            }
            Self::Stderr {
                level: minimum,
                bubble,
            } if level >= *minimum => {
                let output = output()?;
                io::stderr().lock().write_all(output.as_bytes())?;
                Ok(!bubble)
            }
            Self::Stdout {
                level: minimum,
                bubble,
            } if level >= *minimum => {
                let output = output()?;
                io::stdout().lock().write_all(output.as_bytes())?;
                Ok(!bubble)
            }
            Self::Registered {
                write,
                level: minimum,
                bubble,
            } if level >= *minimum => {
                write(record, &output()?).map_err(io::Error::other)?;
                Ok(!bubble)
            }
            Self::Buffered(buffer) => {
                if let Some(batch) = buffer.accept(record, formatter) {
                    for (record, formatter) in batch {
                        buffer
                            .target
                            .write(&record, &RecordText::default(), formatter.as_ref())?;
                    }
                } else {
                    buffer.target.write(record, text, formatter)?;
                }
                Ok(false)
            }
            Self::Null(minimum) => Ok(level >= *minimum),
            Self::IgnoreFailures(handlers) => {
                for handler in handlers.iter() {
                    let _ = handler.write(record, text, formatter);
                }
                Ok(false)
            }
            Self::Formatted {
                handler,
                formatter: default,
            } => handler.write(record, text, formatter.or(Some(default))),
            _ => Ok(false),
        }
    }
}

pub(super) fn accepts(handlers: &[Arc<Handler>], level: Level) -> bool {
    handlers.iter().any(|handler| handler.accepts(level))
}

pub(super) fn write(handlers: &[Arc<Handler>], record: &LogRecord<'_>) -> io::Result<()> {
    let text = RecordText::default();
    for handler in handlers {
        if handler.write(record, &text, None)? {
            break;
        }
    }
    Ok(())
}
