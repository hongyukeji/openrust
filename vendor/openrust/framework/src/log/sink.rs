use std::{
    cell::LazyCell,
    fs::{self, File, OpenOptions},
    io::{self, Write as _},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use std::borrow::Cow;

use super::{Formatter, Level, LogRecord, rotating::RotatingFile};
use crate::contracts::log::Logger;

pub(super) type Handlers = Arc<[Arc<Handler>]>;

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
    Null(Level),
    IgnoreFailures(Handlers),
    Formatted {
        handler: Arc<Handler>,
        formatter: Arc<Formatter>,
    },
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
        options.create(true).append(true);
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
            | Self::Null(minimum) => level >= *minimum,
            Self::IgnoreFailures(handlers) => accepts(handlers, level),
            Self::Formatted { handler, .. } => handler.accepts(level),
        }
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
        message: &LazyCell<Cow<'a, str>, impl FnOnce() -> Cow<'a, str>>,
        line: &LazyCell<String, impl FnOnce() -> String>,
        formatter: Option<&Formatter>,
    ) -> io::Result<bool> {
        let level = record.level;
        let output = || match formatter {
            Some(formatter) => formatter(record).map(Cow::Owned).map_err(io::Error::other),
            None => Ok(Cow::Borrowed(line.as_str())),
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
                file.write(record.at.date_naive(), output()?.as_bytes())?;
                Ok(!bubble)
            }
            Self::Custom {
                logger,
                level: minimum,
                bubble,
            } if level >= *minimum => {
                logger
                    .logWithContext(level, message, &record.context, &record.extra)
                    .map_err(io::Error::other)?;
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
            Self::Null(minimum) => Ok(level >= *minimum),
            Self::IgnoreFailures(handlers) => {
                for handler in handlers.iter() {
                    let _ = handler.write(record, message, line, formatter);
                }
                Ok(false)
            }
            Self::Formatted {
                handler,
                formatter: default,
            } => handler.write(record, message, line, formatter.or(Some(default.as_ref()))),
            _ => Ok(false),
        }
    }
}

pub(super) fn accepts(handlers: &[Arc<Handler>], level: Level) -> bool {
    handlers.iter().any(|handler| handler.accepts(level))
}

pub(super) fn write(handlers: &[Arc<Handler>], record: &LogRecord<'_>) -> io::Result<()> {
    let message = LazyCell::new(|| record.message());
    let line = LazyCell::new(|| record.format_line(&message));
    for handler in handlers {
        if handler.write(record, &message, &line, None)? {
            break;
        }
    }
    Ok(())
}
