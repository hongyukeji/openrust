use std::{
    fmt::Write as _,
    io::{self, Read as _},
    path::{Path, PathBuf},
};

use cap_std::{ambient_authority, fs::Dir};
use tokio::io::{AsyncRead, AsyncWriteExt as _};

use super::{
    FilesystemError, FilesystemResult, LocalPermissions, Visibility,
    filesystem_adapter::validate_path,
};

/// Performs confined local operations relative to the disk root.
pub struct LocalFilesystemAdapter {
    root: PathBuf,
    visibility: Visibility,
    directory_visibility: Visibility,
    permissions: LocalPermissions,
}

impl LocalFilesystemAdapter {
    pub(super) fn new(
        root: PathBuf,
        visibility: Visibility,
        directory_visibility: Visibility,
        permissions: LocalPermissions,
    ) -> Self {
        Self {
            root,
            visibility,
            directory_visibility,
            permissions,
        }
    }

    pub(super) fn root(&self) -> &Path {
        &self.root
    }

    fn open_directory(&self, path: &Path, create: bool) -> io::Result<Dir> {
        let mode = self.permissions.directory_mode(self.directory_visibility);
        let mut dir = match Dir::open_ambient_dir(&self.root, ambient_authority()) {
            Err(error) if create && error.kind() == io::ErrorKind::NotFound => {
                let parent = self.root.parent().ok_or(io::ErrorKind::InvalidInput)?;
                let name = self.root.file_name().ok_or(io::ErrorKind::InvalidInput)?;
                let mut builder = std::fs::DirBuilder::new();
                builder.recursive(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::DirBuilderExt as _;
                    builder.mode(mode);
                }
                builder.create(parent)?;
                let parent = Dir::open_ambient_dir(parent, ambient_authority())?;
                open_child(&parent, Path::new(name), true, mode)?
            }
            result => result?,
        };
        for part in path.components() {
            dir = open_child(&dir, Path::new(part.as_os_str()), create, mode)?;
        }
        Ok(dir)
    }

    fn resolve(&self, path: &Path, create: bool) -> io::Result<(Dir, PathBuf)> {
        let directory =
            self.open_directory(path.parent().expect("validated filesystem path"), create)?;
        let name = path.file_name().expect("validated filesystem path").into();
        Ok((directory, name))
    }

