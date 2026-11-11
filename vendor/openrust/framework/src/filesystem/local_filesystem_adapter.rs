use std::{
    io,
    path::{Path, PathBuf},
};

use cap_std::{ambient_authority, fs::Dir};
use tokio::io::{AsyncRead, AsyncWriteExt as _};

use super::{FilesystemError, FilesystemResult, Visibility};

/// Performs confined local operations relative to the disk root.
pub struct LocalFilesystemAdapter {
    root: PathBuf,
    visibility: Visibility,
    directory_visibility: Visibility,
}

impl LocalFilesystemAdapter {
    pub(super) fn new(
        root: PathBuf,
        visibility: Visibility,
        directory_visibility: Visibility,
    ) -> Self {
        Self {
            root,
            visibility,
            directory_visibility,
        }
    }

    pub(super) fn root(&self) -> &Path {
        &self.root
    }

    fn resolve(&self, path: &Path, create: bool) -> io::Result<(Dir, PathBuf)> {
        if create {
            let mut builder = std::fs::DirBuilder::new();
            builder.recursive(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt as _;
                builder.mode(self.directory_visibility.directory_mode());
            }
            builder.create(&self.root)?;
        }
        let mut dir = Dir::open_ambient_dir(&self.root, ambient_authority())?;
        for part in path.parent().into_iter().flat_map(Path::components) {
            dir = open_child(
                &dir,
                Path::new(part.as_os_str()),
                create,
                self.directory_visibility,
            )?;
        }
        let name = path.file_name().expect("validated filesystem path").into();
        Ok((dir, name))
    }

    pub(super) async fn read(&self, path: PathBuf) -> FilesystemResult<tokio::fs::File> {
        let disk = self.clone_for_task();
        let file = blocking(move || {
            let (dir, name) = disk.resolve(&path, false)?;
            open_regular(&dir, &name)
        })
        .await?;
        Ok(tokio::fs::File::from_std(file))
    }

    pub(super) async fn write(
        &self,
        path: PathBuf,
        reader: &mut (impl AsyncRead + Unpin),
        visibility: Option<Visibility>,
    ) -> FilesystemResult<()> {
        #[cfg(not(unix))]
        if visibility.is_some() {
            return Err(FilesystemError::UnsupportedOperation);
        }
        #[cfg(unix)]
        let visibility = visibility.unwrap_or(self.visibility);
        let disk = self.clone_for_task();
        let (stage, file) = blocking(move || {
            let (parent, destination) = disk.resolve(&path, true)?;
            StagedWrite::create(parent, destination)
        })
        .await?;
        let mut writer = tokio::fs::File::from_std(file);
        tokio::io::copy(reader, &mut writer).await?;
        writer.flush().await?;
        let file = writer.into_std().await;
        blocking(move || {
            #[cfg(unix)]
            set_visibility(&file, visibility)?;
            drop(file);
            stage.publish()
        })
        .await
    }

    pub(super) async fn exists(
        &self,
        path: PathBuf,
        directory: Option<bool>,
    ) -> FilesystemResult<bool> {
        let disk = self.clone_for_task();
        let result = blocking(move || {
            let (parent, name) = disk.resolve(&path, false)?;
            let metadata = parent.symlink_metadata(name)?;
            Ok(match directory {
                Some(true) => metadata.is_dir(),
                Some(false) => metadata.is_file(),
                None => metadata.is_file() || metadata.is_dir(),
            })
        })
        .await;
        match result {
            Err(FilesystemError::NotFound) => Ok(false),
            result => result,
        }
    }

    pub(super) async fn delete(&self, path: PathBuf) -> FilesystemResult<()> {
        let disk = self.clone_for_task();
        let result = blocking(move || {
            let (parent, name) = disk.resolve(&path, false)?;
            parent.remove_file(name)
        })
        .await;
        match result {
            Err(FilesystemError::NotFound) => Ok(()),
            result => result,
        }
    }

    pub(super) async fn get_visibility(&self, path: PathBuf) -> FilesystemResult<Visibility> {
        let disk = self.clone_for_task();
        blocking(move || {
            let (parent, name) = disk.resolve(&path, false)?;
            read_visibility(&open_regular(&parent, &name)?)
        })
        .await
    }

    pub(super) async fn set_visibility(
        &self,
        path: PathBuf,
        visibility: Visibility,
    ) -> FilesystemResult<()> {
        let disk = self.clone_for_task();
        blocking(move || {
            let (parent, name) = disk.resolve(&path, false)?;
            set_visibility(&open_regular(&parent, &name)?, visibility)
        })
        .await
    }

    fn clone_for_task(&self) -> Self {
        Self::new(
            self.root.clone(),
            self.visibility,
            self.directory_visibility,
        )
    }
}

async fn blocking<T: Send + 'static>(
    operation: impl FnOnce() -> io::Result<T> + Send + 'static,
) -> FilesystemResult<T> {
    tokio::task::spawn_blocking(operation)
        .await
        .map_err(|error| FilesystemError::Backend {
            source: Box::new(error),
        })?
        .map_err(FilesystemError::from)
}

