use std::{
    io::{self, Read as _, Write as _},
    path::PathBuf,
    sync::Arc,
};

use cap_std::{
    ambient_authority,
    fs::{Dir, OpenOptions},
};

use super::{
    CacheResult, backend,
    engine::{Change, Engine, Record, key_digest, now},
    store::StoreCore,
};

/// Stores cache entries in a shared directory using locked atomic updates.
pub struct FileStore(StoreCore);

super::store::impl_store!(FileStore);

impl FileStore {
    pub fn new(path: PathBuf, lock_path: PathBuf) -> Self {
        let data = Arc::new(FileBackend(path.join("entries")));
        let locks = Arc::new(FileBackend(lock_path.join("locks")));
        Self(StoreCore::new(
            Engine::File(data),
            Engine::File(locks),
            String::new(),
            false,
        ))
    }
}

pub(super) struct FileBackend(PathBuf);

impl FileBackend {
    fn lock_root(&self) -> io::Result<(Dir, std::fs::File)> {
        std::fs::create_dir_all(&self.0).map_err(io_context("create cache directory"))?;
        let root = Dir::open_ambient_dir(&self.0, ambient_authority())
            .map_err(io_context("open cache directory"))?;
        let gate = open_gate(&root).map_err(io_context("open cache gate"))?;
        if !gate.metadata()?.is_file() {
            return Err(io::Error::other("invalid cache gate"));
        }
        gate.lock().map_err(io_context("lock cache gate"))?;
        Ok((root, gate))
    }

    pub(super) fn change<T>(
        &self,
        key: &str,
        edit: impl FnOnce(Option<Record>) -> CacheResult<(Change, T)>,
    ) -> CacheResult<T> {
        let (root, _gate) = self.lock_root().map_err(backend)?;
        let name = format!("{}.record", key_digest(key));
        let entry = match root.open_with(&name, private_options().read(true)) {
            Ok(mut file) => {
                if !file.metadata().map_err(backend)?.is_file() {
                    return Err(backend(io::Error::other("invalid cache entry")));
                }
                let mut bytes = Vec::new();
                file.read_to_end(&mut bytes).map_err(backend)?;
                let record: Record = serde_json::from_slice(&bytes).map_err(backend)?;
                if record.expires <= now()? {
                    root.remove_file(&name).map_err(backend)?;
                    None
                } else {
                    Some(record)
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(backend(error)),
        };
        let present = entry.is_some();
        let (change, result) = edit(entry)?;
        match change {
            Change::Keep => {}
            Change::Remove if present => root.remove_file(&name).map_err(backend)?,
            Change::Remove => {}
            Change::Write(record) => {
                let temporary = format!(".write-{:032x}", fastrand::u128(..));
                let mut options = private_options();
                options.write(true).create_new(true);
                let mut file = root
                    .open_with(&temporary, &options)
                    .map_err(io_context("create cache entry"))
                    .map_err(backend)?;
                let written = (|| {
                    serde_json::to_writer(&mut file, &record).map_err(backend)?;
                    file.flush().map_err(backend)?;
                    drop(file);
                    root.rename(&temporary, &root, name)
                        .map_err(io_context("publish cache entry"))
                        .map_err(backend)
                })();
                if written.is_err() {
                    let _ = root.remove_file(&temporary);
                }
                written?;
            }
        }
        Ok(result)
    }

    pub(super) fn flush(&self) -> CacheResult<()> {
        let (root, _gate) = self.lock_root().map_err(backend)?;
        for entry in root.entries().map_err(backend)? {
            let entry = entry.map_err(backend)?;
            let name = entry.file_name();
            if name
                .to_str()
                .and_then(|name| name.strip_suffix(".record"))
                .is_some_and(|name| {
                    name.len() == 64 && name.bytes().all(|byte| byte.is_ascii_hexdigit())
                })
                && entry.file_type().map_err(backend)?.is_file()
            {
                root.remove_file(name).map_err(backend)?;
            }
        }
        Ok(())
    }
}

#[cfg(unix)]
fn open_gate(root: &Dir) -> io::Result<std::fs::File> {
    use rustix::fs::{Mode, OFlags, openat};
    let flags = OFlags::RDWR | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC;
    let file = match openat(root, ".gate", flags, Mode::empty()) {
        Ok(file) => file,
        Err(rustix::io::Errno::NOENT) => {
            // Exclusive creation keeps competing openers on the same lock inode.
            match openat(
                root,
                ".gate",
                flags | OFlags::CREATE | OFlags::EXCL,
                Mode::RUSR | Mode::WUSR,
            ) {
                Ok(file) => file,
                Err(rustix::io::Errno::EXIST) => openat(root, ".gate", flags, Mode::empty())?,
                Err(error) => return Err(error.into()),
            }
        }
        Err(error) => return Err(error.into()),
    };
    Ok(file.into())
}

#[cfg(not(unix))]
fn open_gate(root: &Dir) -> io::Result<std::fs::File> {
    root.open_with(
        ".gate",
        OpenOptions::new().read(true).write(true).create(true),
    )
    .map(cap_std::fs::File::into_std)
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

fn io_context(operation: &'static str) -> impl FnOnce(io::Error) -> io::Error {
    move |error| io::Error::new(error.kind(), format!("{operation}: {error}"))
}