    async fn with_file<T: Send + 'static>(
        &self,
        path: PathBuf,
        operation: impl FnOnce(std::fs::File) -> io::Result<T> + Send + 'static,
    ) -> FilesystemResult<T> {
        let disk = self.clone_for_task();
        blocking(move || {
            let (dir, name) = disk.resolve(&path, false)?;
            operation(open_regular(&dir, &name)?)
        })
        .await
    }

    pub(super) async fn read(&self, path: PathBuf) -> FilesystemResult<tokio::fs::File> {
        self.with_file(path, Ok)
            .await
            .map(tokio::fs::File::from_std)
    }

    pub(super) async fn metadata(&self, path: PathBuf) -> FilesystemResult<std::fs::Metadata> {
        self.with_file(path, |file| file.metadata()).await
    }

    pub(super) async fn mime_type(&self, path: PathBuf) -> FilesystemResult<String> {
        let fallback = mime_guess::from_path(&path).first_or_octet_stream();
        self.with_file(path, move |file| {
            let mut prefix = Vec::new();
            file.take(8192).read_to_end(&mut prefix)?;
            Ok(infer::get(&prefix).map_or_else(
                || fallback.essence_str().to_owned(),
                |kind| kind.mime_type().to_owned(),
            ))
        })
        .await
    }

    pub(super) async fn checksum<D: md5::Digest>(&self, path: PathBuf) -> FilesystemResult<String> {
        self.with_file(path, |mut file| {
            let mut digest = D::new();
            let mut buffer = [0; 16 * 1024];
            loop {
                let size = match file.read(&mut buffer) {
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                    result => result?,
                };
                if size == 0 {
                    break;
                }
                digest.update(&buffer[..size]);
            }
            let digest = digest.finalize();
            let mut hex = String::with_capacity(digest.len() * 2);
            for byte in digest {
                write!(hex, "{byte:02x}").expect("writing to a String");
            }
            Ok(hex)
        })
        .await
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
        let mode = self
            .permissions
            .file_mode(visibility.unwrap_or(self.visibility));
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
            set_mode(&file, mode)?;
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

    pub(super) async fn list(
        &self,
        path: String,
        recursive: bool,
        directories: bool,
    ) -> FilesystemResult<Vec<String>> {
        let disk = self.clone_for_task();
        blocking(move || {
            let directory = match disk.open_directory(Path::new(&path), false) {
                Ok(directory) => directory,
                Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
                Err(error) => return Err(error),
            };
            let mut found = Vec::new();
            let entries = directory.entries()?;
            let mut stack = vec![(path, directory, entries)];
            while let Some((prefix, parent, entries)) = stack.last_mut() {
                let Some(entry) = entries.next() else {
                    stack.pop();
                    continue;
                };
                let entry = entry?;
                let name = entry.file_name();
                let Some(name) = name.to_str().filter(|name| validate_path(name).is_ok()) else {
                    continue;
                };
                let kind = entry.file_type()?;
                if !kind.is_file() && !kind.is_dir() {
                    continue;
                }
                let relative = if prefix.is_empty() {
                    name.to_owned()
                } else {
                    format!("{prefix}/{name}")
                };
                let child = if recursive && kind.is_dir() {
                    Some(open_child(parent, Path::new(name), false, 0)?)
                } else {
                    None
                };

                if let Some(child) = child {
                    let entries = child.entries()?;
                    if directories {
                        found.push(relative.clone());
                    }
                    stack.push((relative, child, entries));
                } else if kind.is_dir() == directories {
                    found.push(relative);
                }
            }
            if !directories {
                found.sort_unstable();
            }
            Ok(found)
        })
        .await
    }

    pub(super) async fn make_directory(&self, path: PathBuf) -> FilesystemResult<()> {
        let disk = self.clone_for_task();
        blocking(move || disk.open_directory(&path, true).map(drop)).await
    }

    pub(super) async fn delete_directory(&self, path: PathBuf) -> FilesystemResult<()> {
        let disk = self.clone_for_task();
        let result = blocking(move || {
            let (parent, name) = disk.resolve(&path, false)?;
            if !parent.symlink_metadata(&name)?.is_dir() {
                return Err(io::ErrorKind::NotADirectory.into());
            }
            parent.remove_dir_all(name)
        })
        .await;
        match result {
            Err(FilesystemError::NotFound) => Ok(()),
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
        let permissions = self.permissions;
        self.with_file(path, move |file| read_visibility(&file, permissions))
            .await
    }

    pub(super) async fn set_visibility(
        &self,
        path: PathBuf,
        visibility: Visibility,
    ) -> FilesystemResult<()> {
        #[cfg(unix)]
        {
            let mode = self.permissions.file_mode(visibility);
            self.with_file(path, move |file| set_mode(&file, mode))
                .await
        }
        #[cfg(not(unix))]
        {
            let _ = (path, visibility);
            Err(FilesystemError::UnsupportedOperation)
        }
    }

    fn clone_for_task(&self) -> Self {
        Self::new(
            self.root.clone(),
            self.visibility,
            self.directory_visibility,
            self.permissions,
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
fn open_child(parent: &Dir, name: &Path, create: bool, mode: u32) -> io::Result<Dir> {
    use rustix::fs::{Mode, OFlags, fchmod, mkdirat, openat};

    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let mode = Mode::from_raw_mode(mode as _);
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
fn open_child(parent: &Dir, name: &Path, create: bool, _: u32) -> io::Result<Dir> {
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

pub(crate) fn open_public_file(
    root: &Dir,
    path: &Path,
    permissions: LocalPermissions,
) -> io::Result<std::fs::File> {
    let mut directory = root.try_clone()?;
    public_directory(&directory, permissions)?;
    for component in path.parent().into_iter().flat_map(Path::components) {
        directory = open_child(&directory, Path::new(component.as_os_str()), false, 0)?;
        public_directory(&directory, permissions)?;
    }
    let file = open_regular(
        &directory,
        Path::new(path.file_name().ok_or(io::ErrorKind::InvalidInput)?),
    )?;
    #[cfg(unix)]
    {
        let mode = file_mode(&file)?;
        if mode & 0o004 == 0 || permissions.file_visibility(mode) != Visibility::Public {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
    }
    Ok(file)
}

#[cfg(unix)]
fn public_directory(directory: &Dir, permissions: LocalPermissions) -> io::Result<()> {
    use cap_std::fs::PermissionsExt as _;
    let mode = directory.dir_metadata()?.permissions().mode() & 0o777;
    if mode & 0o001 == 0 || mode == permissions.directory_mode(Visibility::Private) {
        return Err(io::ErrorKind::PermissionDenied.into());
    }
    Ok(())
}

#[cfg(not(unix))]
fn public_directory(_: &Dir, _: LocalPermissions) -> io::Result<()> {
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
fn set_mode(file: &std::fs::File, mode: u32) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    file.set_permissions(std::fs::Permissions::from_mode(mode))
}

#[cfg(unix)]
fn file_mode(file: &std::fs::File) -> io::Result<u32> {
    use std::os::unix::fs::PermissionsExt as _;
    Ok(file.metadata()?.permissions().mode())
}

#[cfg(unix)]
fn read_visibility(file: &std::fs::File, permissions: LocalPermissions) -> io::Result<Visibility> {
    Ok(permissions.file_visibility(file_mode(file)?))
}

#[cfg(not(unix))]
fn read_visibility(_: &std::fs::File, _: LocalPermissions) -> io::Result<Visibility> {
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
