use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs::{self, File},
    io::{self, BufReader, ErrorKind},
    path::{Path, PathBuf},
};

use crate::foundation::bootstrap::LoadEnvironmentVariables;

use super::{ConfigError, Environment, Value};

pub(crate) struct ConfigurationCache {
    pub(crate) path: PathBuf,
    pub(crate) environment: Environment,
    pub(crate) values: Option<BTreeMap<String, Value>>,
}

impl ConfigurationCache {
    pub(crate) fn bootstrap(
        base: &Path,
        selected: Option<&str>,
        use_cache: bool,
        testing: bool,
    ) -> Result<Self, ConfigError> {
        let mut environment = LoadEnvironmentVariables::snapshot(base, selected, testing)?;
        let mut path = Self::path(base, &environment);
        let mut values = if use_cache { Self::read(&path)? } else { None };
        if values.is_none() {
            environment = LoadEnvironmentVariables::load(environment)?;
            let resolved = Self::path(base, &environment);
            if use_cache && resolved != path {
                values = Self::read(&resolved)?;
            }
            path = resolved;
        }
        Ok(Self {
            path,
            environment,
            values,
        })
    }

    fn path(base: &Path, environment: &Environment) -> PathBuf {
        match environment
            .values
            .get("APP_CONFIG_CACHE")
            .map(OsString::as_os_str)
            .filter(|path| !path.is_empty())
        {
            Some(path) => base.join(path),
            None => base.join("storage/framework/config.json"),
        }
    }

    pub(crate) fn read(path: &Path) -> Result<Option<BTreeMap<String, Value>>, ConfigError> {
        let read_failed = |source| ConfigError::ReadFailed {
            path: path.to_owned(),
            source,
        };
        if !regular_file_exists(path).map_err(read_failed)? {
            return Ok(None);
        }
        let file = match File::open(path) {
            Ok(file) => file,
            Err(source) if source.kind() == ErrorKind::NotFound => return Ok(None),
            Err(source) => return Err(read_failed(source)),
        };
        serde_json::from_reader(BufReader::new(file))
            .map(Some)
            .map_err(|_| ConfigError::InvalidCache {
                path: path.to_owned(),
            })
    }

    pub(crate) fn clear(path: &Path) -> std::io::Result<()> {
        if !regular_file_exists(path)? {
            return Ok(());
        }
        match fs::remove_file(path) {
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
            result => result,
        }
    }
}

fn regular_file_exists(path: &Path) -> io::Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() => Ok(true),
        Ok(_) => Err(io::Error::new(
            ErrorKind::InvalidInput,
            "configuration cache is not a regular file",
        )),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}
