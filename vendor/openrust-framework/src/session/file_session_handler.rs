use std::{
    fs, io,
    io::{Read as _, Write as _},
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use crate::contracts::session::SessionHandler;
use cap_std::{
    ambient_authority,
    fs::{Dir, OpenOptions},
};

use super::{
    SessionError, SessionFuture, SessionResult,
    store::{MAX_PAYLOAD_BYTES, valid_id},
};

/// Stores one session per file, expiring records by their modification time.
pub struct FileSessionHandler {
    directory: PathBuf,
    lifetime: Duration,
}

impl FileSessionHandler {
    pub fn new(directory: PathBuf, lifetime: Duration) -> SessionResult<Self> {
        if directory.as_os_str().is_empty() {
            return Err(SessionError::Configuration);
        }
        Ok(Self {
            directory,
            lifetime,
        })
    }

    async fn with_id<T: Send + 'static>(
        &self,
        id: &str,
        operation: impl FnOnce(&Path, &str) -> SessionResult<T> + Send + 'static,
    ) -> SessionResult<T> {
        if !valid_id(id) {
            return Err(SessionError::Encoding);
        }
        let directory = self.directory.clone();
        let id = id.to_owned();
        on_disk(move || operation(&directory, &id)).await
    }
}

impl SessionHandler for FileSessionHandler {
    fn read<'a>(&'a mut self, id: &'a str) -> SessionFuture<'a, SessionResult<Option<Vec<u8>>>> {
        let lifetime = self.lifetime;
        Box::pin(self.with_id(id, move |directory, id| {
            let Some(root) = open_root(directory)? else {
                return Ok(None);
            };
            let file = match root.open_with(id, private_options().read(true)) {
                Ok(file) => file,
                Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
                Err(error) => return Err(storage(error)),
            };
            let metadata = file.metadata().map_err(storage)?;
            if !metadata.is_file() {
                return Err(SessionError::Storage);
            }
            let modified = metadata.modified().map_err(storage)?.into_std();
            if SystemTime::now()
                .duration_since(modified)
                .is_ok_and(|age| age > lifetime)
            {
                return Ok(None);
            }
            let mut payload = Vec::new();
            file.take(MAX_PAYLOAD_BYTES as u64 + 1)
                .read_to_end(&mut payload)
                .map_err(storage)?;
            if payload.len() > MAX_PAYLOAD_BYTES {
                return Err(SessionError::PayloadTooLarge);
            }
            Ok(Some(payload))
        }))
    }

    fn write<'a>(
        &'a mut self,
        id: &'a str,
        payload: &'a [u8],
    ) -> SessionFuture<'a, SessionResult<()>> {
        Box::pin(async move {
            if payload.len() > MAX_PAYLOAD_BYTES {
                return Err(SessionError::PayloadTooLarge);
            }
            let payload = payload.to_vec();
            self.with_id(id, move |directory, id| {
                fs::create_dir_all(directory).map_err(storage)?;
                let root =
                    Dir::open_ambient_dir(directory, ambient_authority()).map_err(storage)?;
                let _gate = lock(&root)?;
                let temporary = format!(".write-{:032x}", fastrand::u128(..));
                let mut file = root
                    .open_with(&temporary, private_options().write(true).create_new(true))
                    .map_err(storage)?;
                let written = file.write_all(&payload).and_then(|()| file.flush());
                drop(file);
                let published = written.and_then(|()| root.rename(&temporary, &root, id));
                if published.is_err() {
                    let _ = root.remove_file(&temporary);
                }
                published.map_err(storage)
            })
            .await
        })
    }

    fn destroy<'a>(&'a mut self, id: &'a str) -> SessionFuture<'a, SessionResult<()>> {
        Box::pin(self.with_id(id, |directory, id| {
            if let Some(root) = open_root(directory)? {
                let _gate = lock(&root)?;
                remove(&root, id)?;
            }
            Ok(())
        }))
    }

    fn gc(&mut self, lifetime: Duration) -> SessionFuture<'_, SessionResult<Option<u64>>> {
        let directory = self.directory.clone();
        Box::pin(on_disk(move || {
            let Some(root) = open_root(&directory)? else {
                return Ok(Some(0));
            };
            let _gate = lock(&root)?;
            let now = SystemTime::now();
            let mut deleted = 0;
            for entry in root.entries().map_err(storage)? {
                let entry = entry.map_err(storage)?;
                let name = entry.file_name();
                if let Some(id) = name.to_str().filter(|id| valid_id(id))
                    && entry.file_type().map_err(storage)?.is_file()
                {
                    let modified = entry
                        .metadata()
                        .and_then(|metadata| metadata.modified())
                        .map_err(storage)?
                        .into_std();
                    if now
                        .duration_since(modified)
                        .is_ok_and(|age| age >= lifetime)
                        && remove(&root, id)?
                    {
                        deleted += 1;
                    }
                }
            }
            Ok(Some(deleted))
        }))
    }
}

async fn on_disk<T: Send + 'static>(
    operation: impl FnOnce() -> SessionResult<T> + Send + 'static,
) -> SessionResult<T> {
    tokio::task::spawn_blocking(operation)
        .await
        .map_err(|_| SessionError::Storage)?
}

fn open_root(directory: &Path) -> SessionResult<Option<Dir>> {
    match Dir::open_ambient_dir(directory, ambient_authority()) {
        Ok(root) => Ok(Some(root)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(storage(error)),
    }
}

fn lock(root: &Dir) -> SessionResult<fs::File> {
    // Writers and collection share a gate so collection cannot remove a refreshed record.
    let gate = root
        .open_with(
            ".lock",
            private_options().read(true).write(true).create(true),
        )
        .map_err(storage)?
        .into_std();
    if !gate.metadata().map_err(storage)?.is_file() {
        return Err(SessionError::Storage);
    }
    gate.lock().map_err(storage)?;
    Ok(gate)
}

fn private_options() -> OpenOptions {
    #[allow(unused_mut)]
    let mut options = OpenOptions::new();
    #[cfg(unix)]
    {
        use cap_std::fs::OpenOptionsExt as _;
        options.mode(0o600).custom_flags(
            (rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK).bits() as i32,
        );
    }
    options
}

fn remove(root: &Dir, id: &str) -> SessionResult<bool> {
    match root.remove_file(id) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(storage(error)),
    }
}

fn storage(_: io::Error) -> SessionError {
    SessionError::Storage
}
