use std::{
    cell::LazyCell,
    fs::{self, File, OpenOptions},
    io::{self, Write as _},
    path::PathBuf,
    sync::{Arc, Mutex},
};

use super::Level;

pub(super) type Handlers = Arc<[Arc<Handler>]>;

pub(super) enum Handler {
    File {
        options: FileOptions,
        file: Mutex<Option<File>>,
        level: Level,
        bubble: bool,
    },
    Stderr {
        level: Level,
        bubble: bool,
    },
    Null(Level),
    IgnoreFailures(Handlers),
}

pub(super) struct FileOptions {
    pub path: PathBuf,
    pub permission: Option<u32>,
    pub locking: bool,
}

impl FileOptions {
    fn open(&self) -> io::Result<File> {
        if let Some(parent) = self
            .path
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
        let file = options.open(&self.path)?;
        #[cfg(not(unix))]
        let _ = self.permission;
        #[cfg(unix)]
        if let Some(permission) = self.permission {
            use std::os::unix::fs::PermissionsExt as _;
            file.set_permissions(fs::Permissions::from_mode(permission))?;
        }
        Ok(file)
    }

    fn write(&self, cached: &Mutex<Option<File>>, bytes: &[u8]) -> io::Result<()> {
        let mut cached = cached
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // Retire the handle on any I/O failure so the next record can reopen it.
        let mut file = match cached.take() {
            Some(file) => file,
            None => self.open()?,
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

    fn write(
        &self,
        level: Level,
        line: &LazyCell<String, impl FnOnce() -> String>,
    ) -> io::Result<bool> {
        match self {
            Self::File {
                options,
                file,
                level: minimum,
                bubble,
            } if level >= *minimum => {
                options.write(file, line.as_bytes())?;
                Ok(!bubble)
            }
            Self::Stderr {
                level: minimum,
                bubble,
            } if level >= *minimum => {
                io::stderr().lock().write_all(line.as_bytes())?;
                Ok(!bubble)
            }
            Self::Null(minimum) => Ok(level >= *minimum),
            Self::IgnoreFailures(handlers) => {
                for handler in handlers.iter() {
                    let _ = handler.write(level, line);
                }
                Ok(false)
            }
            _ => Ok(false),
        }
    }
}

pub(super) fn write(
    handlers: &[Arc<Handler>],
    level: Level,
    render: impl FnOnce() -> String,
) -> io::Result<()> {
    let line = LazyCell::new(render);
    for handler in handlers {
        if handler.write(level, &line)? {
            break;
        }
    }
    Ok(())
}