#[cfg(unix)]
fn open_child(parent: &Dir, name: &Path, create: bool, visibility: Visibility) -> io::Result<Dir> {
    use rustix::fs::{Mode, OFlags, fchmod, mkdirat, openat};

    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let mode = Mode::from_raw_mode(visibility.directory_mode() as _);
    let directory = match openat(parent, name, flags, Mode::empty()) {
        Ok(directory) => directory,
        Err(rustix::io::Errno::NOENT) if create => {
            let created = match mkdirat(parent, name, mode) {
                Ok(()) => true,
                Err(rustix::io::Errno::EXIST) => false,
                Err(error) => return Err(error.into()),
            };
            let directory = openat(parent, name, flags, Mode::empty())?;
            if created {
                fchmod(&directory, mode)?;
            }
            directory
        }
        Err(error) => return Err(error.into()),
    };
    Ok(Dir::from_std_file(directory.into()))
}

#[cfg(not(unix))]
fn open_child(parent: &Dir, name: &Path, create: bool, _: Visibility) -> io::Result<Dir> {
    match parent.open_dir(name) {
        Err(error) if error.kind() == io::ErrorKind::NotFound && create => {
            if let Err(error) = parent.create_dir(name)
                && error.kind() != io::ErrorKind::AlreadyExists
            {
                return Err(error);
            }
            parent.open_dir(name)
        }
        result => result,
    }
}

pub(crate) fn open_public_file(root: &Dir, path: &Path) -> io::Result<std::fs::File> {
    let mut directory = root.try_clone()?;
    public_directory(&directory)?;
    for component in path.parent().into_iter().flat_map(Path::components) {
        directory = open_child(
            &directory,
            Path::new(component.as_os_str()),
            false,
            Visibility::Public,
        )?;
        public_directory(&directory)?;
    }
    let file = open_regular(
        &directory,
        Path::new(path.file_name().ok_or(io::ErrorKind::InvalidInput)?),
    )?;
    #[cfg(unix)]
    if read_visibility(&file)? != Visibility::Public {
        return Err(io::ErrorKind::PermissionDenied.into());
    }
    Ok(file)
}

#[cfg(unix)]
fn public_directory(directory: &Dir) -> io::Result<()> {
    use cap_std::fs::PermissionsExt as _;
    if directory.dir_metadata()?.permissions().mode() & 0o001 == 0 {
        return Err(io::ErrorKind::PermissionDenied.into());
    }
    Ok(())
}

#[cfg(not(unix))]
fn public_directory(_: &Dir) -> io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn open_regular(parent: &Dir, name: &Path) -> io::Result<std::fs::File> {
    use rustix::fs::{Mode, OFlags, openat};
    use std::os::unix::fs::MetadataExt as _;

    let flags = OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC;
    let file = std::fs::File::from(openat(parent, name, flags, Mode::empty())?);
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.nlink() != 1 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "expected a regular file with one link",
        ));
    }
    Ok(file)
}

#[cfg(not(unix))]
fn open_regular(parent: &Dir, name: &Path) -> io::Result<std::fs::File> {
    if !parent.symlink_metadata(name)?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "expected a regular file",
        ));
    }
    Ok(parent.open(name)?.into_std())
}

#[cfg(unix)]
fn set_visibility(file: &std::fs::File, visibility: Visibility) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    file.set_permissions(std::fs::Permissions::from_mode(visibility.file_mode()))
}

#[cfg(unix)]
fn read_visibility(file: &std::fs::File) -> io::Result<Visibility> {
    use std::os::unix::fs::PermissionsExt as _;
    Ok(if file.metadata()?.permissions().mode() & 0o004 == 0 {
        Visibility::Private
    } else {
        Visibility::Public
    })
}

#[cfg(not(unix))]
fn set_visibility(_: &std::fs::File, _: Visibility) -> io::Result<()> {
    Err(io::ErrorKind::Unsupported.into())
}

#[cfg(not(unix))]
fn read_visibility(_: &std::fs::File) -> io::Result<Visibility> {
    Err(io::ErrorKind::Unsupported.into())
}

const STAGING_PREFIX: &str = ".storage-write-";

pub(crate) fn is_staging_name(name: &str) -> bool {
    name.get(..STAGING_PREFIX.len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(STAGING_PREFIX))
}

struct StagedWrite {
    parent: Dir,
    name: String,
    destination: PathBuf,
}

impl StagedWrite {
    fn create(parent: Dir, destination: PathBuf) -> io::Result<(Self, std::fs::File)> {
        let mut options = cap_std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use cap_std::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        let (name, file) = loop {
            let name = format!("{STAGING_PREFIX}{:032x}", fastrand::u128(..));
            match parent.open_with(&name, &options) {
                Ok(file) => break (name, file.into_std()),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        };
        Ok((
            Self {
                parent,
                name,
                destination,
            },
            file,
        ))
    }

    fn publish(mut self) -> io::Result<()> {
        self.parent
            .rename(&self.name, &self.parent, &self.destination)?;
        self.name.clear();
        Ok(())
    }
}

impl Drop for StagedWrite {
    fn drop(&mut self) {
        if !self.name.is_empty() {
            let _ = self.parent.remove_file(&self.name);
        }
    }
}
