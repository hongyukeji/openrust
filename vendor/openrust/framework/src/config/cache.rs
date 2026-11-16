use std::{
    fs::{self, File},
    io::{self, BufReader, Write},
    path::{Path, PathBuf},
};

use super::{ConfigError, Repository};

pub(crate) fn path(base: &Path) -> Result<PathBuf, ConfigError> {
    match std::env::var_os("APP_CONFIG_CACHE") {
        Some(value) if value.is_empty() => Err(ConfigError::InvalidType {
            key: "APP_CONFIG_CACHE".into(),
            expected: "a non-empty path",
        }),
        Some(value) => Ok(base.join(value)),
        None => Ok(base.join("bootstrap/cache/config.json")),
    }
}

pub(crate) fn load(path: &Path) -> Result<Option<Repository>, ConfigError> {
    if !exists(path).map_err(|source| read_failed(path, source))? {
        return Ok(None);
    }
    let file = match File::open(path) {
        Ok(file) => file,
        Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(source) => return Err(read_failed(path, source)),
    };
    serde_json::from_reader(BufReader::new(file))
        .map(|values| Some(Repository::new(values)))
        .map_err(|_| ConfigError::InvalidCache { path: path.into() })
}

pub(crate) fn clear(path: &Path) -> io::Result<()> {
    if exists(path)? {
        match fs::remove_file(path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            result => result,
        }
    } else {
        Ok(())
    }
}

pub(crate) fn save(
    path: &Path,
    config: &dyn crate::contracts::config::Repository,
) -> io::Result<()> {
    exists(path)?;
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("configuration cache needs a parent directory"))?;
    fs::create_dir_all(parent)?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    serde_json::to_writer(&mut file, &config.all())?;
    file.flush()?;
    file.as_file().sync_all()?;
    file.persist(path).map_err(|error| error.error)?;
    Ok(())
}

fn exists(path: &Path) -> io::Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() => Ok(true),
        Ok(_) => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "configuration cache is not a regular file",
        )),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn read_failed(path: &Path, source: io::Error) -> ConfigError {
    ConfigError::ReadFailed {
        path: path.into(),
        source,
    }
}
