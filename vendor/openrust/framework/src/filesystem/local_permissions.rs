use serde::Deserialize;

use super::{FilesystemError, FilesystemResult, Visibility};

/// Optional Unix mode overrides for files and directories on a local disk.
#[derive(Clone, Copy, Debug, Default, Deserialize)]
pub struct LocalPermissions {
    file: Option<PermissionModes>,
    dir: Option<PermissionModes>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
struct PermissionModes {
    public: Option<u32>,
    private: Option<u32>,
}

impl LocalPermissions {
    /// Sets the public and private modes for files and directories, respectively.
    pub fn new(
        file_public: u32,
        file_private: u32,
        directory_public: u32,
        directory_private: u32,
    ) -> FilesystemResult<Self> {
        let permissions = Self {
            file: Some(PermissionModes {
                public: Some(file_public),
                private: Some(file_private),
            }),
            dir: Some(PermissionModes {
                public: Some(directory_public),
                private: Some(directory_private),
            }),
        };
        permissions.validate()?;
        Ok(permissions)
    }

    pub(crate) fn from_config(value: Option<serde_json::Value>) -> FilesystemResult<Self> {
        let permissions = serde_json::from_value::<Option<Self>>(value.unwrap_or_default())
            .map_err(|_| FilesystemError::InvalidConfiguration)?
            .unwrap_or_default();
        permissions.validate()?;
        Ok(permissions)
    }

    pub(super) fn validate(self) -> FilesystemResult<()> {
        let file = self.file.unwrap_or_default();
        let dir = self.dir.unwrap_or_default();
        let overrides = [file.public, file.private, dir.public, dir.private];
        if overrides.into_iter().flatten().any(|mode| mode > 0o777) {
            return Err(FilesystemError::InvalidConfiguration);
        }
        #[cfg(not(unix))]
        if overrides.into_iter().any(|mode| mode.is_some()) {
            return Err(FilesystemError::UnsupportedOperation);
        }
        Ok(())
    }

    #[cfg(unix)]
    pub(super) fn file_mode(self, visibility: Visibility) -> u32 {
        let file = self.file.unwrap_or_default();
        match visibility {
            Visibility::Public => file.public.unwrap_or(0o644),
            Visibility::Private => file.private.unwrap_or(0o600),
        }
    }

    pub(super) fn directory_mode(self, visibility: Visibility) -> u32 {
        let dir = self.dir.unwrap_or_default();
        match visibility {
            Visibility::Public => dir.public.unwrap_or(0o755),
            Visibility::Private => dir.private.unwrap_or(0o700),
        }
    }

    #[cfg(unix)]
    pub(super) fn file_visibility(self, mode: u32) -> Visibility {
        let mode = mode & 0o777;
        if mode == self.file_mode(Visibility::Private) {
            Visibility::Private
        } else if mode == self.file_mode(Visibility::Public) || mode & 0o004 != 0 {
            Visibility::Public
        } else {
            Visibility::Private
        }
    }
}
