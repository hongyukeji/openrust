use std::{
    fs::{self, File},
    io::{self, Read as _, Write as _},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

const MAX_BYTES: u64 = 1024;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Receipt {
    pub(super) server: String,
    pub(super) token: String,
}

impl Receipt {
    pub(super) fn publish(
        directory: &Path,
        name: &str,
        server: &str,
        token: &str,
    ) -> io::Result<PathBuf> {
        check_directory(directory)?;
        let directory = directory.canonicalize()?;
        let path = directory.join(format!("{name}.json"));
        let record = Self {
            server: server.into(),
            token: token.into(),
        };
        let mut temporary = tempfile::NamedTempFile::new_in(&directory)?;
        serde_json::to_writer(&mut temporary, &record)?;
        temporary.flush()?;
        temporary.as_file().sync_all()?;
        temporary.persist_noclobber(&path)?;
        #[cfg(unix)]
        File::open(directory)?.sync_all()?;
        Ok(path)
    }

    pub(super) fn read(path: &Path) -> io::Result<Option<(&str, Self)>> {
        let invalid = || io::Error::other("invalid MySQL test schema recovery receipt");
        let name = path
            .file_stem()
            .and_then(|name| name.to_str())
            .ok_or_else(invalid)?;
        if path.extension().is_none_or(|extension| extension != "json")
            || !name.strip_prefix("openrust_test_").is_some_and(valid_hex)
        {
            return Err(invalid());
        }
        check_directory(
            path.parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .unwrap_or(Path::new(".")),
        )?;
        let metadata = match fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        if !metadata.is_file() {
            return Err(invalid());
        }
        check_private(&metadata)?;
        let mut bytes = Vec::new();
        File::open(path)?
            .take(MAX_BYTES + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err(invalid());
        }
        let record: Self = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
        if !valid_hex(&record.token) || uuid::Uuid::parse_str(&record.server).is_err() {
            return Err(invalid());
        }
        Ok(Some((name, record)))
    }

    pub(super) fn remove(path: &Path) -> io::Result<()> {
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    }
}

fn valid_hex(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn check_directory(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() {
        return Err(io::Error::other(
            "expected a private MySQL test recovery directory",
        ));
    }
    check_private(&metadata)
}

fn check_private(metadata: &fs::Metadata) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(io::Error::other(
                "MySQL test recovery paths must be private",
            ));
        }
    }
    #[cfg(not(unix))]
    let _ = metadata;
    Ok(())
}
